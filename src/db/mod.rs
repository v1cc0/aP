pub mod models;
pub mod queries;

use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use tokio::sync::Semaphore;

#[derive(Debug, Clone)]
pub struct DbPool {
    db: turso::Database,
    permits: Arc<Semaphore>,
    #[allow(dead_code)]
    max_connections: u32,
    begin_concurrent: bool,
}

impl DbPool {
    async fn conn(&self) -> Result<turso::Connection> {
        Ok(self.db.connect()?)
    }

    pub fn size(&self) -> u32 {
        1
    }
    pub fn num_idle(&self) -> usize {
        self.permits.available_permits()
    }
    #[allow(dead_code)]
    pub fn max_connections(&self) -> u32 {
        self.max_connections
    }
    #[allow(dead_code)]
    pub fn begin_concurrent(&self) -> bool {
        self.begin_concurrent
    }

    pub async fn close(&self) {}

    pub async fn execute(&self, sql: &str, params: Vec<turso::Value>) -> Result<u64> {
        let _permit = self.permits.acquire().await?;
        let conn = self.conn().await?;
        Ok(conn.execute(sql, turso::params_from_iter(params)).await?)
    }

    pub async fn execute_write(&self, sql: &str, params: Vec<turso::Value>) -> Result<u64> {
        if self.begin_concurrent {
            self.transaction_write(|conn| {
                let sql = sql.to_string();
                let params = params.clone();
                Box::pin(
                    async move { Ok(conn.execute(&sql, turso::params_from_iter(params)).await?) },
                )
            })
            .await
        } else {
            self.execute(sql, params).await
        }
    }

    pub async fn transaction_write<T, F>(&self, f: F) -> Result<T>
    where
        T: Send,
        F: for<'a> FnOnce(&'a turso::Connection) -> futures::future::BoxFuture<'a, Result<T>>,
    {
        let _permit = self.permits.acquire().await?;
        let conn = self.conn().await?;
        let begin = if self.begin_concurrent {
            "BEGIN CONCURRENT"
        } else {
            "BEGIN IMMEDIATE"
        };
        conn.execute(begin, ()).await?;
        match f(&conn).await {
            Ok(v) => {
                if let Err(e) = conn.execute("COMMIT", ()).await {
                    let _ = conn.execute("ROLLBACK", ()).await;
                    Err(e.into())
                } else {
                    Ok(v)
                }
            }
            Err(e) => {
                let _ = conn.execute("ROLLBACK", ()).await;
                Err(e)
            }
        }
    }

    pub async fn query_all(&self, sql: &str, params: Vec<turso::Value>) -> Result<Vec<DbRow>> {
        let _permit = self.permits.acquire().await?;
        let conn = self.conn().await?;
        query_all_on(&conn, sql, params).await
    }

    pub async fn query_one(&self, sql: &str, params: Vec<turso::Value>) -> Result<DbRow> {
        self.query_all(sql, params)
            .await?
            .into_iter()
            .next()
            .context("query returned no rows")
    }

    pub async fn query_all_write(
        &self,
        sql: &str,
        params: Vec<turso::Value>,
    ) -> Result<Vec<DbRow>> {
        if self.begin_concurrent {
            self.transaction_write(|conn| {
                let sql = sql.to_string();
                let params = params.clone();
                Box::pin(async move { query_all_on(conn, &sql, params).await })
            })
            .await
        } else {
            self.query_all(sql, params).await
        }
    }

    pub async fn query_one_write(&self, sql: &str, params: Vec<turso::Value>) -> Result<DbRow> {
        self.query_all_write(sql, params)
            .await?
            .into_iter()
            .next()
            .context("query returned no rows")
    }

    pub async fn query_optional(
        &self,
        sql: &str,
        params: Vec<turso::Value>,
    ) -> Result<Option<DbRow>> {
        Ok(self.query_all(sql, params).await?.into_iter().next())
    }
}

#[derive(Debug, Clone)]
pub struct DbRow {
    columns: Vec<String>,
    values: Vec<turso::Value>,
}

impl DbRow {
    pub fn get_i64(&self, name: &str) -> Result<i64> {
        match self.value(name)? {
            turso::Value::Integer(v) => Ok(*v),
            turso::Value::Real(v) => Ok(*v as i64),
            turso::Value::Text(v) => Ok(v.parse()?),
            turso::Value::Null => Ok(0),
            v => anyhow::bail!("cannot convert {v:?} to i64 for {name}"),
        }
    }

    pub fn get_i32(&self, name: &str) -> Result<i32> {
        Ok(self.get_i64(name)? as i32)
    }
    pub fn get_f64(&self, name: &str) -> Result<f64> {
        match self.value(name)? {
            turso::Value::Integer(v) => Ok(*v as f64),
            turso::Value::Real(v) => Ok(*v),
            turso::Value::Text(v) => Ok(v.parse()?),
            turso::Value::Null => Ok(0.0),
            v => anyhow::bail!("cannot convert {v:?} to f64 for {name}"),
        }
    }
    pub fn get_bool(&self, name: &str) -> Result<bool> {
        Ok(self.get_i64(name)? != 0)
    }
    pub fn get_string(&self, name: &str) -> Result<String> {
        match self.value(name)? {
            turso::Value::Text(v) => Ok(v.clone()),
            turso::Value::Integer(v) => Ok(v.to_string()),
            turso::Value::Real(v) => Ok(v.to_string()),
            turso::Value::Null => Ok(String::new()),
            v => anyhow::bail!("cannot convert {v:?} to string for {name}"),
        }
    }
    pub fn get_opt_string(&self, name: &str) -> Result<Option<String>> {
        match self.value(name)? {
            turso::Value::Null => Ok(None),
            _ => Ok(Some(self.get_string(name)?)),
        }
    }
    fn value(&self, name: &str) -> Result<&turso::Value> {
        let idx = self
            .columns
            .iter()
            .position(|c| c == name)
            .with_context(|| format!("missing column {name}"))?;
        Ok(&self.values[idx])
    }
}

pub(crate) fn v_str(s: &str) -> turso::Value {
    turso::Value::Text(s.to_string())
}
pub(crate) fn v_string(s: String) -> turso::Value {
    turso::Value::Text(s)
}
pub(crate) fn v_i64(v: i64) -> turso::Value {
    turso::Value::Integer(v)
}
pub(crate) fn v_i32(v: i32) -> turso::Value {
    turso::Value::Integer(v as i64)
}
pub(crate) fn v_f64(v: f64) -> turso::Value {
    turso::Value::Real(v)
}
pub(crate) fn v_bool(v: bool) -> turso::Value {
    turso::Value::Integer(if v { 1 } else { 0 })
}

async fn query_all_on(
    conn: &turso::Connection,
    sql: &str,
    params: Vec<turso::Value>,
) -> Result<Vec<DbRow>> {
    let mut rows = conn.query(sql, turso::params_from_iter(params)).await?;
    let columns = rows.column_names();
    let mut out = Vec::new();
    while let Some(row) = rows.next().await? {
        let mut values = Vec::with_capacity(row.column_count());
        for i in 0..row.column_count() {
            values.push(row.get_value(i)?);
        }
        out.push(DbRow {
            columns: columns.clone(),
            values,
        });
    }
    Ok(out)
}

/// 初始化 Turso 数据库并建表。
pub async fn init(
    database_path: &str,
    pool_size: u32,
    begin_concurrent: bool,
    multiprocess_wal: bool,
) -> Result<DbPool> {
    if database_path != ":memory:" {
        if let Some(parent) = Path::new(database_path).parent() {
            if !parent.as_os_str().is_empty() {
                tokio::fs::create_dir_all(parent).await?;
            }
        }
    }

    let db = turso::Builder::new_local(database_path)
        .experimental_multiprocess_wal(multiprocess_wal)
        .build()
        .await?;
    let pool = DbPool {
        db,
        permits: Arc::new(Semaphore::new(pool_size.max(1) as usize)),
        max_connections: pool_size.max(1),
        begin_concurrent,
    };

    let conn = pool.conn().await?;
    let _ = query_all_on(&conn, "PRAGMA journal_mode = wal", vec![]).await?;
    if begin_concurrent {
        let _ = query_all_on(&conn, "PRAGMA journal_mode = experimental_mvcc", vec![]).await?;
    }
    create_tables(&pool).await?;
    Ok(pool)
}

async fn create_tables(pool: &DbPool) -> Result<()> {
    let statements = [
        "CREATE TABLE IF NOT EXISTS accounts (
            id INTEGER PRIMARY KEY,
            name TEXT NOT NULL DEFAULT '', platform TEXT NOT NULL DEFAULT 'openai', type TEXT NOT NULL DEFAULT 'oauth',
            credentials TEXT NOT NULL DEFAULT '{}', proxy_url TEXT NOT NULL DEFAULT '', status TEXT NOT NULL DEFAULT 'active',
            error_message TEXT NOT NULL DEFAULT '', cooldown_reason TEXT NOT NULL DEFAULT '', cooldown_until INTEGER,
            enable_scheduling INTEGER NOT NULL DEFAULT 1, created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
            updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')))",
        "CREATE INDEX IF NOT EXISTS idx_accounts_status ON accounts(status)",
        "CREATE INDEX IF NOT EXISTS idx_accounts_status_id ON accounts(status, id)",
        "CREATE TABLE IF NOT EXISTS usage_logs (
            id INTEGER PRIMARY KEY, account_id INTEGER NOT NULL DEFAULT 0, endpoint TEXT NOT NULL DEFAULT '', model TEXT NOT NULL DEFAULT '',
            prompt_tokens INTEGER NOT NULL DEFAULT 0, completion_tokens INTEGER NOT NULL DEFAULT 0, total_tokens INTEGER NOT NULL DEFAULT 0,
            input_tokens INTEGER NOT NULL DEFAULT 0, output_tokens INTEGER NOT NULL DEFAULT 0, reasoning_tokens INTEGER NOT NULL DEFAULT 0,
            cached_tokens INTEGER NOT NULL DEFAULT 0, first_token_ms INTEGER NOT NULL DEFAULT 0, reasoning_effort TEXT NOT NULL DEFAULT '',
            status_code INTEGER NOT NULL DEFAULT 0, duration_ms INTEGER NOT NULL DEFAULT 0, stream INTEGER NOT NULL DEFAULT 0,
            service_tier TEXT NOT NULL DEFAULT '', account_email TEXT NOT NULL DEFAULT '', cost REAL NOT NULL DEFAULT 0,
            tt_request_id TEXT NOT NULL DEFAULT '', tt_user_id TEXT NOT NULL DEFAULT '', tt_api_key_id TEXT NOT NULL DEFAULT '',
            tt_group_id TEXT NOT NULL DEFAULT '', tt_provider_account_id TEXT NOT NULL DEFAULT '', tt_provider_platform TEXT NOT NULL DEFAULT '',
            created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')))",
        "CREATE INDEX IF NOT EXISTS idx_usage_logs_created ON usage_logs(created_at)",
        "CREATE INDEX IF NOT EXISTS idx_usage_logs_status ON usage_logs(created_at, status_code)",
        "CREATE TABLE IF NOT EXISTS api_keys (id INTEGER PRIMARY KEY, name TEXT NOT NULL DEFAULT '', key TEXT NOT NULL UNIQUE, created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')))",
        "CREATE TABLE IF NOT EXISTS system_settings (
            id INTEGER PRIMARY KEY DEFAULT 1 CHECK (id = 1), max_concurrency INTEGER NOT NULL DEFAULT 2, global_rpm INTEGER NOT NULL DEFAULT 0,
            test_model TEXT NOT NULL DEFAULT 'gpt-5.4-mini', test_concurrency INTEGER NOT NULL DEFAULT 50, proxy_url TEXT NOT NULL DEFAULT '', admin_secret TEXT NOT NULL DEFAULT '',
            auto_clean_unauthorized INTEGER NOT NULL DEFAULT 0, auto_clean_rate_limited INTEGER NOT NULL DEFAULT 0, auto_clean_full_usage INTEGER NOT NULL DEFAULT 0,
            auto_clean_error INTEGER NOT NULL DEFAULT 0, auto_clean_expired INTEGER NOT NULL DEFAULT 0, fast_scheduler_enabled INTEGER NOT NULL DEFAULT 0,
            max_retries INTEGER NOT NULL DEFAULT 2, pg_max_conns INTEGER NOT NULL DEFAULT 256, proxy_pool_enabled INTEGER NOT NULL DEFAULT 0)",
        "INSERT OR IGNORE INTO system_settings (id) VALUES (1)",
        "CREATE TABLE IF NOT EXISTS usage_stats_baseline (id INTEGER PRIMARY KEY DEFAULT 1 CHECK (id = 1), total_requests INTEGER NOT NULL DEFAULT 0, total_tokens INTEGER NOT NULL DEFAULT 0, prompt_tokens INTEGER NOT NULL DEFAULT 0, completion_tokens INTEGER NOT NULL DEFAULT 0, cached_tokens INTEGER NOT NULL DEFAULT 0)",
        "INSERT OR IGNORE INTO usage_stats_baseline (id) VALUES (1)",
        "CREATE TABLE IF NOT EXISTS account_events (id INTEGER PRIMARY KEY, account_id INTEGER NOT NULL, event_type TEXT NOT NULL DEFAULT '', source TEXT NOT NULL DEFAULT '', created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')))",
        "CREATE INDEX IF NOT EXISTS idx_account_events_created ON account_events(created_at)",
        "CREATE TABLE IF NOT EXISTS proxies (
            id INTEGER PRIMARY KEY,
            url TEXT NOT NULL UNIQUE,
            label TEXT NOT NULL DEFAULT '',
            enabled INTEGER NOT NULL DEFAULT 1,
            created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
            test_ip TEXT NOT NULL DEFAULT '',
            test_location TEXT NOT NULL DEFAULT '',
            test_latency_ms INTEGER NOT NULL DEFAULT 0
        )",
    ];
    for sql in statements {
        pool.execute(sql, vec![]).await?;
    }
    for sql in [
        "ALTER TABLE accounts ADD COLUMN enable_scheduling INTEGER NOT NULL DEFAULT 1",
        "ALTER TABLE usage_logs ADD COLUMN cost REAL NOT NULL DEFAULT 0",
        "ALTER TABLE usage_logs ADD COLUMN tt_request_id TEXT NOT NULL DEFAULT ''",
        "ALTER TABLE usage_logs ADD COLUMN tt_user_id TEXT NOT NULL DEFAULT ''",
        "ALTER TABLE usage_logs ADD COLUMN tt_api_key_id TEXT NOT NULL DEFAULT ''",
        "ALTER TABLE usage_logs ADD COLUMN tt_group_id TEXT NOT NULL DEFAULT ''",
        "ALTER TABLE usage_logs ADD COLUMN tt_provider_account_id TEXT NOT NULL DEFAULT ''",
        "ALTER TABLE usage_logs ADD COLUMN tt_provider_platform TEXT NOT NULL DEFAULT ''",
        "ALTER TABLE system_settings ADD COLUMN pg_max_conns INTEGER NOT NULL DEFAULT 256",
        "ALTER TABLE system_settings ADD COLUMN proxy_pool_enabled INTEGER NOT NULL DEFAULT 0",
    ] {
        let _ = pool.execute(sql, vec![]).await;
    }
    pool.execute(
        "UPDATE system_settings SET pg_max_conns = 256 WHERE id = 1 AND pg_max_conns = 20",
        vec![],
    )
    .await?;
    Ok(())
}
