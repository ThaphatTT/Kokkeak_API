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
    Invalid {
        /// Config key that failed validation (e.g. `"server.addr"`).
        key: String,
        /// Human-readable reason (used in error messages + logs).
        message: String,
    },
}

impl From<figment::Error> for ConfigError {
    fn from(err: figment::Error) -> Self {
        Self::Figment(Box::new(err))
    }
}

/// Top-level settings struct (โครงสร้างตั้งค่าระดับบนสุด).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct Settings {
    /// HTTP server settings (การตั้งค่า HTTP server).
    #[serde(default)]
    pub server: ServerSettings,

    /// Logging settings (การตั้งค่า log).
    #[serde(default)]
    pub log: LogSettings,

    /// SQL Server database settings (T06).
    /// Empty by default; production MUST set `KOKKAK_DATABASE__SQLSERVER_URL`.
    /// Acts as the **catch-all** for [`Settings::database_topology`].
    #[serde(default)]
    pub database: DatabaseSettings,

    /// Multi-DB connection topology (M12). One pool per
    /// [`DbRole`]. When a role's URL is empty it inherits from
    /// the legacy [`Self::database`] field. See module-level
    /// docs for the env-var contract.
    #[serde(default)]
    pub database_topology: DatabaseTopologySettings,

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

    /// Deployment environment (T-11). Defaults to `development`;
    /// `production` enables the strict validation path
    /// (TLS must be enabled, etc.).
    #[serde(default)]
    pub environment: Environment,

    /// TLS / HTTPS settings (T-08). Disabled by default so dev
    /// runs can use plain HTTP on `server.addr`. Production
    /// deployments enable TLS and supply cert + key paths.
    #[serde(default)]
    pub tls: TlsSettings,

    /// HTTP middleware stack (T-06): CORS allowlist, request
    /// timeout, response compression. Defaults are production-safe
    /// (deny CORS, 30 s timeout, compression on).
    #[serde(default)]
    pub middleware: MiddlewareSettings,
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

/// One slot in the multi-DB topology (M12).
///
/// Each role corresponds to a physical SQL Server database
/// (`KOKKAK_MASTER`, `KOKKAK_CATALOG`, ...). Lives in the
/// `config` module so the topology struct (which needs it)
/// can live above `infra`. Adding a new role is a deliberate
/// change: the compiler will guide every repository that needs
/// updating via [`DatabaseTopologySettings::for_role`] / the
/// matching `Mssql*Repository::new` calls.
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub enum DbRole {
    /// `KOKKAK_MASTER` — auth, user, RBAC, geo, config, bank, vat, commission, transport.
    Master,
    /// `KOKKAK_CATALOG` — services, categories, fees, warranties.
    Catalog,
    /// `KOKKAK_ORDER` — orders, bodies, stages, assignments, reviews, addons.
    Order,
    /// `KOKKAK_PAYMENT` — payments, statements, payouts.
    Payment,
    /// `KOKKAK_LOG` — audit, error log, login history.
    Log,
    /// `KOKKAK_REPORT` — read-only views for reports.
    Report,
    /// `KOKKAK_TEMP` — migration staging.
    Temp,
}

impl DbRole {
    /// Canonical env-var suffix: `KOKKAK_DATABASE__MASTER_URL`, etc.
    pub const fn env_suffix(self) -> &'static str {
        match self {
            Self::Master => "MASTER_URL",
            Self::Catalog => "CATALOG_URL",
            Self::Order => "ORDER_URL",
            Self::Payment => "PAYMENT_URL",
            Self::Log => "LOG_URL",
            Self::Report => "REPORT_URL",
            Self::Temp => "TEMP_URL",
        }
    }

    /// Stable string id (used in logs + health checks).
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Master => "master",
            Self::Catalog => "catalog",
            Self::Order => "order",
            Self::Payment => "payment",
            Self::Log => "log",
            Self::Report => "report",
            Self::Temp => "temp",
        }
    }

    /// All roles, in stable iteration order.
    pub const ALL: [DbRole; 7] = [
        Self::Master,
        Self::Catalog,
        Self::Order,
        Self::Payment,
        Self::Log,
        Self::Report,
        Self::Temp,
    ];
}

impl std::fmt::Display for DbRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
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

/// Convenience constructor used by tests and by
/// [`crate::config::DatabaseTopologySettings::from_settings`].
/// Lets the caller write `DatabaseSettings::from_url("Server=...")`
/// without juggling four `Default` fields.
impl DatabaseSettings {
    /// Build a `DatabaseSettings` from a single connection URL,
    /// using the default pool size / timeout / migrations dir.
    ///
    /// Recognised URL forms (forwarded verbatim to
    /// `kokkak_infra::db::mssql::parse_connection_url`):
    /// - ADO.NET: `Server=host;Database=db;User Id=u;Password=p`
    /// - URL:     `mssql://user:pass@host:1433/db?encrypt=true`
    /// - JDBC:    `jdbc:sqlserver://host:1433;database=db;...`
    pub fn from_url(url: impl Into<String>) -> Self {
        Self {
            sqlserver_url: url.into(),
            ..Self::default()
        }
    }
}

/// Multi-DB connection topology (M12).
///
/// Implements the per-role connection-strings defined in
/// `AGENTS.md` § 7.1. Each role (`master`, `catalog`, `order`, ...)
/// has its own [`DatabaseSettings`]. Roles whose URL is empty
/// inherit from the `catch_all` slot, which itself is populated
/// from the legacy [`Settings::database`] field at startup.
///
/// ## Env-var contract
///
/// ```text
/// KOKKAK_DATABASE__SQLSERVER_URL          (legacy catch-all — still works)
/// KOKKAK_DATABASE__CATCH_ALL_URL          (new — equivalent, takes precedence)
/// KOKKAK_DATABASE__MASTER_URL             (per-role override)
/// KOKKAK_DATABASE__CATALOG_URL            (per-role override)
/// KOKKAK_DATABASE__ORDER_URL              (per-role override)
/// KOKKAK_DATABASE__PAYMENT_URL            (per-role override)
/// KOKKAK_DATABASE__LOG_URL                (per-role override)
/// KOKKAK_DATABASE__REPORT_URL             (per-role override)
/// KOKKAK_DATABASE__TEMP_URL               (per-role override)
/// ```
///
/// The `Migrations` / `Pool size` / `Connect timeout` fields are
/// per-role. If you set only the URL, defaults are used.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct DatabaseTopologySettings {
    /// Fallback URL used by every role that has no per-role URL.
    /// Mirrors the legacy `Settings.database.sqlserver_url` for
    /// backward compat.
    #[serde(default)]
    pub catch_all: DatabaseSettings,
    /// `KOKKAK_MASTER` — auth, user, RBAC, geo, config, bank, vat, commission, transport.
    #[serde(default)]
    pub master: DatabaseSettings,
    /// `KOKKAK_CATALOG` — services, categories, fees, warranties.
    #[serde(default)]
    pub catalog: DatabaseSettings,
    /// `KOKKAK_ORDER` — orders, bodies, stages, assignments, reviews, addons.
    #[serde(default)]
    pub order: DatabaseSettings,
    /// `KOKKAK_PAYMENT` — payments, statements, payouts.
    #[serde(default)]
    pub payment: DatabaseSettings,
    /// `KOKKAK_LOG` — audit, error log, login history.
    #[serde(default)]
    pub log: DatabaseSettings,
    /// `KOKKAK_REPORT` — read-only views for reports.
    #[serde(default)]
    pub report: DatabaseSettings,
    /// `KOKKAK_TEMP` — migration staging.
    #[serde(default)]
    pub temp: DatabaseSettings,
}

impl DatabaseTopologySettings {
    /// Synthesise a topology from the top-level [`Settings`].
    ///
    /// For each role:
    /// 1. If the per-role `sqlserver_url` is set, use it.
    /// 2. Otherwise, if the top-level `database.sqlserver_url` is
    ///    set, inherit it (preserving M10 behaviour).
    /// 3. Otherwise, the role is unconfigured.
    ///
    /// Per-role `pool_size` / `connect_timeout_secs` /
    /// `migrations_dir` follow the same precedence: per-role
    /// > catch-all > default.
    pub fn from_settings(s: &Settings) -> Self {
        let mut out = DatabaseTopologySettings {
            catch_all: s.database.clone(),
            ..Self::default()
        };
        for role in crate::config::DbRole::ALL {
            let per_role = s.database_topology.settings_for(role);
            let merged = merge_with_fallback(per_role, &s.database);
            *out.slot_mut(role) = merged;
        }
        out
    }

    /// Borrow the settings for a role, with the catch-all fallback
    /// applied. **Returns the slot itself, not the merged value** —
    /// use this when the caller already knows the role is
    /// configured and wants to inspect its own URL.
    pub fn slot(&self, role: crate::config::DbRole) -> &DatabaseSettings {
        match role {
            crate::config::DbRole::Master => &self.master,
            crate::config::DbRole::Catalog => &self.catalog,
            crate::config::DbRole::Order => &self.order,
            crate::config::DbRole::Payment => &self.payment,
            crate::config::DbRole::Log => &self.log,
            crate::config::DbRole::Report => &self.report,
            crate::config::DbRole::Temp => &self.temp,
        }
    }

    /// Mutable accessor — see [`Self::slot`].
    pub fn slot_mut(&mut self, role: crate::config::DbRole) -> &mut DatabaseSettings {
        match role {
            crate::config::DbRole::Master => &mut self.master,
            crate::config::DbRole::Catalog => &mut self.catalog,
            crate::config::DbRole::Order => &mut self.order,
            crate::config::DbRole::Payment => &mut self.payment,
            crate::config::DbRole::Log => &mut self.log,
            crate::config::DbRole::Report => &mut self.report,
            crate::config::DbRole::Temp => &mut self.temp,
        }
    }

    /// Effective `DatabaseSettings` for a role: per-role slot,
    /// filled in with the catch-all values wherever the per-role
    /// slot is empty. This is what the topology builder feeds to
    /// `tiberius`.
    pub fn for_role(&self, role: crate::config::DbRole) -> DatabaseSettings {
        let slot = self.slot(role);
        if !slot.sqlserver_url.trim().is_empty() {
            return slot.clone();
        }
        // Per-role URL empty: inherit the catch-all.
        if !self.catch_all.sqlserver_url.trim().is_empty() {
            // Use the catch-all URL; keep the per-role pool size
            // override if the operator set one.
            let mut out = self.catch_all.clone();
            if slot.pool_size != default_db_pool_size() {
                out.pool_size = slot.pool_size;
            }
            if slot.connect_timeout_secs != default_db_connect_timeout_secs() {
                out.connect_timeout_secs = slot.connect_timeout_secs;
            }
            if slot.migrations_dir != default_migrations_dir() {
                out.migrations_dir = slot.migrations_dir.clone();
            }
            return out;
        }
        slot.clone()
    }

    /// Private helper for `from_settings`: read the per-role slot
    /// from a `Settings.database_topology` instance.
    fn settings_for(&self, role: crate::config::DbRole) -> &DatabaseSettings {
        self.slot(role)
    }
}

/// Merge a per-role slot with the legacy catch-all. The per-role
/// slot wins wherever it has been explicitly set. This is the
/// serde-level version of [`DatabaseTopologySettings::for_role`].
fn merge_with_fallback(
    per_role: &DatabaseSettings,
    catch_all: &DatabaseSettings,
) -> DatabaseSettings {
    let mut out = catch_all.clone();
    if !per_role.sqlserver_url.trim().is_empty() {
        out.sqlserver_url = per_role.sqlserver_url.clone();
    }
    if per_role.pool_size != default_db_pool_size() {
        out.pool_size = per_role.pool_size;
    }
    if per_role.connect_timeout_secs != default_db_connect_timeout_secs() {
        out.connect_timeout_secs = per_role.connect_timeout_secs;
    }
    if per_role.migrations_dir != default_migrations_dir() {
        out.migrations_dir = per_role.migrations_dir.clone();
    }
    out
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
    /// `true` iff a real Redis URL is set (not the `redis://disabled` sentinel).
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
    /// `true` iff a real NATS URL is set (not the `nats://disabled` sentinel).
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
    /// MongoDB connection URL.
    #[serde(default = "default_mongo_url")]
    pub url: String,

    /// Logical database name (e.g. `"kokkak"`).
    #[serde(default = "default_mongo_db")]
    pub database: String,
}

impl MongoSettings {
    /// `true` iff a real MongoDB URL is set (not the `mongodb://disabled` sentinel).
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

/// Deployment environment (T-11 production enforcement builds on
/// this). Set via `KOKKAK_ENVIRONMENT` (case-insensitive:
/// `development` | `production`). Defaults to `development`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Environment {
    /// Local development: relaxed validation (TLS optional,
    /// default secrets acceptable).
    #[default]
    Development,
    /// Production deployment: stricter validation enforced in
    /// [`Settings::validate`].
    Production,
}

impl Environment {
    /// True when [`Self::Production`]. Convenience for the
    /// production-only code paths.
    pub const fn is_production(self) -> bool {
        matches!(self, Self::Production)
    }
}

/// TLS / HTTPS settings (T-08).
///
/// Layered on top of [`ServerSettings`]: when
/// [`TlsSettings::enabled`] is `false` the API still serves plain
/// HTTP on `server.addr` (dev mode). When `true`, the
/// [`crate::main`](kokkak_api) entry point must construct a
/// `rustls` server config from `cert_path` + `key_path` and bind
/// with `axum_server` (T-09); HSTS is added by middleware (T-10);
/// production enforcement lives in [`Settings::validate`] (T-11).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TlsSettings {
    /// Enable HTTPS. When `true`, `cert_path` and `key_path` must
    /// point to readable PEM files (validated at startup, not by
    /// `Settings::validate` — see T-09).
    #[serde(default)]
    pub enabled: bool,

    /// Path to the PEM-encoded TLS certificate (full chain).
    /// Required only when `enabled = true`.
    #[serde(default)]
    pub cert_path: Option<String>,

    /// Path to the PEM-encoded TLS private key.
    /// Required only when `enabled = true`.
    #[serde(default)]
    pub key_path: Option<String>,

    /// Plain-HTTP listener port for the HTTPS redirect server
    /// (T-10). Typical value: `80`. Set to `0` to disable.
    /// Ignored when `enabled = false`.
    #[serde(default)]
    pub redirect_from_port: u16,

    /// HSTS `max-age` in seconds. `0` = HSTS disabled.
    /// Recommended for production: `31536000` (1 year).
    /// Ignored when `enabled = false`.
    #[serde(default = "default_hsts_max_age_secs")]
    pub hsts_max_age_secs: u64,

    /// Auto-reload on cert file change (T-12). When `true`, the
    /// service watches `cert_path` + `key_path` for modifications
    /// and triggers a graceful shutdown so systemd/k8s can restart
    /// with the new chain (LE 90-day rotation). Defaults to `false`
    /// because the restart causes a brief connection blip — operators
    /// who can tolerate that opt in; everyone else can rely on the
    /// periodic restart performed by their orchestrator.
    #[serde(default)]
    pub auto_reload: bool,
}

impl Default for TlsSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            cert_path: None,
            key_path: None,
            redirect_from_port: 0,
            hsts_max_age_secs: default_hsts_max_age_secs(),
            auto_reload: false,
        }
    }
}

impl TlsSettings {
    /// Trimmed `cert_path` (empty when unset or blank).
    pub fn cert_path_or_empty(&self) -> &str {
        self.cert_path.as_deref().unwrap_or("").trim()
    }

    /// Trimmed `key_path` (empty when unset or blank).
    pub fn key_path_or_empty(&self) -> &str {
        self.key_path.as_deref().unwrap_or("").trim()
    }
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
fn default_hsts_max_age_secs() -> u64 {
    0
}

/// HTTP middleware settings (T-06).
///
/// Loaded from env vars prefixed `KOKKAK_MIDDLEWARE__*`. Defaults
/// are production-safe: deny all CORS origins, 30-second request
/// timeout, compression on.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MiddlewareSettings {
    /// Allowlist of CORS origins. Empty (default) means **deny all
    /// cross-origin requests** — the browser will block the
    /// request client-side. Operators MUST opt-in by setting
    /// `KOKKAK_MIDDLEWARE__CORS_ALLOW_ORIGINS` to either a
    /// comma-separated string (`https://app.example.com,https://admin.example.com`)
    /// or a TOML array. The custom deserializer accepts both.
    ///
    /// Wildcard `*` is intentionally NOT supported because it
    /// cannot be combined with `allow_credentials(true)`, and the
    /// marketplace endpoints use cookies for the BFF.
    #[serde(default, deserialize_with = "deserialize_comma_list")]
    pub cors_allow_origins: Vec<String>,

    /// Maximum request duration in seconds. `0` disables the
    /// timeout entirely (NOT recommended for production — slow
    /// handlers will tie up tokio workers forever).
    #[serde(default = "default_request_timeout_secs")]
    pub request_timeout_secs: u64,

    /// Enable response compression (gzip/deflate/br based on the
    /// client's `Accept-Encoding`). Defaults to `true` — the CPU
    /// cost is negligible on modern hardware and saves bandwidth
    /// for JSON-heavy mobile clients.
    #[serde(default = "default_compression_enabled")]
    pub compression_enabled: bool,

    /// Per-IP rate limit (T-07). Disabled by default in dev so
    /// hot-reload + integration tests don't trip the limiter;
    /// production deployments should enable it (recommended 100
    /// rps + burst 200, tuned per workload).
    #[serde(default)]
    pub rate_limit: RateLimitSettings,

    /// HTTP idempotency cache (T-14). Disabled by default in dev
    /// (no point caching test traffic); production deployments
    /// should enable it so mobile retries on flaky networks don't
    /// create duplicate orders/payments.
    #[serde(default)]
    pub idempotency: IdempotencySettings,
}

/// HTTP idempotency settings (T-14). See
/// [`crate::middleware::idempotency`] for the on-the-wire contract.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IdempotencySettings {
    /// Master switch. `false` (default) means the middleware
    /// short-circuits to a passthrough — no cache, no overhead.
    #[serde(default)]
    pub enabled: bool,

    /// Time-to-live for cached responses. Matches the IETF
    /// `Idempotency-Key` draft and Stripe's contract. Default
    /// 24 hours — long enough to survive an overnight mobile
    /// retry, short enough to bound the storage footprint.
    #[serde(default = "default_idempotency_ttl_secs")]
    pub ttl_secs: u64,

    /// Soft cap on the number of cached entries. The in-memory
    /// store evicts half the entries when the cap is hit
    /// (Ponytail: half-flush is the cheapest correct policy;
    /// swap for LRU when traffic demands).
    #[serde(default = "default_idempotency_max_entries")]
    pub max_entries: usize,
}

impl Default for IdempotencySettings {
    fn default() -> Self {
        Self {
            enabled: false,
            ttl_secs: default_idempotency_ttl_secs(),
            max_entries: default_idempotency_max_entries(),
        }
    }
}

fn default_idempotency_ttl_secs() -> u64 {
    86_400 // 24 hours
}

fn default_idempotency_max_entries() -> usize {
    10_000
}

/// Per-IP rate limit (T-07). Uses the GCRA algorithm via
/// `tower_governor` + `governor`. Default OFF (dev mode) — the
/// marketplace endpoints sit behind the BFF in production, so
/// per-IP limiting at the Rust layer is a backstop rather than
/// the primary defence.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RateLimitSettings {
    /// Master switch. `false` (default) means no rate-limit layer
    /// is wired — the request pipeline runs unmodified.
    #[serde(default)]
    pub enabled: bool,

    /// Sustained request rate per IP, in requests/second. Must be
    /// >= 1 if [`Self::enabled`] is true.
    #[serde(default = "default_rate_per_second")]
    pub requests_per_second: u32,

    /// Token-bucket burst capacity. A burst above the sustained
    /// rate is allowed as long as the bucket has tokens. Must be
    /// >= 1 if [`Self::enabled`] is true.
    #[serde(default = "default_rate_burst_size")]
    pub burst_size: u32,
}

impl Default for RateLimitSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            requests_per_second: default_rate_per_second(),
            burst_size: default_rate_burst_size(),
        }
    }
}

fn default_rate_per_second() -> u32 {
    100
}

fn default_rate_burst_size() -> u32 {
    200
}

impl Default for MiddlewareSettings {
    fn default() -> Self {
        Self {
            cors_allow_origins: Vec::new(),
            request_timeout_secs: default_request_timeout_secs(),
            compression_enabled: default_compression_enabled(),
            rate_limit: RateLimitSettings::default(),
            idempotency: IdempotencySettings::default(),
        }
    }
}

/// Accept either a JSON/TOML array of strings or a single
/// comma-separated string. figment's Env provider hands us the
/// latter (env vars are scalars), so we split on `,` to keep the
/// operator UX ergonomic without paying for a custom provider.
fn deserialize_comma_list<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum StringOrVec {
        Vec(Vec<String>),
        Str(String),
    }

    match StringOrVec::deserialize(deserializer)? {
        StringOrVec::Vec(v) => Ok(v),
        StringOrVec::Str(s) => Ok(s
            .split(',')
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .map(String::from)
            .collect()),
    }
}

fn default_request_timeout_secs() -> u64 {
    30
}

fn default_compression_enabled() -> bool {
    true
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
        // M12: also check the per-role slots in the topology.
        // They use the same defaults as `database` so a zero
        // here is a misconfiguration regardless of whether the
        // URL itself is set.
        for role in DbRole::ALL {
            let s = self.database_topology.slot(role);
            if s.pool_size == 0 {
                return Err(ConfigError::Invalid {
                    key: format!("KOKKAK_DATABASE__{}__POOL_SIZE", role.env_suffix()),
                    message: "must be >= 1".into(),
                });
            }
            if s.connect_timeout_secs == 0 {
                return Err(ConfigError::Invalid {
                    key: format!(
                        "KOKKAK_DATABASE__{}__CONNECT_TIMEOUT_SECS",
                        role.env_suffix()
                    ),
                    message: "must be >= 1".into(),
                });
            }
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
        // T-08: when TLS is enabled, both paths must be present
        // and non-empty. File existence + readability are checked
        // at startup by the T-09 server bootstrap, not here, so a
        // config-only validation pass stays fast and doesn't depend
        // on the filesystem layout (k8s mounts the secret after
        // process start in some deployment flows).
        if self.tls.enabled {
            if self.tls.cert_path_or_empty().is_empty() {
                return Err(ConfigError::Invalid {
                    key: "KOKKAK_TLS__CERT_PATH".into(),
                    message: "must not be empty when KOKKAK_TLS__ENABLED=true".into(),
                });
            }
            if self.tls.key_path_or_empty().is_empty() {
                return Err(ConfigError::Invalid {
                    key: "KOKKAK_TLS__KEY_PATH".into(),
                    message: "must not be empty when KOKKAK_TLS__ENABLED=true".into(),
                });
            }
        }
        // T-11: production deployments must serve over TLS. A
        // bare-HTTP production rollout is the #1 way to leak
        // JWTs and PII through misconfigured reverse proxies, so
        // fail the process at startup rather than discover it
        // in a post-mortem. The dev path is unaffected — plain
        // HTTP stays the default.
        if self.environment.is_production() && !self.tls.enabled {
            return Err(ConfigError::Invalid {
                key: "KOKKAK_TLS__ENABLED".into(),
                message: "must be true when KOKKAK_ENVIRONMENT=production".into(),
            });
        }
        // T-06: middleware defaults are all-zero / safe, but a
        // production deployment with CORS allowlist = [] means
        // the BFF / mobile app cannot reach the API at all.
        // Fail loudly so operators discover it during deploy, not
        // when users start reporting 403s.
        if self.environment.is_production() && self.middleware.cors_allow_origins.is_empty() {
            return Err(ConfigError::Invalid {
                key: "KOKKAK_MIDDLEWARE__CORS_ALLOW_ORIGINS".into(),
                message: "must list at least one origin when KOKKAK_ENVIRONMENT=production".into(),
            });
        }
        // T-07: production rate-limit knobs must be positive when
        // enabled — a 0-rps limiter would block every request.
        if self.middleware.rate_limit.enabled {
            if self.middleware.rate_limit.requests_per_second == 0 {
                return Err(ConfigError::Invalid {
                    key: "KOKKAK_MIDDLEWARE__RATE_LIMIT__REQUESTS_PER_SECOND".into(),
                    message: "must be >= 1 when rate limiting is enabled".into(),
                });
            }
            if self.middleware.rate_limit.burst_size == 0 {
                return Err(ConfigError::Invalid {
                    key: "KOKKAK_MIDDLEWARE__RATE_LIMIT__BURST_SIZE".into(),
                    message: "must be >= 1 when rate limiting is enabled".into(),
                });
            }
        }
        // T-14: idempotency cache must have positive knobs when
        // enabled — a 0-ttl cache would never replay anything.
        if self.middleware.idempotency.enabled {
            if self.middleware.idempotency.ttl_secs == 0 {
                return Err(ConfigError::Invalid {
                    key: "KOKKAK_MIDDLEWARE__IDEMPOTENCY__TTL_SECS".into(),
                    message: "must be >= 1 when idempotency is enabled".into(),
                });
            }
            if self.middleware.idempotency.max_entries == 0 {
                return Err(ConfigError::Invalid {
                    key: "KOKKAK_MIDDLEWARE__IDEMPOTENCY__MAX_ENTRIES".into(),
                    message: "must be >= 1 when idempotency is enabled".into(),
                });
            }
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
            "KOKKAK_DATABASE__CATCH_ALL_URL",
            "KOKKAK_DATABASE__MASTER_URL",
            "KOKKAK_DATABASE__CATALOG_URL",
            "KOKKAK_DATABASE__ORDER_URL",
            "KOKKAK_DATABASE__PAYMENT_URL",
            "KOKKAK_DATABASE__LOG_URL",
            "KOKKAK_DATABASE__REPORT_URL",
            "KOKKAK_DATABASE__TEMP_URL",
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
            "KOKKAK_ENVIRONMENT",
            "KOKKAK_TLS__ENABLED",
            "KOKKAK_TLS__CERT_PATH",
            "KOKKAK_TLS__KEY_PATH",
            "KOKKAK_TLS__REDIRECT_FROM_PORT",
            "KOKKAK_TLS__HSTS_MAX_AGE_SECS",
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

    // ---- T-08: TLS settings ----

    #[test]
    fn tls_default_is_disabled() {
        let t = TlsSettings::default();
        assert!(!t.enabled);
        assert_eq!(t.cert_path, None);
        assert_eq!(t.key_path, None);
        assert_eq!(t.redirect_from_port, 0);
        assert_eq!(t.hsts_max_age_secs, 0);
        // T-12: auto_reload defaults to false to avoid surprise
        // restarts; operators opt in via KOKKAK_TLS__AUTO_RELOAD=true.
        assert!(!t.auto_reload);
    }

    #[test]
    fn tls_auto_reload_load_from_env_overrides() {
        clear_kokkak_env();
        std::env::set_var("KOKKAK_TLS__AUTO_RELOAD", "true");
        let s = Settings::load().expect("load should succeed");
        assert!(s.tls.auto_reload);
        clear_kokkak_env();
    }

    #[test]
    fn tls_default_settings_validate() {
        // Plain-HTTP dev default: TLS off, no cert/key required.
        let s = Settings::default();
        assert!(!s.tls.enabled);
        assert!(s.validate().is_ok());
    }

    #[test]
    fn tls_enabled_without_cert_path_fails_validation() {
        let s = Settings {
            tls: TlsSettings {
                enabled: true,
                cert_path: None,
                key_path: Some("/etc/kokkak/key.pem".into()),
                redirect_from_port: 0,
                hsts_max_age_secs: 0,
                auto_reload: false,
            },
            ..Settings::default()
        };
        let err = s.validate().expect_err("must reject missing cert_path");
        assert!(
            err.to_string().contains("KOKKAK_TLS__CERT_PATH"),
            "error should mention cert_path key, got: {err}"
        );
    }

    #[test]
    fn tls_enabled_with_blank_cert_path_fails_validation() {
        let s = Settings {
            tls: TlsSettings {
                enabled: true,
                cert_path: Some("   ".into()),
                key_path: Some("/etc/kokkak/key.pem".into()),
                redirect_from_port: 0,
                hsts_max_age_secs: 0,
                auto_reload: false,
            },
            ..Settings::default()
        };
        assert!(s.validate().is_err());
    }

    #[test]
    fn tls_enabled_without_key_path_fails_validation() {
        let s = Settings {
            tls: TlsSettings {
                enabled: true,
                cert_path: Some("/etc/kokkak/cert.pem".into()),
                key_path: None,
                redirect_from_port: 0,
                hsts_max_age_secs: 0,
                auto_reload: false,
            },
            ..Settings::default()
        };
        let err = s.validate().expect_err("must reject missing key_path");
        assert!(
            err.to_string().contains("KOKKAK_TLS__KEY_PATH"),
            "error should mention key_path key, got: {err}"
        );
    }

    #[test]
    fn tls_enabled_with_both_paths_validates() {
        let s = Settings {
            tls: TlsSettings {
                enabled: true,
                cert_path: Some("/etc/kokkak/cert.pem".into()),
                key_path: Some("/etc/kokkak/key.pem".into()),
                redirect_from_port: 80,
                hsts_max_age_secs: 31_536_000,
                auto_reload: false,
            },
            ..Settings::default()
        };
        // File existence is NOT checked by validate() — that's the
        // T-09 server bootstrap's job. The settings alone pass.
        assert!(s.validate().is_ok());
    }

    #[test]
    fn environment_default_is_development() {
        let s = Settings::default();
        assert_eq!(s.environment, Environment::Development);
        assert!(!s.environment.is_production());
    }

    #[test]
    fn environment_parses_production_value() {
        let e: Environment = serde_json::from_str("\"production\"").unwrap();
        assert_eq!(e, Environment::Production);
        assert!(e.is_production());
    }

    #[test]
    fn tls_settings_or_empty_returns_trimmed() {
        let t = TlsSettings {
            cert_path: Some("  /etc/kokkak/cert.pem  ".into()),
            key_path: Some("".into()),
            ..TlsSettings::default()
        };
        assert_eq!(t.cert_path_or_empty(), "/etc/kokkak/cert.pem");
        assert_eq!(t.key_path_or_empty(), "");
        let t_none = TlsSettings::default();
        assert_eq!(t_none.cert_path_or_empty(), "");
        assert_eq!(t_none.key_path_or_empty(), "");
    }

    #[test]
    fn tls_load_from_env_overrides() {
        let _guard = ENV_LOCK.lock().expect("mutex poisoned");
        clear_kokkak_env();
        // add the new TLS keys to the clear list (next test run sees
        // the canonical list — for now this test relies on
        // clear_kokkak_env removing everything it knows about and
        // the new keys being absent).
        std::env::set_var("KOKKAK_TLS__ENABLED", "true");
        std::env::set_var("KOKKAK_TLS__CERT_PATH", "/etc/kokkak/cert.pem");
        std::env::set_var("KOKKAK_TLS__KEY_PATH", "/etc/kokkak/key.pem");
        std::env::set_var("KOKKAK_TLS__REDIRECT_FROM_PORT", "80");
        std::env::set_var("KOKKAK_TLS__HSTS_MAX_AGE_SECS", "31536000");
        std::env::set_var("KOKKAK_ENVIRONMENT", "production");

        let s = Settings::load().expect("load should succeed");
        assert!(s.tls.enabled);
        assert_eq!(s.tls.cert_path.as_deref(), Some("/etc/kokkak/cert.pem"));
        assert_eq!(s.tls.key_path.as_deref(), Some("/etc/kokkak/key.pem"));
        assert_eq!(s.tls.redirect_from_port, 80);
        assert_eq!(s.tls.hsts_max_age_secs, 31_536_000);
        assert_eq!(s.environment, Environment::Production);
        assert!(s.validate().is_ok());

        clear_kokkak_env();
    }

    // ---- T-11: production enforcement ----

    #[test]
    fn production_without_tls_fails_validation() {
        let s = Settings {
            environment: Environment::Production,
            ..Settings::default()
        };
        let err = s
            .validate()
            .expect_err("production + plain HTTP must be rejected");
        assert!(
            err.to_string().contains("KOKKAK_TLS__ENABLED")
                && err.to_string().contains("production"),
            "error should point at TLS in a production context, got: {err}"
        );
    }

    #[test]
    fn production_with_tls_enabled_validates() {
        // T-06: a production deployment must also have a non-empty
        // CORS allowlist (the browser blocks cross-origin requests
        // otherwise). The TLS-only check is no longer sufficient.
        let s = Settings {
            environment: Environment::Production,
            tls: TlsSettings {
                enabled: true,
                cert_path: Some("/etc/kokkak/cert.pem".into()),
                key_path: Some("/etc/kokkak/key.pem".into()),
                redirect_from_port: 80,
                hsts_max_age_secs: 31_536_000,
                auto_reload: false,
            },
            middleware: MiddlewareSettings {
                cors_allow_origins: vec!["https://app.example.com".into()],
                ..MiddlewareSettings::default()
            },
            ..Settings::default()
        };
        assert!(s.validate().is_ok());
    }

    #[test]
    fn production_with_tls_enabled_but_missing_key_still_fails() {
        // The TLS-on-but-blank-paths rule from T-08 must still
        // trip in production. This guards against a future
        // refactor that reorders the validation checks and
        // accidentally lets a broken prod config through.
        let s = Settings {
            environment: Environment::Production,
            tls: TlsSettings {
                enabled: true,
                cert_path: Some("/etc/kokkak/cert.pem".into()),
                key_path: None,
                redirect_from_port: 80,
                hsts_max_age_secs: 31_536_000,
                auto_reload: false,
            },
            ..Settings::default()
        };
        let err = s.validate().expect_err("must reject blank key path");
        assert!(
            err.to_string().contains("KOKKAK_TLS__KEY_PATH"),
            "error should mention key_path, got: {err}"
        );
    }

    #[test]
    fn development_with_tls_disabled_still_validates() {
        // The T-11 enforcement must not regress dev mode.
        let s = Settings::default();
        assert_eq!(s.environment, Environment::Development);
        assert!(!s.tls.enabled);
        assert!(s.validate().is_ok());
    }

    // ---- T-06: middleware settings ----

    #[test]
    fn middleware_default_is_production_safe() {
        let s = Settings::default();
        assert!(
            s.middleware.cors_allow_origins.is_empty(),
            "default CORS allowlist must be empty (deny all)"
        );
        assert_eq!(s.middleware.request_timeout_secs, 30);
        assert!(s.middleware.compression_enabled);
        assert!(s.validate().is_ok());
    }

    #[test]
    fn middleware_load_from_env_overrides() {
        clear_kokkak_env();
        std::env::set_var(
            "KOKKAK_MIDDLEWARE__CORS_ALLOW_ORIGINS",
            "https://app.example.com,https://admin.example.com",
        );
        std::env::set_var("KOKKAK_MIDDLEWARE__REQUEST_TIMEOUT_SECS", "60");
        std::env::set_var("KOKKAK_MIDDLEWARE__COMPRESSION_ENABLED", "false");

        let s = Settings::load().expect("load should succeed");
        assert_eq!(
            s.middleware.cors_allow_origins,
            vec![
                "https://app.example.com".to_string(),
                "https://admin.example.com".to_string(),
            ]
        );
        assert_eq!(s.middleware.request_timeout_secs, 60);
        assert!(!s.middleware.compression_enabled);

        clear_kokkak_env();
    }

    #[test]
    fn middleware_zero_timeout_is_allowed() {
        // `0` means "disabled" — useful for long-running handlers
        // like chat WebSocket upgrades. Production deployments
        // should leave the 30 s default; opt-in to disable.
        let s = Settings {
            middleware: MiddlewareSettings {
                request_timeout_secs: 0,
                ..MiddlewareSettings::default()
            },
            ..Settings::default()
        };
        assert!(s.validate().is_ok());
    }

    #[test]
    fn production_without_cors_allowlist_fails_validation() {
        // Mirrors the T-11 TLS rule: a prod deployment with empty
        // CORS allowlist means the BFF / mobile app cannot reach
        // the API at all. Fail loudly at startup.
        let s = Settings {
            environment: Environment::Production,
            tls: TlsSettings {
                enabled: true,
                cert_path: Some("/etc/kokkak/cert.pem".into()),
                key_path: Some("/etc/kokkak/key.pem".into()),
                redirect_from_port: 80,
                hsts_max_age_secs: 31_536_000,
                auto_reload: false,
            },
            // cors_allow_origins: [] (default) — must fail.
            ..Settings::default()
        };
        let err = s
            .validate()
            .expect_err("production + empty CORS allowlist must be rejected");
        assert!(
            err.to_string()
                .contains("KOKKAK_MIDDLEWARE__CORS_ALLOW_ORIGINS")
                && err.to_string().contains("production"),
            "error should point at CORS in a production context, got: {err}"
        );
    }

    #[test]
    fn production_with_cors_allowlist_validates() {
        let s = Settings {
            environment: Environment::Production,
            tls: TlsSettings {
                enabled: true,
                cert_path: Some("/etc/kokkak/cert.pem".into()),
                key_path: Some("/etc/kokkak/key.pem".into()),
                redirect_from_port: 80,
                hsts_max_age_secs: 31_536_000,
                auto_reload: false,
            },
            middleware: MiddlewareSettings {
                cors_allow_origins: vec!["https://app.example.com".into()],
                ..MiddlewareSettings::default()
            },
            ..Settings::default()
        };
        assert!(s.validate().is_ok());
    }

    // ---- T-07: rate-limit settings ----

    #[test]
    fn rate_limit_default_is_disabled() {
        // Dev mode keeps the limiter off so hot-reload and
        // integration tests can hammer endpoints without
        // tripping 429s. Operators opt in via env.
        let s = Settings::default();
        assert!(!s.middleware.rate_limit.enabled);
        assert_eq!(s.middleware.rate_limit.requests_per_second, 100);
        assert_eq!(s.middleware.rate_limit.burst_size, 200);
        assert!(s.validate().is_ok());
    }

    #[test]
    fn rate_limit_load_from_env_overrides() {
        clear_kokkak_env();
        std::env::set_var("KOKKAK_MIDDLEWARE__RATE_LIMIT__ENABLED", "true");
        std::env::set_var("KOKKAK_MIDDLEWARE__RATE_LIMIT__REQUESTS_PER_SECOND", "50");
        std::env::set_var("KOKKAK_MIDDLEWARE__RATE_LIMIT__BURST_SIZE", "75");

        let s = Settings::load().expect("load should succeed");
        assert!(s.middleware.rate_limit.enabled);
        assert_eq!(s.middleware.rate_limit.requests_per_second, 50);
        assert_eq!(s.middleware.rate_limit.burst_size, 75);

        clear_kokkak_env();
    }

    #[test]
    fn rate_limit_zero_per_second_fails_when_enabled() {
        let s = Settings {
            middleware: MiddlewareSettings {
                rate_limit: RateLimitSettings {
                    enabled: true,
                    requests_per_second: 0,
                    burst_size: 100,
                },
                ..MiddlewareSettings::default()
            },
            ..Settings::default()
        };
        let err = s
            .validate()
            .expect_err("enabled limiter with 0 rps must be rejected");
        assert!(
            err.to_string()
                .contains("KOKKAK_MIDDLEWARE__RATE_LIMIT__REQUESTS_PER_SECOND"),
            "error should point at REQUESTS_PER_SECOND, got: {err}"
        );
    }

    #[test]
    fn rate_limit_zero_burst_fails_when_enabled() {
        let s = Settings {
            middleware: MiddlewareSettings {
                rate_limit: RateLimitSettings {
                    enabled: true,
                    requests_per_second: 100,
                    burst_size: 0,
                },
                ..MiddlewareSettings::default()
            },
            ..Settings::default()
        };
        let err = s
            .validate()
            .expect_err("enabled limiter with 0 burst must be rejected");
        assert!(
            err.to_string()
                .contains("KOKKAK_MIDDLEWARE__RATE_LIMIT__BURST_SIZE"),
            "error should point at BURST_SIZE, got: {err}"
        );
    }

    #[test]
    fn rate_limit_disabled_with_zero_knobs_validates() {
        // Zero knobs are fine when the limiter is OFF — operators
        // sometimes leave defaults untouched. The validation rule
        // only fires when `enabled = true`.
        let s = Settings {
            middleware: MiddlewareSettings {
                rate_limit: RateLimitSettings {
                    enabled: false,
                    requests_per_second: 0,
                    burst_size: 0,
                },
                ..MiddlewareSettings::default()
            },
            ..Settings::default()
        };
        assert!(s.validate().is_ok());
    }

    // ---- T-14: idempotency settings ----

    #[test]
    fn idempotency_default_is_disabled() {
        let s = Settings::default();
        assert!(!s.middleware.idempotency.enabled);
        assert_eq!(s.middleware.idempotency.ttl_secs, 86_400);
        assert_eq!(s.middleware.idempotency.max_entries, 10_000);
        assert!(s.validate().is_ok());
    }

    #[test]
    fn idempotency_load_from_env_overrides() {
        clear_kokkak_env();
        std::env::set_var("KOKKAK_MIDDLEWARE__IDEMPOTENCY__ENABLED", "true");
        std::env::set_var("KOKKAK_MIDDLEWARE__IDEMPOTENCY__TTL_SECS", "3600");
        std::env::set_var("KOKKAK_MIDDLEWARE__IDEMPOTENCY__MAX_ENTRIES", "5000");

        let s = Settings::load().expect("load should succeed");
        assert!(s.middleware.idempotency.enabled);
        assert_eq!(s.middleware.idempotency.ttl_secs, 3600);
        assert_eq!(s.middleware.idempotency.max_entries, 5000);

        clear_kokkak_env();
    }

    #[test]
    fn idempotency_zero_ttl_fails_when_enabled() {
        let s = Settings {
            middleware: MiddlewareSettings {
                idempotency: IdempotencySettings {
                    enabled: true,
                    ttl_secs: 0,
                    max_entries: 100,
                },
                ..MiddlewareSettings::default()
            },
            ..Settings::default()
        };
        let err = s
            .validate()
            .expect_err("enabled idempotency with 0 ttl must be rejected");
        assert!(
            err.to_string()
                .contains("KOKKAK_MIDDLEWARE__IDEMPOTENCY__TTL_SECS"),
            "error should point at TTL_SECS, got: {err}"
        );
    }

    #[test]
    fn idempotency_zero_max_entries_fails_when_enabled() {
        let s = Settings {
            middleware: MiddlewareSettings {
                idempotency: IdempotencySettings {
                    enabled: true,
                    ttl_secs: 60,
                    max_entries: 0,
                },
                ..MiddlewareSettings::default()
            },
            ..Settings::default()
        };
        let err = s
            .validate()
            .expect_err("enabled idempotency with 0 max_entries must be rejected");
        assert!(
            err.to_string()
                .contains("KOKKAK_MIDDLEWARE__IDEMPOTENCY__MAX_ENTRIES"),
            "error should point at MAX_ENTRIES, got: {err}"
        );
    }
}
