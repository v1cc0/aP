use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

/// 应用配置 — 从 config.toml 加载，不可变
#[derive(Debug, Clone)]
pub struct AppConfig {
    /// 服务端口
    pub port: u16,
    /// Turso/SQLite 本地数据库文件
    pub database_url: String,
    /// 逻辑连接上限（Turso 按需打开连接，用于 UI/限流配置兼容）
    pub db_pool_size: u32,
    /// 启用实验性 BEGIN CONCURRENT / MVCC
    pub db_begin_concurrent: bool,
    /// 启用实验性 multi-process WAL coordination
    pub db_multiprocess_wal: bool,
    /// 管理后台密钥（可选）
    pub admin_secret: Option<String>,
    /// 全局默认代理 URL（可选）
    pub proxy_url: Option<String>,
    /// 显式允许 /v1/* 在未配置 API Key 时无鉴权放行（默认禁止，fail-closed）
    pub allow_anonymous_v1: bool,

    // 设备指纹配置
    pub device_user_agent: Option<String>,
    pub device_package_version: Option<String>,
    pub device_runtime_version: Option<String>,
    pub device_os: Option<String>,
    pub device_arch: Option<String>,
    pub stabilize_device_profile: bool,
}

#[derive(Debug, Default, Deserialize)]
struct FileConfig {
    app: Option<AppSection>,
    database: Option<DatabaseSection>,
    admin: Option<AdminSection>,
    proxy: Option<ProxySection>,
    device: Option<DeviceSection>,
}

#[derive(Debug, Default, Deserialize)]
struct AppSection {
    port: Option<u16>,
    allow_anonymous_v1: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
struct DatabaseSection {
    path: Option<String>,
    pool_size: Option<u32>,
    begin_concurrent: Option<bool>,
    multiprocess_wal: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
struct AdminSection { secret: Option<String> }
#[derive(Debug, Default, Deserialize)]
struct ProxySection { url: Option<String> }

#[derive(Debug, Default, Deserialize)]
struct DeviceSection {
    user_agent: Option<String>,
    package_version: Option<String>,
    runtime_version: Option<String>,
    os: Option<String>,
    arch: Option<String>,
    stabilize_profile: Option<bool>,
}

impl AppConfig {
    /// 从 config.toml 加载配置；可用 CODEX_CONFIG 指定路径。
    pub fn from_file() -> Self {
        let path = if let Ok(env_path) = std::env::var("CODEX_CONFIG") {
            PathBuf::from(env_path)
        } else {
            let mut ap_config = None;
            if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
                let p = home.join(".ap").join("config.toml");
                if p.exists() {
                    ap_config = Some(p);
                }
            }
            ap_config.unwrap_or_else(|| {
                let local = PathBuf::from("config.toml");
                if local.exists() {
                    local
                } else if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
                    home.join(".ap").join("config.toml")
                } else {
                    local
                }
            })
        };
        Self::from_path(path)
    }

    pub fn from_path(path: impl AsRef<Path>) -> Self {
        let path = path.as_ref();
        let cfg: FileConfig = fs::read_to_string(path)
            .ok()
            .and_then(|s| toml::from_str(&s).ok())
            .unwrap_or_default();

        let app = cfg.app.unwrap_or_default();
        let database = cfg.database.unwrap_or_default();
        let admin = cfg.admin.unwrap_or_default();
        let proxy = cfg.proxy.unwrap_or_default();
        let device = cfg.device.unwrap_or_default();

        let database_path = database.path.unwrap_or_else(|| {
            if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
                home.join(".ap").join("ap.db").to_string_lossy().to_string()
            } else {
                "data/ap.db".to_string()
            }
        });
        let database_url = normalize_db_path(path.parent(), &database_path);

        Self {
            port: app.port.unwrap_or(8080),
            database_url,
            db_pool_size: database.pool_size.unwrap_or(20),
            db_begin_concurrent: database.begin_concurrent.unwrap_or(true),
            db_multiprocess_wal: database.multiprocess_wal.unwrap_or(true),
            admin_secret: admin.secret.filter(|s| !s.trim().is_empty()),
            proxy_url: proxy.url.filter(|s| !s.trim().is_empty()),
            allow_anonymous_v1: app.allow_anonymous_v1.unwrap_or(false),
            device_user_agent: device.user_agent.filter(|s| !s.trim().is_empty()),
            device_package_version: device.package_version.filter(|s| !s.trim().is_empty()),
            device_runtime_version: device.runtime_version.filter(|s| !s.trim().is_empty()),
            device_os: device.os.filter(|s| !s.trim().is_empty()),
            device_arch: device.arch.filter(|s| !s.trim().is_empty()),
            stabilize_device_profile: device.stabilize_profile.unwrap_or(false),
        }
    }
}

fn normalize_db_path(config_dir: Option<&Path>, value: &str) -> String {
    let value = value.strip_prefix("sqlite://").unwrap_or(value);
    if value == ":memory:" {
        return value.to_string();
    }
    let p = PathBuf::from(value);
    if p.is_absolute() {
        p.to_string_lossy().to_string()
    } else {
        config_dir.unwrap_or_else(|| Path::new(".")).join(p).to_string_lossy().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_db_path() {
        assert_eq!(normalize_db_path(None, ":memory:"), ":memory:");
        assert_eq!(normalize_db_path(Some(Path::new("/etc")), "/var/lib/ap.db"), "/var/lib/ap.db");
        assert_eq!(normalize_db_path(Some(Path::new("/home/user/.ap")), "ap.db"), "/home/user/.ap/ap.db");
        assert_eq!(normalize_db_path(None, "ap.db"), "./ap.db");
    }

    #[test]
    fn test_from_file_with_env() {
        unsafe {
            std::env::set_var("CODEX_CONFIG", "non_existent_config_file_xyz.toml");
        }
        let cfg = AppConfig::from_file();
        // 因为文件不存在，所以应该加载默认配置
        assert_eq!(cfg.port, 8080);
        unsafe {
            std::env::remove_var("CODEX_CONFIG");
        }
    }
}
