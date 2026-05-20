use super::models::*;
use super::{v_bool, v_f64, v_i32, v_i64, v_str, v_string, DbPool, DbRow};
use anyhow::Result;
use serde::{Deserialize, Serialize};

fn account_row(r: &DbRow) -> Result<AccountRow> {
    Ok(AccountRow {
        id: r.get_i64("id")?,
        name: r.get_string("name")?,
        platform: r.get_string("platform")?,
        account_type: r.get_string("type")?,
        credentials: r.get_string("credentials")?,
        proxy_url: r.get_string("proxy_url")?,
        status: r.get_string("status")?,
        error_message: r.get_string("error_message")?,
        cooldown_reason: r.get_string("cooldown_reason")?,
        cooldown_until: r.get_opt_string("cooldown_until")?,
        enable_scheduling: r.get_bool("enable_scheduling")?,
        created_at: r.get_string("created_at")?,
        updated_at: r.get_string("updated_at")?,
    })
}

fn usage_log_row(r: &DbRow) -> Result<UsageLogRow> {
    Ok(UsageLogRow {
        id: r.get_i64("id")?, account_id: r.get_i64("account_id")?, endpoint: r.get_string("endpoint")?, model: r.get_string("model")?,
        prompt_tokens: r.get_i32("prompt_tokens")?, completion_tokens: r.get_i32("completion_tokens")?, total_tokens: r.get_i32("total_tokens")?,
        input_tokens: r.get_i32("input_tokens")?, output_tokens: r.get_i32("output_tokens")?, reasoning_tokens: r.get_i32("reasoning_tokens")?, cached_tokens: r.get_i32("cached_tokens")?,
        first_token_ms: r.get_i32("first_token_ms")?, reasoning_effort: r.get_string("reasoning_effort")?, status_code: r.get_i32("status_code")?, duration_ms: r.get_i32("duration_ms")?,
        stream: r.get_bool("stream")?, service_tier: r.get_string("service_tier")?, account_email: r.get_string("account_email")?, created_at: r.get_string("created_at")?,
    })
}

fn settings_row(r: &DbRow) -> Result<SystemSettings> {
    Ok(SystemSettings {
        max_concurrency: r.get_i32("max_concurrency")?, global_rpm: r.get_i32("global_rpm")?, test_model: r.get_string("test_model")?, test_concurrency: r.get_i32("test_concurrency")?,
        proxy_url: r.get_string("proxy_url")?, admin_secret: r.get_string("admin_secret")?, auto_clean_unauthorized: r.get_bool("auto_clean_unauthorized")?, auto_clean_rate_limited: r.get_bool("auto_clean_rate_limited")?,
        auto_clean_full_usage: r.get_bool("auto_clean_full_usage")?, auto_clean_error: r.get_bool("auto_clean_error")?, auto_clean_expired: r.get_bool("auto_clean_expired")?, fast_scheduler_enabled: r.get_bool("fast_scheduler_enabled")?,
        max_retries: r.get_i32("max_retries")?, pg_max_conns: r.get_i32("pg_max_conns")?,
        proxy_pool_enabled: r.get_bool("proxy_pool_enabled")?,
    })
}

/// 批量查询各账号的历史请求统计（启动时恢复内存计数器）
pub async fn get_account_request_counts(pool: &DbPool) -> Result<std::collections::HashMap<i64, (u64, u64)>> {
    let rows = pool.query_all(
        "SELECT account_id, COUNT(*) AS total, COALESCE(SUM(CASE WHEN status_code >= 400 AND status_code != 499 THEN 1 ELSE 0 END), 0) AS errors
         FROM usage_logs WHERE status_code != 499 GROUP BY account_id", vec![]).await?;
    let mut map = std::collections::HashMap::new();
    for row in rows { map.insert(row.get_i64("account_id")?, (row.get_i64("total")? as u64, row.get_i64("errors")? as u64)); }
    Ok(map)
}

pub async fn list_active_accounts(pool: &DbPool) -> Result<Vec<AccountRow>> {
    let rows = pool.query_all(
        "SELECT id, name, platform, type, credentials, proxy_url, status, error_message, cooldown_reason,
                CASE WHEN cooldown_until IS NULL THEN NULL ELSE strftime('%Y-%m-%dT%H:%M:%SZ', cooldown_until, 'unixepoch') END AS cooldown_until,
                COALESCE(enable_scheduling, 1) AS enable_scheduling, created_at, updated_at
         FROM accounts WHERE status = 'active' ORDER BY id", vec![]).await?;
    rows.iter().map(account_row).collect()
}

pub async fn insert_account(pool: &DbPool, name: &str, creds: &Credentials, proxy_url: &str) -> Result<i64> {
    let creds_json = serde_json::to_string(creds)?;
    let row = pool.query_one_write("INSERT INTO accounts (name, credentials, proxy_url) VALUES (?1, ?2, ?3) RETURNING id", vec![v_str(name), v_string(creds_json), v_str(proxy_url)]).await?;
    row.get_i64("id")
}

pub async fn insert_at_account(pool: &DbPool, name: &str, creds: &Credentials, proxy_url: &str) -> Result<i64> {
    let creds_json = serde_json::to_string(creds)?;
    let row = pool.query_one_write("INSERT INTO accounts (name, type, credentials, proxy_url) VALUES (?1, 'at', ?2, ?3) RETURNING id", vec![v_str(name), v_string(creds_json), v_str(proxy_url)]).await?;
    row.get_i64("id")
}

pub async fn get_all_access_tokens(pool: &DbPool) -> Result<std::collections::HashSet<String>> {
    let rows = pool.query_all("SELECT json_extract(credentials, '$.access_token') AS token FROM accounts WHERE status != 'deleted' AND COALESCE(json_extract(credentials, '$.access_token'), '') != ''", vec![]).await?;
    Ok(rows.into_iter().filter_map(|r| r.get_opt_string("token").ok().flatten()).collect())
}

pub async fn get_all_refresh_tokens(pool: &DbPool) -> Result<std::collections::HashSet<String>> {
    let rows = pool.query_all("SELECT json_extract(credentials, '$.refresh_token') AS token FROM accounts WHERE status != 'deleted' AND COALESCE(json_extract(credentials, '$.refresh_token'), '') != ''", vec![]).await?;
    Ok(rows.into_iter().filter_map(|r| r.get_opt_string("token").ok().flatten()).collect())
}

pub async fn batch_delete_accounts(pool: &DbPool, ids: &[i64]) -> Result<i64> {
    if ids.is_empty() { return Ok(0); }
    let placeholders = (1..=ids.len()).map(|i| format!("?{i}")).collect::<Vec<_>>().join(",");
    let sql = format!("UPDATE accounts SET status = 'deleted', updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE id IN ({placeholders}) AND status != 'deleted'");
    let n = pool.execute_write(&sql, ids.iter().copied().map(v_i64).collect()).await?;
    Ok(n as i64)
}

pub async fn update_account_credentials(pool: &DbPool, id: i64, creds: &Credentials) -> Result<()> {
    let creds_json = serde_json::to_string(creds)?;
    pool.execute_write("UPDATE accounts SET credentials = json_patch(credentials, ?1), updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE id = ?2", vec![v_string(creds_json), v_i64(id)]).await?;
    Ok(())
}

pub async fn delete_account(pool: &DbPool, id: i64) -> Result<()> {
    pool.execute_write("UPDATE accounts SET status = 'deleted', updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE id = ?1", vec![v_i64(id)]).await?;
    Ok(())
}

pub async fn update_account_enabled(pool: &DbPool, id: i64, enabled: bool) -> Result<()> {
    pool.execute_write("UPDATE accounts SET enable_scheduling = ?1, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE id = ?2", vec![v_bool(enabled), v_i64(id)]).await?;
    Ok(())
}

pub async fn get_account_by_id(pool: &DbPool, id: i64) -> Result<Option<AccountRow>> {
    let row = pool.query_optional(
        "SELECT id, name, platform, type, credentials, proxy_url, status, error_message, cooldown_reason,
                CASE WHEN cooldown_until IS NULL THEN NULL ELSE strftime('%Y-%m-%dT%H:%M:%SZ', cooldown_until, 'unixepoch') END AS cooldown_until,
                COALESCE(enable_scheduling, 1) AS enable_scheduling, created_at, updated_at
         FROM accounts WHERE id = ?1", vec![v_i64(id)]).await?;
    row.as_ref().map(account_row).transpose()
}

pub async fn batch_insert_usage_logs(pool: &DbPool, logs: &[UsageLog]) -> Result<()> {
    if logs.is_empty() { return Ok(()); }
    const BATCH_SIZE: usize = 20;
    for chunk in logs.chunks(BATCH_SIZE) { insert_usage_logs_chunk(pool, chunk).await?; }
    Ok(())
}

async fn insert_usage_logs_chunk(pool: &DbPool, logs: &[UsageLog]) -> Result<()> {
    let mut query = String::from("INSERT INTO usage_logs (account_id, endpoint, model, prompt_tokens, completion_tokens, total_tokens, input_tokens, output_tokens, reasoning_tokens, cached_tokens, first_token_ms, reasoning_effort, status_code, duration_ms, stream, service_tier, account_email, cost) VALUES ");
    let mut params = Vec::with_capacity(logs.len() * 18);
    let mut p = 1;
    for (i, log) in logs.iter().enumerate() {
        if i > 0 { query.push(','); }
        let qs = (p..p+18).map(|n| format!("?{n}")).collect::<Vec<_>>().join(",");
        query.push('('); query.push_str(&qs); query.push(')'); p += 18;
        params.extend([v_i64(log.account_id), v_str(&log.endpoint), v_str(&log.model), v_i64(log.prompt_tokens), v_i64(log.completion_tokens), v_i64(log.total_tokens), v_i64(log.input_tokens), v_i64(log.output_tokens), v_i64(log.reasoning_tokens), v_i64(log.cached_tokens), v_i64(log.first_token_ms), v_str(&log.reasoning_effort), v_i64(log.status_code), v_i64(log.duration_ms), v_bool(log.stream), v_str(&log.service_tier), v_str(&log.account_email), v_f64(log.cost)]);
    }
    pool.execute_write(&query, params).await?;
    Ok(())
}

pub async fn query_chart_data(pool: &DbPool, range_minutes: i64, bucket_minutes: i64) -> Result<ChartData> {
    let bucket_secs = bucket_minutes * 60;
    let timeline_rows = pool.query_all(
        "SELECT strftime('%Y-%m-%dT%H:%M:%S', datetime((CAST(strftime('%s', created_at) AS INTEGER) / ?1) * ?1, 'unixepoch', '+8 hours')) AS bucket,
                COUNT(*) AS requests, COALESCE(AVG(duration_ms), 0) AS avg_latency, COALESCE(SUM(input_tokens), 0) AS input_tokens,
                COALESCE(SUM(output_tokens), 0) AS output_tokens, COALESCE(SUM(reasoning_tokens), 0) AS reasoning_tokens, COALESCE(SUM(cached_tokens), 0) AS cached_tokens,
                COALESCE(SUM(CASE WHEN status_code = 401 THEN 1 ELSE 0 END), 0) AS errors_401,
                COALESCE(SUM(CASE WHEN status_code >= 200 AND status_code < 300 THEN 1 ELSE 0 END), 0) AS success_200
         FROM usage_logs WHERE created_at >= datetime('now', '-' || ?2 || ' minutes') AND status_code != 499 GROUP BY bucket ORDER BY bucket", vec![v_i64(bucket_secs), v_i64(range_minutes)]).await?;
    let timeline = timeline_rows.iter().map(|r| Ok(ChartBucket { bucket: r.get_string("bucket")?, requests: r.get_i64("requests")?, avg_latency: r.get_f64("avg_latency")?, input_tokens: r.get_i64("input_tokens")?, output_tokens: r.get_i64("output_tokens")?, reasoning_tokens: r.get_i64("reasoning_tokens")?, cached_tokens: r.get_i64("cached_tokens")?, errors_401: r.get_i64("errors_401")?, success_200: r.get_i64("success_200")? })).collect::<Result<Vec<_>>>()?;
    let model_rows = pool.query_all("SELECT model, COUNT(*) AS requests FROM usage_logs WHERE created_at >= datetime('now', '-' || ?1 || ' minutes') AND status_code != 499 AND model != '' GROUP BY model ORDER BY requests DESC LIMIT 10", vec![v_i64(range_minutes)]).await?;
    let models = model_rows.iter().map(|r| Ok(ModelRanking { model: r.get_string("model")?, requests: r.get_i64("requests")? })).collect::<Result<Vec<_>>>()?;
    Ok(ChartData { timeline, models })
}

pub async fn query_usage_logs_filtered(pool: &DbPool, page: i64, page_size: i64, model: Option<&str>, email: Option<&str>, endpoint: Option<&str>, stream: Option<&str>, start: Option<&str>, end: Option<&str>) -> Result<(Vec<UsageLogRow>, i64)> {
    let offset = (page - 1) * page_size;
    let mut where_clauses = vec!["status_code != 499".to_string()];
    let mut params = Vec::new();
    let mut idx = 0usize;
    macro_rules! bind { ($clause:expr, $val:expr) => {{ idx += 1; where_clauses.push(format!($clause, idx)); params.push($val); }}; }
    if let Some(s) = start { bind!("created_at >= ?{}", v_str(s)); }
    if let Some(e) = end { bind!("created_at <= ?{}", v_str(e)); }
    if let Some(m) = model.filter(|m| !m.is_empty()) { bind!("model = ?{}", v_str(m)); }
    if let Some(em) = email.filter(|e| !e.is_empty()) { bind!("LOWER(account_email) LIKE LOWER(?{})", v_string(format!("%{}%", em))); }
    if let Some(ep) = endpoint.filter(|e| !e.is_empty()) { bind!("endpoint = ?{}", v_str(ep)); }
    if let Some(s) = stream { match s { "true" => where_clauses.push("stream = 1".into()), "false" => where_clauses.push("stream = 0".into()), _ => {} } }
    let where_sql = where_clauses.join(" AND ");
    let count_sql = format!("SELECT COUNT(*) AS total FROM usage_logs WHERE {where_sql}");
    let total = pool.query_one(&count_sql, params.clone()).await?.get_i64("total")?;
    let data_sql = format!("SELECT id, account_id, endpoint, model, prompt_tokens, completion_tokens, total_tokens, input_tokens, output_tokens, reasoning_tokens, cached_tokens, first_token_ms, reasoning_effort, status_code, duration_ms, stream, service_tier, account_email, created_at FROM usage_logs WHERE {where_sql} ORDER BY created_at DESC LIMIT {page_size} OFFSET {offset}");
    let rows = pool.query_all(&data_sql, params).await?;
    Ok((rows.iter().map(usage_log_row).collect::<Result<Vec<_>>>()?, total))
}

pub async fn get_system_settings(pool: &DbPool) -> Result<SystemSettings> {
    settings_row(&pool.query_one("SELECT max_concurrency, global_rpm, test_model, test_concurrency, proxy_url, admin_secret, auto_clean_unauthorized, auto_clean_rate_limited, auto_clean_full_usage, auto_clean_error, auto_clean_expired, fast_scheduler_enabled, max_retries, pg_max_conns, proxy_pool_enabled FROM system_settings WHERE id = 1", vec![]).await?)
}

pub async fn update_system_settings(pool: &DbPool, s: &SystemSettings) -> Result<()> {
    pool.execute_write("UPDATE system_settings SET max_concurrency=?1, global_rpm=?2, test_model=?3, test_concurrency=?4, proxy_url=?5, admin_secret=?6, auto_clean_unauthorized=?7, auto_clean_rate_limited=?8, auto_clean_full_usage=?9, auto_clean_error=?10, auto_clean_expired=?11, fast_scheduler_enabled=?12, max_retries=?13, pg_max_conns=?14, proxy_pool_enabled=?15 WHERE id = 1",
        vec![v_i32(s.max_concurrency), v_i32(s.global_rpm), v_str(&s.test_model), v_i32(s.test_concurrency), v_str(&s.proxy_url), v_str(&s.admin_secret), v_bool(s.auto_clean_unauthorized), v_bool(s.auto_clean_rate_limited), v_bool(s.auto_clean_full_usage), v_bool(s.auto_clean_error), v_bool(s.auto_clean_expired), v_bool(s.fast_scheduler_enabled), v_i32(s.max_retries), v_i32(s.pg_max_conns), v_bool(s.proxy_pool_enabled)]).await?;
    Ok(())
}

pub async fn count_today_requests(pool: &DbPool) -> Result<i64> {
    pool.query_one("SELECT COUNT(*) AS count FROM usage_logs WHERE created_at >= date('now') AND status_code != 499", vec![]).await?.get_i64("count")
}

pub async fn get_usage_stats_full(pool: &DbPool) -> Result<UsageStatsFull> {
    let row = pool.query_one("SELECT COUNT(*) AS total_requests, COALESCE(SUM(total_tokens), 0) AS total_tokens, COALESCE(SUM(prompt_tokens), 0) AS total_prompt_tokens, COALESCE(SUM(completion_tokens), 0) AS total_completion_tokens, COALESCE(SUM(cached_tokens), 0) AS total_cached_tokens, COALESCE(AVG(duration_ms), 0) AS avg_duration_ms FROM usage_logs WHERE status_code != 499", vec![]).await?;
    let today = pool.query_one("SELECT COUNT(*) AS today_requests, COALESCE(SUM(total_tokens), 0) AS today_tokens FROM usage_logs WHERE created_at >= date('now') AND status_code != 499", vec![]).await?;
    let minute = pool.query_one("SELECT COUNT(*) AS rpm, COALESCE(SUM(total_tokens), 0) AS tpm FROM usage_logs WHERE created_at >= datetime('now', '-1 minute') AND status_code != 499", vec![]).await?;
    let error_count = pool.query_one("SELECT COUNT(*) AS count FROM usage_logs WHERE created_at >= date('now') AND status_code >= 400 AND status_code != 499", vec![]).await?.get_i64("count")?;
    let today_requests = today.get_i64("today_requests")?;
    let error_rate = if today_requests > 0 { error_count as f64 / today_requests as f64 * 100.0 } else { 0.0 };
    Ok(UsageStatsFull { total_requests: row.get_i64("total_requests")?, total_tokens: row.get_i64("total_tokens")?, total_prompt_tokens: row.get_i64("total_prompt_tokens")?, total_completion_tokens: row.get_i64("total_completion_tokens")?, total_cached_tokens: row.get_i64("total_cached_tokens")?, today_requests, today_tokens: today.get_i64("today_tokens")?, rpm: minute.get_i64("rpm")?, tpm: minute.get_i64("tpm")?, avg_duration_ms: row.get_f64("avg_duration_ms")?, error_rate })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageStatsFull { pub total_requests: i64, pub total_tokens: i64, pub total_prompt_tokens: i64, pub total_completion_tokens: i64, pub total_cached_tokens: i64, pub today_requests: i64, pub today_tokens: i64, pub rpm: i64, pub tpm: i64, pub avg_duration_ms: f64, pub error_rate: f64 }

pub async fn get_account_usage(pool: &DbPool, account_id: i64) -> Result<AccountUsageDetail> {
    let row = pool.query_one("SELECT COUNT(*) AS total_requests, COALESCE(SUM(total_tokens),0) AS total_tokens, COALESCE(SUM(input_tokens),0) AS input_tokens, COALESCE(SUM(output_tokens),0) AS output_tokens, COALESCE(SUM(reasoning_tokens),0) AS reasoning_tokens, COALESCE(SUM(cached_tokens),0) AS cached_tokens FROM usage_logs WHERE account_id = ?1 AND status_code != 499", vec![v_i64(account_id)]).await?;
    let model_rows = pool.query_all("SELECT model, COUNT(*) AS requests, COALESCE(SUM(total_tokens),0) AS tokens FROM usage_logs WHERE account_id = ?1 AND status_code != 499 AND model != '' GROUP BY model ORDER BY requests DESC LIMIT 10", vec![v_i64(account_id)]).await?;
    let models = model_rows.iter().map(|r| Ok(AccountModelStat { model: r.get_string("model")?, requests: r.get_i64("requests")?, tokens: r.get_i64("tokens")? })).collect::<Result<Vec<_>>>()?;
    Ok(AccountUsageDetail { total_requests: row.get_i64("total_requests")?, total_tokens: row.get_i64("total_tokens")?, input_tokens: row.get_i64("input_tokens")?, output_tokens: row.get_i64("output_tokens")?, reasoning_tokens: row.get_i64("reasoning_tokens")?, cached_tokens: row.get_i64("cached_tokens")?, models })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountUsageDetail { pub total_requests: i64, pub total_tokens: i64, pub input_tokens: i64, pub output_tokens: i64, pub reasoning_tokens: i64, pub cached_tokens: i64, pub models: Vec<AccountModelStat> }
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountModelStat { pub model: String, pub requests: i64, pub tokens: i64 }

pub async fn list_api_keys(pool: &DbPool) -> Result<Vec<ApiKey>> {
    let rows = pool.query_all("SELECT id, name, key, created_at FROM api_keys ORDER BY id", vec![]).await?;
    rows.iter().map(|r| Ok(ApiKey { id: r.get_i64("id")?, name: r.get_string("name")?, key: r.get_string("key")?, created_at: r.get_string("created_at")? })).collect()
}

pub async fn insert_api_key(pool: &DbPool, name: &str, key: &str) -> Result<i64> {
    pool.query_one_write("INSERT INTO api_keys (name, key) VALUES (?1, ?2) RETURNING id", vec![v_str(name), v_str(key)]).await?.get_i64("id")
}

pub async fn delete_api_key(pool: &DbPool, id: i64) -> Result<()> { pool.execute_write("DELETE FROM api_keys WHERE id = ?1", vec![v_i64(id)]).await?; Ok(()) }

pub async fn clear_usage_logs(pool: &DbPool) -> Result<()> {
    pool.transaction_write(|conn| Box::pin(async move {
        conn.execute("UPDATE usage_stats_baseline SET total_requests = total_requests + (SELECT COUNT(*) FROM usage_logs WHERE status_code != 499), total_tokens = total_tokens + COALESCE((SELECT SUM(total_tokens) FROM usage_logs WHERE status_code != 499), 0), prompt_tokens = prompt_tokens + COALESCE((SELECT SUM(prompt_tokens) FROM usage_logs WHERE status_code != 499), 0), completion_tokens = completion_tokens + COALESCE((SELECT SUM(completion_tokens) FROM usage_logs WHERE status_code != 499), 0), cached_tokens = cached_tokens + COALESCE((SELECT SUM(cached_tokens) FROM usage_logs WHERE status_code != 499), 0) WHERE id = 1", ()).await?;
        conn.execute("DELETE FROM usage_logs", ()).await?;
        Ok(())
    })).await
}

pub async fn update_account_resets_at(pool: &DbPool, id: i64, resets_at: i64) -> Result<()> {
    let ts_str = if resets_at > 0 { resets_at.to_string() } else { String::new() };
    pool.execute_write("UPDATE accounts SET credentials = json_patch(credentials, json_object('codex_7d_reset_at', ?1)), updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE id = ?2", vec![v_string(ts_str), v_i64(id)]).await?; Ok(())
}

#[allow(dead_code)]
pub async fn update_account_resets_5h_at(pool: &DbPool, id: i64, resets_5h_at: i64) -> Result<()> {
    let ts_str = if resets_5h_at > 0 { resets_5h_at.to_string() } else { String::new() };
    pool.execute_write("UPDATE accounts SET credentials = json_patch(credentials, json_object('codex_5h_reset_at', ?1)), updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE id = ?2", vec![v_string(ts_str), v_i64(id)]).await?; Ok(())
}

pub async fn persist_account_usage(pool: &DbPool, id: i64, usage_7d: f64, usage_5h: f64) -> Result<()> {
    pool.execute_write("UPDATE accounts SET credentials = json_patch(credentials, json_object('codex_7d_used_percent', ?1, 'codex_5h_used_percent', ?2)), updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE id = ?3", vec![v_f64(usage_7d), v_f64(usage_5h), v_i64(id)]).await?; Ok(())
}

pub async fn clear_account_usage_state(pool: &DbPool, id: i64) -> Result<()> {
    pool.execute_write("UPDATE accounts SET credentials = json_patch(credentials, '{\"codex_7d_used_percent\":0,\"codex_5h_used_percent\":0,\"codex_7d_reset_at\":\"\",\"codex_5h_reset_at\":\"\"}'), cooldown_until = NULL, cooldown_reason = '', updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE id = ?1", vec![v_i64(id)]).await?; Ok(())
}

pub async fn update_account_cooldown(pool: &DbPool, id: i64, until_ts: i64, reason: &str) -> Result<()> {
    pool.execute_write("UPDATE accounts SET cooldown_until = ?1, cooldown_reason = ?2, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE id = ?3", vec![v_i64(until_ts), v_str(reason), v_i64(id)]).await?; Ok(())
}

pub async fn clear_account_cooldown(pool: &DbPool, id: i64) -> Result<()> {
    pool.execute_write("UPDATE accounts SET cooldown_until = NULL, cooldown_reason = '', updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE id = ?1", vec![v_i64(id)]).await?; Ok(())
}

pub async fn insert_account_event(pool: &DbPool, account_id: i64, event_type: &str, source: &str) {
    let _ = pool.execute_write("INSERT INTO account_events (account_id, event_type, source) VALUES (?1, ?2, ?3)", vec![v_i64(account_id), v_str(event_type), v_str(source)]).await;
}

pub async fn get_account_event_trend(pool: &DbPool, start: &str, end: &str, bucket_minutes: i64) -> Result<Vec<AccountEventPoint>> {
    let bucket_secs = bucket_minutes * 60;
    let rows = pool.query_all("SELECT strftime('%Y-%m-%dT%H:%M:%S', datetime((CAST(strftime('%s', created_at) AS INTEGER) / ?1) * ?1, 'unixepoch', '+8 hours')) AS bucket, COALESCE(SUM(CASE WHEN event_type = 'added' THEN 1 ELSE 0 END), 0) AS added, COALESCE(SUM(CASE WHEN event_type = 'deleted' THEN 1 ELSE 0 END), 0) AS deleted FROM account_events WHERE created_at >= ?2 AND created_at <= ?3 GROUP BY 1 ORDER BY 1", vec![v_i64(bucket_secs), v_str(start), v_str(end)]).await?;
    rows.iter().map(|r| Ok(AccountEventPoint { bucket: r.get_string("bucket")?, added: r.get_i64("added")?, deleted: r.get_i64("deleted")? })).collect()
}

#[derive(Debug, serde::Serialize)]
pub struct AccountEventPoint { pub bucket: String, pub added: i64, pub deleted: i64 }

// ─── 代理池操作 ───

pub async fn list_proxies(pool: &DbPool) -> Result<Vec<ProxyRow>> {
    let rows = pool.query_all("SELECT id, url, label, enabled, created_at, test_ip, test_location, test_latency_ms FROM proxies ORDER BY id DESC", vec![]).await?;
    rows.iter().map(|r| Ok(ProxyRow {
        id: r.get_i64("id")?,
        url: r.get_string("url")?,
        label: r.get_string("label")?,
        enabled: r.get_bool("enabled")?,
        created_at: r.get_string("created_at")?,
        test_ip: r.get_string("test_ip")?,
        test_location: r.get_string("test_location")?,
        test_latency_ms: r.get_i64("test_latency_ms")?,
    })).collect()
}

pub async fn list_enabled_proxy_urls(pool: &DbPool) -> Result<Vec<String>> {
    let rows = pool.query_all("SELECT url FROM proxies WHERE enabled = 1 ORDER BY id DESC", vec![]).await?;
    rows.iter().map(|r| r.get_string("url")).collect()
}

pub async fn insert_proxy(pool: &DbPool, url: &str, label: &str) -> Result<i64> {
    let row = pool.query_one_write("INSERT INTO proxies (url, label, enabled) VALUES (?1, ?2, 1) RETURNING id", vec![v_str(url), v_str(label)]).await?;
    row.get_i64("id")
}

pub async fn delete_proxy(pool: &DbPool, id: i64) -> Result<()> {
    pool.execute_write("DELETE FROM proxies WHERE id = ?1", vec![v_i64(id)]).await?;
    Ok(())
}

pub async fn update_proxy(pool: &DbPool, id: i64, label: Option<&str>, enabled: Option<bool>) -> Result<()> {
    if let Some(lbl) = label {
        pool.execute_write("UPDATE proxies SET label = ?1 WHERE id = ?2", vec![v_str(lbl), v_i64(id)]).await?;
    }
    if let Some(en) = enabled {
        pool.execute_write("UPDATE proxies SET enabled = ?1 WHERE id = ?2", vec![v_bool(en), v_i64(id)]).await?;
    }
    Ok(())
}

pub async fn update_proxy_test_result(pool: &DbPool, id: i64, ip: &str, location: &str, latency_ms: i64) -> Result<()> {
    pool.execute_write("UPDATE proxies SET test_ip = ?1, test_location = ?2, test_latency_ms = ?3 WHERE id = ?4", vec![v_str(ip), v_str(location), v_i64(latency_ms), v_i64(id)]).await?;
    Ok(())
}

pub async fn batch_delete_proxies(pool: &DbPool, ids: &[i64]) -> Result<()> {
    if ids.is_empty() {
        return Ok(());
    }
    let placeholders: Vec<String> = (1..=ids.len()).map(|i| format!("?{}", i)).collect();
    let sql = format!("DELETE FROM proxies WHERE id IN ({})", placeholders.join(","));
    let params = ids.iter().map(|&id| v_i64(id)).collect();
    pool.execute_write(&sql, params).await?;
    Ok(())
}
