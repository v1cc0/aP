use serde::{Deserialize, Serialize};

// ─── 账号 ───

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountRow {
    pub id: i64,
    pub name: String,
    pub platform: String,
    pub account_type: String,
    pub credentials: String,
    pub proxy_url: String,
    pub status: String,
    pub error_message: String,
    pub cooldown_reason: String,
    pub cooldown_until: Option<String>,
    pub enable_scheduling: bool,
    pub created_at: String,
    pub updated_at: String,
}

/// 存储在 credentials JSONB 中的字段
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Credentials {
    #[serde(default)]
    pub refresh_token: String,
    #[serde(default)]
    pub access_token: String,
    #[serde(default)]
    pub id_token: String,
    #[serde(default)]
    pub expires_at: String,
    #[serde(default)]
    pub email: String,
    #[serde(default)]
    pub account_id: String,
    #[serde(default)]
    pub plan_type: String,
    #[serde(default, skip_serializing_if = "is_zero_f64")]
    pub codex_7d_used_percent: f64,
    #[serde(default, skip_serializing_if = "is_zero_f64")]
    pub codex_5h_used_percent: f64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub codex_7d_reset_at: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub codex_5h_reset_at: String,
}

fn is_zero_f64(v: &f64) -> bool { *v == 0.0 }

// ─── 使用日志 ───

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageLog {
    pub id: i64,
    pub account_id: i64,
    pub endpoint: String,
    pub model: String,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub total_tokens: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub reasoning_tokens: i64,
    pub cached_tokens: i64,
    pub first_token_ms: i64,
    pub reasoning_effort: String,
    pub status_code: i64,
    pub duration_ms: i64,
    pub stream: bool,
    pub service_tier: String,
    pub account_email: String,
    pub cost: f64,
    pub tt_request_id: String,
    pub tt_user_id: String,
    pub tt_api_key_id: String,
    pub tt_group_id: String,
    pub tt_provider_account_id: String,
    pub tt_provider_platform: String,
    pub created_at: String,
}

/// 从数据库查出的日志行
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageLogRow {
    pub id: i64,
    pub account_id: i64,
    pub endpoint: String,
    pub model: String,
    pub prompt_tokens: i32,
    pub completion_tokens: i32,
    pub total_tokens: i32,
    pub input_tokens: i32,
    pub output_tokens: i32,
    pub reasoning_tokens: i32,
    pub cached_tokens: i32,
    pub first_token_ms: i32,
    pub reasoning_effort: String,
    pub status_code: i32,
    pub duration_ms: i32,
    pub stream: bool,
    pub service_tier: String,
    pub account_email: String,
    pub tt_request_id: String,
    pub tt_user_id: String,
    pub tt_api_key_id: String,
    pub tt_group_id: String,
    pub tt_provider_account_id: String,
    pub tt_provider_platform: String,
    pub created_at: String,
}

// ─── API Key ───

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKey {
    pub id: i64,
    pub name: String,
    pub key: String,
    pub created_at: String,
}

// ─── 系统设置 ───

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemSettings {
    pub max_concurrency: i32,
    pub global_rpm: i32,
    pub test_model: String,
    pub test_concurrency: i32,
    pub proxy_url: String,
    pub admin_secret: String,
    pub auto_clean_unauthorized: bool,
    pub auto_clean_rate_limited: bool,
    pub auto_clean_full_usage: bool,
    pub auto_clean_error: bool,
    pub auto_clean_expired: bool,
    pub fast_scheduler_enabled: bool,
    pub max_retries: i32,
    #[serde(default = "default_pg_max_conns")]
    pub pg_max_conns: i32,
    #[serde(default)]
    pub proxy_pool_enabled: bool,
}

fn default_pg_max_conns() -> i32 { 256 }

// ─── 代理池模型 ───

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyRow {
    pub id: i64,
    pub url: String,
    pub label: String,
    pub enabled: bool,
    pub created_at: String,
    pub test_ip: String,
    pub test_location: String,
    pub test_latency_ms: i64,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyTestResult {
    pub success: bool,
    pub ip: Option<String>,
    pub country: Option<String>,
    pub region: Option<String>,
    pub city: Option<String>,
    pub isp: Option<String>,
    pub latency_ms: Option<i64>,
    pub location: Option<String>,
    pub error: Option<String>,
}


// ─── 图表聚合数据 ───

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChartBucket {
    pub bucket: String,
    pub requests: i64,
    pub avg_latency: f64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub reasoning_tokens: i64,
    pub cached_tokens: i64,
    pub errors_401: i64,
    pub success_200: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelRanking {
    pub model: String,
    pub requests: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChartData {
    pub timeline: Vec<ChartBucket>,
    pub models: Vec<ModelRanking>,
}
