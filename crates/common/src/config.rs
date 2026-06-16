//! Configuration loader (ตัวโหลดค่าตั้งค่า).
//!
//! Loads [`Settings`] from environment variables (prefix `KOKKAK_`,
//! separator `__`) and validates them at startup. Process exits
//! fast on misconfiguration (fail-fast principle).
//!
//! ## Environment variable convention
//!
//! ```text
//! KOKKAK_<SECTION>__<KEY>=value
//! ```
//!
//! Examples:
//! - `KOKKAK_SERVER__ADDR=0.0.0.0:3000`
//! - `KOKKAK_SERVER__WORKERS=4`
//! - `KOKKAK_LOG__FORMAT=json`  (or `pretty`)
//! - `KOKKAK_DATABASE__SQLSERVER_URL=sqlserver://...`
//! - `KOKKAK_REDIS__URL=redis://host:6379`
//! - `KOKKAK_NATS__URL=nats://host:4222`
//! - `KOKKAK_MONGO__URL=mongodb://host:27017`
//! - `KOKKAK_MONGO__DATABASE=kokkak`
//! - `KOKKAK_DATA_DIR__PATH=./data/json_db`
//! - `KOKKAK_AUTH__JWT_SECRET=...`

use figment::providers::{Env, Format, Toml};
use figment::Figment;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Errors when loading or validating configuration.
#[derive(Debug, Error)]
pub enum ConfigError {
    /// Underlying figment provider error (missing/invalid env, parse error, etc.).
    #[error("config provider error: {0}")]
    Figment(#[from] Box<figment::Error>),

    /// Semantically invalid value: a specific setting failed a post-load check.
    #[error("invalid config: key={key}, {message}")]
    Invalid { key: String, message: String },
}

impl From<figment::Error> for ConfigError {
    fn from(err: figment::Error) -> Self {
        Self::Figment(Box::new(err))
    }
}

/// Top-level settings struct (โครงสร้างตั้งค่าระดับบนสุด).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Settings {
    /// HTTP server settings (การตั้งค่า HTTP server).
    #[serde(default)]
    pub server: ServerSettings,

    /// Logging settings (การตั้งค่า log).
    #[serde(default)]
    pub log: LogSettings,

    /// SQL Server database settings (T06).
    /// Empty by default; production MUST set `KOKKAK_DATABASE__SQLSERVER_URL`.
    #[serde(default)]
    pub database: DatabaseSettings,

    /// Redis cache + pub/sub settings (T07, T07A).
    #[serde(default)]
    pub redis: RedisSettings,

    /// NATS JetStream queue settings (T08).
    #[serde(default)]
    pub nats: NatsSettings,

    /// MongoDB settings (T09).
    #[serde(default)]
    pub mongo: MongoSettings,

    /// JSON-DB simulation directory (M1.5 / M2 / M3).
    #[serde(default)]
    pub data_dir: DataDirSettings,

    /// Auth / JWT settings (M2).
    #[serde(default)]
    pub auth: AuthSettings,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            server: ServerSettings::default(),
            log: LogSettings::default(),
            database: DatabaseSettings::default(),
            redis: RedisSettings::default(),
            nats: NatsSettings::default(),
            mongo: MongoSettings::default(),
            data_dir: DataDirSettings::default(),
            auth: AuthSettings::default(),
        }
    }
}

/// HTTP server settings.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServerSettings {
    /// Bind address (e.g. `0.0.0.0:3000`).
    #[serde(default = "default_addr")]
    pub addr: String,

    /// Number of Tokio worker threads (currently informational — `axum::serve`
    /// runs on a single thread; multi-worker requires a process manager).
    #[serde(default = "default_workers")]
    pub workers: usize,
}

impl Default for ServerSettings {
    fn default() -> Self {
        Self {
            addr: default_addr(),
            workers: default_workers(),
        }
    }
}

/// Logging settings.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LogSettings {
    /// Output format for the structured logger.
    #[serde(default = "default_log_format")]
    pub format: LogFormat,
}

impl Default for LogSettings {
    fn default() -> Self {
        Self {
            format: default_log_format(),
        }
    }
}

/// SQL Server database settings (T06).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DatabaseSettings {
    /// Tiberius connection URL.
    #[serde(default)]
    pub sqlserver_url: String,

    /// bb8 connection-pool max size (per instance). See AGENTS.md § 7.2.
    #[serde(default = "default_db_pool_size")]
    pub pool_size: u32,

    /// Connection-acquisition timeout in seconds.
    #[serde(default = "default_db_connect_timeout_secs")]
    pub connect_timeout_secs: u64,

    /// Path to SQL migration files (T09). Read by the migration runner.
    #[serde(default = "default_migrations_dir")]
    pub migrations_dir: String,
}

impl DatabaseSettings {
    /// `true` when a real SQL Server URL has been configured.
    pub fn is_configured(&self) -> bool {
        !self.sqlserver_url.trim().is_empty() && self.sqlserver_url != "disabled"
    }
}

impl Default for DatabaseSettings {
    fn default() -> Self {
        Self {
            sqlserver_url: String::new(),
            pool_size: default_db_pool_size(),
            connect_timeout_secs: default_db_connect_timeout_secs(),
            migrations_dir: default_migrations_dir(),
        }
    }
}

/// Redis cache + pub/sub settings (T07, T07A).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RedisSettings {
    /// Redis connection URL.
    #[serde(default = "default_redis_url")]
    pub url: String,

    /// deadpool-redis max pool size.
    #[serde(default = "default_redis_pool_size")]
    pub pool_size: usize,
}

impl RedisSettings {
    pub fn is_configured(&self) -> bool {
        !self.url.trim().is_empty() && self.url != "redis://disabled"
    }
}

impl Default for RedisSettings {
    fn default() -> Self {
        Self {
            url: default_redis_url(),
            pool_size: default_redis_pool_size(),
        }
    }
}

/// NATS JetStream settings (T08).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NatsSettings {
    /// NATS connection URL.
    #[serde(default = "default_nats_url")]
    pub url: String,

    /// Prefix prepended to every stream / subject name.
    #[serde(default = "default_nats_prefix")]
    pub stream_prefix: String,
}

impl NatsSettings {
    pub fn is_configured(&self) -> bool {
        !self.url.trim().is_empty() && self.url != "nats://disabled"
    }
}

impl Default for NatsSettings {
    fn default() -> Self {
        Self {
            url: default_nats_url(),
            stream_prefix: default_nats_prefix(),
        }
    }
}

/// MongoDB settings (T09).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MongoSettings {
    #[serde(default = "default_mongo_url")]
    pub url: String,

    #[serde(default = "default_mongo_db")]
    pub database: String,
}

impl MongoSettings {
    pub fn is_configured(&self) -> bool {
        !self.url.trim().is_empty() && self.url != "mongodb://disabled"
    }
}

impl Default for MongoSettings {
    fn default() -> Self {
        Self {
            url: default_mongo_url(),
            database: default_mongo_db(),
        }
    }
}

/// JSON-DB simulation directory (M1.5 / M2 / M3).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DataDirSettings {
    /// Directory where JSON-DB stores its files. Created if missing.
    #[serde(default = "default_data_dir_path")]
    pub path: String,
    /// Delete the data dir on startup (dev convenience).
    #[serde(default)]
    pub reset_on_startup: bool,
}

impl Default for DataDirSettings {
    fn default() -> Self {
        Self {
            path: default_data_dir_path(),
            reset_on_startup: false,
        }
    }
}

/// Auth / JWT settings (M2).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuthSettings {
    /// HS256 secret. Required in prod; empty = dev / JSON-DB mode.
    #[serde(default)]
    pub jwt_secret: String,
    /// Issuer claim (`iss`).
    #[serde(default = "default_auth_issuer")]
    pub issuer: String,
    /// Access-token TTL in seconds.
    #[serde(default = "default_access_ttl")]
    pub access_ttl_secs: i64,
    /// Refresh-token TTL in seconds.
    #[serde(default = "default_refresh_ttl")]
    pub refresh_ttl_secs: i64,
}

impl Default for AuthSettings {
    fn default() -> Self {
        Self {
            jwt_secret: String::new(),
            issuer: default_auth_issuer(),
            access_ttl_secs: default_access_ttl(),
            refresh_ttl_secs: default_refresh_ttl(),
        }
    }
}

impl AuthSettings {
    /// `true` iff a non-empty JWT secret is configured.
    pub fn is_configured(&self) -> bool {
        !self.jwt_secret.is_empty()
    }
}

/// Output format for the structured logger.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LogFormat {
    /// Newline-delimited JSON (for log aggregators).
    Json,
    /// Human-readable pretty format (for local dev).
    Pretty,
}

fn default_addr() -> String {
    "0.0.0.0:3000".into()
}
fn default_workers() -> usize {
    4
}
fn default_log_format() -> LogFormat {
    LogFormat::Pretty
}
fn default_db_pool_size() -> u32 {
    20
}
fn default_db_connect_timeout_secs() -> u64 {
    5
}
fn default_migrations_dir() -> String {
    "migrations".into()
}
fn default_redis_url() -> String {
    "redis://disabled".into()
}
fn default_redis_pool_size() -> usize {
    16
}
fn default_nats_url() -> String {
    "nats://disabled".into()
}
fn default_nats_prefix() -> String {
    "kokkak".into()
}
fn default_mongo_url() -> String {
    "mongodb://disabled".into()
}
fn default_mongo_db() -> String {
    "kokkak".into()
}
fn default_data_dir_path() -> String {
    "./data/json_db".into()
}
fn default_auth_issuer() -> String {
    "kokkak-api".into()
}
fn default_access_ttl() -> i64 {
    900
}
fn default_refresh_ttl() -> i64 {
    2_592_000
}

impl Settings {
    /// Load from environment variables. Fails fast on errors.
    pub fn load() -> Result<Self, ConfigError> {
        let figment = Figment::new()
            .merge(Toml::file("config.toml").nested())
            .merge(Env::prefixed("KOKKAK_").split("__"));
        let settings: Settings = figment.extract()?;
        settings.validate()?;
        Ok(settings)
    }

    /// Validate the loaded settings.
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.server.addr.trim().is_empty() {
            return Err(ConfigError::Invalid {
                key: "KOKKAK_SERVER__ADDR".into(),
                message: "must not be empty".into(),
            });
        }
        if self.server.workers == 0 {
            return Err(ConfigError::Invalid {
                key: "KOKKAK_SERVER__WORKERS".into(),
                message: "must be >= 1".into(),
            });
        }
        if self.database.pool_size == 0 {
            return Err(ConfigError::Invalid {
                key: "KOKKAK_DATABASE__POOL_SIZE".into(),
                message: "must be >= 1".into(),
            });
        }
        if self.database.connect_timeout_secs == 0 {
            return Err(ConfigError::Invalid {
                key: "KOKKAK_DATABASE__CONNECT_TIMEOUT_SECS".into(),
                message: "must be >= 1".into(),
            });
        }
        if self.redis.pool_size == 0 {
            return Err(ConfigError::Invalid {
                key: "KOKKAK_REDIS__POOL_SIZE".into(),
                message: "must be >= 1".into(),
            });
        }
        if self.nats.stream_prefix.trim().is_empty() {
            return Err(ConfigError::Invalid {
                key: "KOKKAK_NATS__STREAM_PREFIX".into(),
                message: "must not be empty".into(),
            });
        }
        if self.mongo.database.trim().is_empty() {
            return Err(ConfigError::Invalid {
                key: "KOKKAK_MONGO__DATABASE".into(),
                message: "must not be empty".into(),
            });
        }
        if self.data_dir.path.trim().is_empty() {
            return Err(ConfigError::Invalid {
                key: "KOKKAK_DATA_DIR__PATH".into(),
                message: "must not be empty".into(),
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Tests that touch env vars must hold this lock.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn clear_kokkak_env() {
        for key in [
            "KOKKAK_SERVER__ADDR",
            "KOKKAK_SERVER__WORKERS",
            "KOKKAK_LOG__FORMAT",
            "KOKKAK_DATABASE__SQLSERVER_URL",
            "KOKKAK_DATABASE__POOL_SIZE",
            "KOKKAK_DATABASE__CONNECT_TIMEOUT_SECS",
            "KOKKAK_DATABASE__MIGRATIONS_DIR",
            "KOKKAK_REDIS__URL",
            "KOKKAK_REDIS__POOL_SIZE",
            "KOKKAK_NATS__URL",
            "KOKKAK_NATS__STREAM_PREFIX",
            "KOKKAK_MONGO__URL",
            "KOKKAK_MONGO__DATABASE",
            "KOKKAK_DATA_DIR__PATH",
            "KOKKAK_AUTH__JWT_SECRET",
            "KOKKAK_AUTH__ISSUER",
            "KOKKAK_AUTH__ACCESS_TTL_SECS",
            "KOKKAK_AUTH__REFRESH_TTL_SECS",
        ] {
            std::env::remove_var(key);
        }
    }

    #[test]
    fn default_settings_validate() {
        let s = Settings::default();
        assert_eq!(s.server.addr, "0.0.0.0:3000");
        assert_eq!(s.server.workers, 4);
        assert_eq!(s.log.format, LogFormat::Pretty);
        assert!(!s.database.is_configured());
        assert!(!s.redis.is_configured());
        assert!(!s.nats.is_configured());
        assert!(!s.mongo.is_configured());
        assert!(s.validate().is_ok());
    }

    #[test]
    fn load_with_no_env_uses_defaults() {
        let _guard = ENV_LOCK.lock().expect("mutex poisoned");
        clear_kokkak_env();

        let s = Settings::load().expect("load should succeed with defaults");
        assert_eq!(s.server.addr, "0.0.0.0:3000");
        assert_eq!(s.server.workers, 4);
        assert_eq!(s.log.format, LogFormat::Pretty);
        assert_eq!(s.database.pool_size, 20);
        assert_eq!(s.redis.pool_size, 16);
        assert_eq!(s.nats.stream_prefix, "kokkak");
        assert_eq!(s.mongo.database, "kokkak");
    }

    #[test]
    fn load_from_env_overrides() {
        let _guard = ENV_LOCK.lock().expect("mutex poisoned");
        clear_kokkak_env();

        std::env::set_var("KOKKAK_SERVER__ADDR", "127.0.0.1:9999");
        std::env::set_var("KOKKAK_SERVER__WORKERS", "8");
        std::env::set_var("KOKKAK_LOG__FORMAT", "json");
        std::env::set_var(
            "KOKKAK_DATABASE__SQLSERVER_URL",
            "sqlserver://sa:secret@db:1433/M",
        );
        std::env::set_var("KOKKAK_DATABASE__POOL_SIZE", "50");
        std::env::set_var("KOKKAK_REDIS__URL", "redis://redis:6379");
        std::env::set_var("KOKKAK_NATS__URL", "nats://nats:4222");
        std::env::set_var("KOKKAK_NATS__STREAM_PREFIX", "kokkak.staging");
        std::env::set_var("KOKKAK_MONGO__URL", "mongodb://mongo:27017");
        std::env::set_var("KOKKAK_MONGO__DATABASE", "kokkak_staging");
        std::env::set_var("KOKKAK_DATA_DIR__PATH", "/tmp/kokkak");
        std::env::set_var("KOKKAK_AUTH__JWT_SECRET", "dev-secret");
        std::env::set_var("KOKKAK_AUTH__ACCESS_TTL_SECS", "300");

        let s = Settings::load().expect("load should succeed");
        assert_eq!(s.server.addr, "127.0.0.1:9999");
        assert_eq!(s.server.workers, 8);
        assert_eq!(s.log.format, LogFormat::Json);
        assert_eq!(s.database.sqlserver_url, "sqlserver://sa:secret@db:1433/M");
        assert_eq!(s.database.pool_size, 50);
        assert!(s.database.is_configured());
        assert_eq!(s.redis.url, "redis://redis:6379");
        assert!(s.redis.is_configured());
        assert_eq!(s.nats.url, "nats://nats:4222");
        assert_eq!(s.nats.stream_prefix, "kokkak.staging");
        assert!(s.nats.is_configured());
        assert_eq!(s.mongo.url, "mongodb://mongo:27017");
        assert_eq!(s.mongo.database, "kokkak_staging");
        assert!(s.mongo.is_configured());
        assert_eq!(s.data_dir.path, "/tmp/kokkak");
        assert!(s.auth.is_configured());
        assert_eq!(s.auth.access_ttl_secs, 300);

        clear_kokkak_env();
    }

    #[test]
    fn invalid_log_format_fails() {
        let _guard = ENV_LOCK.lock().expect("mutex poisoned");
        clear_kokkak_env();
        std::env::set_var("KOKKAK_LOG__FORMAT", "xml");

        let result = Settings::load();
        assert!(result.is_err(), "invalid format should fail to parse");
    }

    #[test]
    fn empty_addr_fails_validation() {
        let s = Settings {
            server: ServerSettings {
                addr: "".into(),
                workers: 4,
            },
            ..Settings::default()
        };
        assert!(s.validate().is_err());
    }

    #[test]
    fn whitespace_only_addr_fails_validation() {
        let s = Settings {
            server: ServerSettings {
                addr: "   ".into(),
                workers: 4,
            },
            ..Settings::default()
        };
        assert!(s.validate().is_err());
    }

    #[test]
    fn zero_workers_fails_validation() {
        let s = Settings {
            server: ServerSettings {
                addr: "0.0.0.0:3000".into(),
                workers: 0,
            },
            ..Settings::default()
        };
        assert!(s.validate().is_err());
    }

    #[test]
    fn zero_db_pool_size_fails_validation() {
        let s = Settings {
            database: DatabaseSettings {
                pool_size: 0,
                ..DatabaseSettings::default()
            },
            ..Settings::default()
        };
        assert!(s.validate().is_err());
    }

    #[test]
    fn zero_redis_pool_size_fails_validation() {
        let s = Settings {
            redis: RedisSettings {
                pool_size: 0,
                ..RedisSettings::default()
            },
            ..Settings::default()
        };
        assert!(s.validate().is_err());
    }

    #[test]
    fn empty_nats_prefix_fails_validation() {
        let s = Settings {
            nats: NatsSettings {
                stream_prefix: "".into(),
                ..NatsSettings::default()
            },
            ..Settings::default()
        };
        assert!(s.validate().is_err());
    }

    #[test]
    fn empty_mongo_database_fails_validation() {
        let s = Settings {
            mongo: MongoSettings {
                database: "".into(),
                ..MongoSettings::default()
            },
            ..Settings::default()
        };
        assert!(s.validate().is_err());
    }

    #[test]
    fn empty_data_dir_fails_validation() {
        let s = Settings {
            data_dir: DataDirSettings {
                path: "".into(),
                ..DataDirSettings::default()
            },
            ..Settings::default()
        };
        assert!(s.validate().is_err());
    }

    #[test]
    fn log_format_parses_both_variants() {
        let json: LogFormat = serde_json::from_str("\"json\"").unwrap();
        let pretty: LogFormat = serde_json::from_str("\"pretty\"").unwrap();
        assert_eq!(json, LogFormat::Json);
        assert_eq!(pretty, LogFormat::Pretty);
    }

    #[test]
    fn is_configured_treats_placeholder_url_as_unset() {
        let s = Settings::default();
        assert!(!s.database.is_configured());
        assert!(!s.redis.is_configured());
        assert!(!s.nats.is_configured());
        assert!(!s.mongo.is_configured());
    }

    #[test]
    fn auth_default_is_unconfigured() {
        let a = AuthSettings::default();
        assert!(!a.is_configured());
        assert_eq!(a.issuer, "kokkak-api");
        assert_eq!(a.access_ttl_secs, 900);
        assert_eq!(a.refresh_ttl_secs, 2_592_000);
    }
}
