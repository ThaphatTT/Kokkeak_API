use figment::providers::{Env, Format, Toml};
use figment::Figment;
use serde::{Deserialize, Serialize};
use thiserror::Error;

fn load_env_file() {
    let candidates = [".env.production", ".env.dev"];
    for path in &candidates {
        if let Ok(contents) = std::fs::read_to_string(path) {
            tracing::debug!("loading env file: {path}");
            for line in contents.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                if let Some((key, val)) = line.split_once('=') {
                    let key = key.trim();
                    let val = val.trim().trim_matches('"').trim_matches('\'');
                    if std::env::var(key).is_err() {
                        std::env::set_var(key, val);
                    }
                }
            }
            return;
        }
    }
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("config provider error: {0}")]
    Figment(#[from] Box<figment::Error>),

    #[error("invalid config: key={key}, {message}")]
    Invalid { key: String, message: String },
}

impl From<figment::Error> for ConfigError {
    fn from(err: figment::Error) -> Self {
        Self::Figment(Box::new(err))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct Settings {
    #[serde(default)]
    pub server: ServerSettings,

    #[serde(default)]
    pub log: LogSettings,

    #[serde(default)]
    pub database: DatabaseSettings,

    #[serde(default)]
    pub database_topology: DatabaseTopologySettings,

    #[serde(default)]
    pub redis: RedisSettings,

    #[serde(default)]
    pub permission_cache: PermissionCacheSettings,

    #[serde(default)]
    pub nats: NatsSettings,

    #[serde(default)]
    pub mongo: MongoSettings,

    #[serde(default)]
    pub data_dir: DataDirSettings,

    #[serde(default)]
    pub auth: AuthSettings,

    #[serde(default)]
    pub environment: Environment,

    #[serde(default)]
    pub tls: TlsSettings,

    #[serde(default)]
    pub middleware: MiddlewareSettings,

    #[serde(default)]
    pub storage: StorageSettings,

    #[serde(default)]
    pub image: ImageProcessorSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StorageSettings {
    #[serde(default)]
    pub s3_bucket: String,

    #[serde(default)]
    pub s3_endpoint: String,

    #[serde(default = "default_storage_s3_region")]
    pub s3_region: String,

    #[serde(default)]
    pub s3_access_key: String,

    #[serde(default)]
    pub s3_secret_key: String,

    #[serde(default)]
    pub s3_path_style: bool,

    #[serde(default)]
    pub local_path: String,

    #[serde(default)]
    pub signed_url_secret: String,

    #[serde(default = "default_signed_url_ttl_secs")]
    pub signed_url_ttl_secs: u32,
}

fn default_signed_url_ttl_secs() -> u32 {
    600
}

impl Default for StorageSettings {
    fn default() -> Self {
        Self {
            s3_bucket: String::new(),
            s3_endpoint: String::new(),
            s3_region: default_storage_s3_region(),
            s3_access_key: String::new(),
            s3_secret_key: String::new(),
            s3_path_style: false,
            local_path: String::new(),
            signed_url_secret: String::new(),
            signed_url_ttl_secs: default_signed_url_ttl_secs(),
        }
    }
}

impl StorageSettings {
    pub fn signed_url_knobs_valid(&self) -> Result<(), ConfigError> {
        if self.signed_url_secret.trim().is_empty() {
            return Err(ConfigError::Invalid {
                key: "KOKKAK_STORAGE__SIGNED_URL_SECRET".into(),
                message: "must not be empty (min 32 bytes)".into(),
            });
        }
        if self.signed_url_secret.len() < 32 {
            return Err(ConfigError::Invalid {
                key: "KOKKAK_STORAGE__SIGNED_URL_SECRET".into(),
                message: format!(
                    "must be at least 32 bytes for HMAC-SHA256 (got {} bytes)",
                    self.signed_url_secret.len()
                ),
            });
        }
        if self.signed_url_ttl_secs < 60 || self.signed_url_ttl_secs > 3600 {
            return Err(ConfigError::Invalid {
                key: "KOKKAK_STORAGE__SIGNED_URL_TTL_SECS".into(),
                message: format!(
                    "must be between 60 and 3600 seconds (got {})",
                    self.signed_url_ttl_secs
                ),
            });
        }
        Ok(())
    }
}

impl StorageSettings {
    pub fn adapter_kind(&self) -> StorageAdapterKind {
        if !self.s3_bucket.trim().is_empty() {
            StorageAdapterKind::S3
        } else if !self.local_path.trim().is_empty() {
            StorageAdapterKind::Local
        } else {
            StorageAdapterKind::Memory
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StorageAdapterKind {
    S3,

    Local,

    Memory,
}

impl StorageAdapterKind {
    pub fn as_str(self) -> &'static str {
        match self {
            StorageAdapterKind::S3 => "s3",
            StorageAdapterKind::Local => "local",
            StorageAdapterKind::Memory => "memory",
        }
    }
}

fn default_storage_s3_region() -> String {
    "us-east-1".into()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImageProcessorSettings {
    #[serde(default = "default_image_max_input_bytes")]
    pub max_input_bytes: usize,

    #[serde(default = "default_image_max_dimension_px")]
    pub max_dimension_px: u32,

    #[serde(default = "default_image_webp_quality")]
    pub webp_quality: u8,
}

impl Default for ImageProcessorSettings {
    fn default() -> Self {
        Self {
            max_input_bytes: default_image_max_input_bytes(),
            max_dimension_px: default_image_max_dimension_px(),
            webp_quality: default_image_webp_quality(),
        }
    }
}

fn default_image_max_input_bytes() -> usize {
    1024 * 1024
}
fn default_image_max_dimension_px() -> u32 {
    2048
}
fn default_image_webp_quality() -> u8 {
    80
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServerSettings {
    #[serde(default = "default_addr")]
    pub addr: String,

    #[serde(default = "default_workers")]
    pub workers: usize,

    #[serde(default = "default_trust_forwarded_for")]
    pub trust_forwarded_for: bool,

    #[serde(default)]
    pub public_base_url: String,
}

impl Default for ServerSettings {
    fn default() -> Self {
        Self {
            addr: default_addr(),
            workers: default_workers(),
            trust_forwarded_for: default_trust_forwarded_for(),
            public_base_url: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LogSettings {
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

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub enum DbRole {
    Master,

    Catalog,

    Order,

    Payment,

    Log,

    Report,

    Temp,
}

impl DbRole {
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DatabaseSettings {
    #[serde(default)]
    pub sqlserver_url: String,

    #[serde(default = "default_db_pool_size")]
    pub pool_size: u32,

    #[serde(default = "default_db_connect_timeout_secs")]
    pub connect_timeout_secs: u64,

    #[serde(default = "default_migrations_dir")]
    pub migrations_dir: String,
}

impl DatabaseSettings {
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

impl DatabaseSettings {
    pub fn from_url(url: impl Into<String>) -> Self {
        Self {
            sqlserver_url: url.into(),
            ..Self::default()
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct DatabaseTopologySettings {
    #[serde(default)]
    pub catch_all: DatabaseSettings,

    #[serde(default)]
    pub master: DatabaseSettings,

    #[serde(default)]
    pub catalog: DatabaseSettings,

    #[serde(default)]
    pub order: DatabaseSettings,

    #[serde(default)]
    pub payment: DatabaseSettings,

    #[serde(default)]
    pub log: DatabaseSettings,

    #[serde(default)]
    pub report: DatabaseSettings,

    #[serde(default)]
    pub temp: DatabaseSettings,
}

impl DatabaseTopologySettings {
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

    pub fn for_role(&self, role: crate::config::DbRole) -> DatabaseSettings {
        let slot = self.slot(role);
        if !slot.sqlserver_url.trim().is_empty() {
            return slot.clone();
        }

        if !self.catch_all.sqlserver_url.trim().is_empty() {
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

    fn settings_for(&self, role: crate::config::DbRole) -> &DatabaseSettings {
        self.slot(role)
    }
}

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RedisSettings {
    #[serde(default = "default_redis_url")]
    pub url: String,

    #[serde(default = "default_redis_pool_size")]
    pub pool_size: usize,

    #[serde(default = "default_redis_namespace")]
    pub namespace: String,
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
            namespace: default_redis_namespace(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PermissionCacheSettings {
    #[serde(default = "default_permission_cache_ttl_secs")]
    pub ttl_secs: u64,
}

impl Default for PermissionCacheSettings {
    fn default() -> Self {
        Self {
            ttl_secs: default_permission_cache_ttl_secs(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NatsSettings {
    #[serde(default = "default_nats_url")]
    pub url: String,

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DataDirSettings {
    #[serde(default = "default_data_dir_path")]
    pub path: String,

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuthSettings {
    #[serde(default)]
    pub jwt_secret: String,

    #[serde(default = "default_auth_issuer")]
    pub issuer: String,

    #[serde(default = "default_access_ttl")]
    pub access_ttl_secs: i64,

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
    pub fn is_configured(&self) -> bool {
        !self.jwt_secret.is_empty()
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LogFormat {
    Json,

    Pretty,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Environment {
    #[default]
    Development,

    Production,
}

impl Environment {
    pub const fn is_production(self) -> bool {
        matches!(self, Self::Production)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TlsSettings {
    #[serde(default)]
    pub enabled: bool,

    #[serde(default)]
    pub cert_path: Option<String>,

    #[serde(default)]
    pub key_path: Option<String>,

    #[serde(default)]
    pub redirect_from_port: u16,

    #[serde(default = "default_hsts_max_age_secs")]
    pub hsts_max_age_secs: u64,

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
    pub fn cert_path_or_empty(&self) -> &str {
        self.cert_path.as_deref().unwrap_or("").trim()
    }

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
fn default_trust_forwarded_for() -> bool {
    true
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

fn default_redis_namespace() -> String {
    "kokkeak-production".into()
}

fn default_permission_cache_ttl_secs() -> u64 {
    300
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MiddlewareSettings {
    #[serde(default, deserialize_with = "deserialize_comma_list")]
    pub cors_allow_origins: Vec<String>,

    #[serde(default = "default_request_timeout_secs")]
    pub request_timeout_secs: u64,

    #[serde(default = "default_compression_enabled")]
    pub compression_enabled: bool,

    #[serde(default)]
    pub rate_limit: RateLimitSettings,

    #[serde(default = "default_request_body_limit_bytes")]
    pub request_body_limit_bytes: usize,

    #[serde(default = "default_max_concurrency")]
    pub max_concurrency: usize,

    #[serde(default)]
    pub idempotency: IdempotencySettings,

    #[serde(default)]
    pub features: FeatureFlagSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FeatureFlagSettings {
    #[serde(default = "default_flag_enabled")]
    pub auth: bool,

    #[serde(default = "default_flag_enabled")]
    pub orders: bool,

    #[serde(default = "default_flag_enabled")]
    pub payments: bool,

    #[serde(default = "default_flag_enabled")]
    pub chat: bool,

    #[serde(default = "default_flag_enabled")]
    pub admin: bool,
}

impl Default for FeatureFlagSettings {
    fn default() -> Self {
        Self {
            auth: default_flag_enabled(),
            orders: default_flag_enabled(),
            payments: default_flag_enabled(),
            chat: default_flag_enabled(),
            admin: default_flag_enabled(),
        }
    }
}

fn default_flag_enabled() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IdempotencySettings {
    #[serde(default)]
    pub enabled: bool,

    #[serde(default = "default_idempotency_ttl_secs")]
    pub ttl_secs: u64,

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
    86_400
}

fn default_idempotency_max_entries() -> usize {
    10_000
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum RateLimitBackend {
    #[default]
    Memory,

    Redis,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RateLimitSettings {
    #[serde(default)]
    pub enabled: bool,

    #[serde(default)]
    pub backend: RateLimitBackend,

    #[serde(default = "default_rate_per_second")]
    pub requests_per_second: u32,

    #[serde(default = "default_rate_burst_size")]
    pub burst_size: u32,
}

impl Default for RateLimitSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            backend: RateLimitBackend::default(),
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
            request_body_limit_bytes: default_request_body_limit_bytes(),
            max_concurrency: default_max_concurrency(),
            idempotency: IdempotencySettings::default(),
            features: FeatureFlagSettings::default(),
        }
    }
}

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

fn default_request_body_limit_bytes() -> usize {
    16 * 1024 * 1024
}

fn default_max_concurrency() -> usize {
    512
}

impl Settings {
    pub fn load() -> Result<Self, ConfigError> {
        load_env_file();
        let figment = Figment::new()
            .merge(Toml::file("config.toml").nested())
            .merge(Env::prefixed("KOKKAK_").split("__"));
        let settings: Settings = figment.extract()?;
        settings.validate()?;
        Ok(settings)
    }

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

        if self.environment.is_production() {
            match self.server.addr.parse::<std::net::SocketAddr>() {
                Ok(addr) if addr.ip().is_loopback() => {}
                Ok(_) => {
                    return Err(ConfigError::Invalid {
                        key: "KOKKAK_SERVER__ADDR".into(),
                        message: format!(
                            "must be a loopback address (127.0.0.1:<port> or [::1]:<port>) \
                             when KOKKAK_ENVIRONMENT=production (got {:?})",
                            self.server.addr
                        ),
                    });
                }
                Err(e) => {
                    return Err(ConfigError::Invalid {
                        key: "KOKKAK_SERVER__ADDR".into(),
                        message: format!("invalid socket address {:?}: {e}", self.server.addr),
                    });
                }
            }
        }

        if self.environment.is_production() && self.middleware.cors_allow_origins.is_empty() {
            return Err(ConfigError::Invalid {
                key: "KOKKAK_MIDDLEWARE__CORS_ALLOW_ORIGINS".into(),
                message: "must list at least one origin when KOKKAK_ENVIRONMENT=production".into(),
            });
        }

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

            if self.middleware.rate_limit.backend == RateLimitBackend::Redis
                && !self.redis.is_configured()
            {
                return Err(ConfigError::Invalid {
                    key: "KOKKAK_MIDDLEWARE__RATE_LIMIT__BACKEND".into(),
                    message: "rate_limit.backend=redis requires KOKKAK_REDIS__URL to be configured"
                        .into(),
                });
            }
        }

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

        if self.middleware.request_body_limit_bytes == 0 {
            return Err(ConfigError::Invalid {
                key: "KOKKAK_MIDDLEWARE__REQUEST_BODY_LIMIT_BYTES".into(),
                message: "must be >= 1 (use usize::MAX to effectively disable)".into(),
            });
        }
        if self.middleware.max_concurrency == 0 {
            return Err(ConfigError::Invalid {
                key: "KOKKAK_MIDDLEWARE__MAX_CONCURRENCY".into(),
                message: "must be >= 1 (use usize::MAX to effectively disable)".into(),
            });
        }

        if self.environment.is_production()
            && self.server.public_base_url.trim().is_empty()
            && self.storage.adapter_kind() != StorageAdapterKind::Memory
        {
            return Err(ConfigError::Invalid {
                key: "KOKKAK_SERVER__PUBLIC_BASE_URL".into(),
                message: "must be set when KOKKAK_ENVIRONMENT=production and a persistent \
                          storage adapter is wired (S3 or Local FS)"
                    .into(),
            });
        }

        if self.environment.is_production() {
            self.storage.signed_url_knobs_valid()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

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
            "KOKKAK_TLS__AUTO_RELOAD",
            "KOKKAK_MIDDLEWARE__CORS_ALLOW_ORIGINS",
            "KOKKAK_MIDDLEWARE__REQUEST_TIMEOUT_SECS",
            "KOKKAK_MIDDLEWARE__COMPRESSION_ENABLED",
            "KOKKAK_MIDDLEWARE__RATE_LIMIT__ENABLED",
            "KOKKAK_MIDDLEWARE__RATE_LIMIT__REQUESTS_PER_SECOND",
            "KOKKAK_MIDDLEWARE__RATE_LIMIT__BURST_SIZE",
            "KOKKAK_MIDDLEWARE__REQUEST_BODY_LIMIT_BYTES",
            "KOKKAK_MIDDLEWARE__MAX_CONCURRENCY",
            "KOKKAK_MIDDLEWARE__IDEMPOTENCY__ENABLED",
            "KOKKAK_MIDDLEWARE__IDEMPOTENCY__TTL_SECS",
            "KOKKAK_MIDDLEWARE__IDEMPOTENCY__MAX_ENTRIES",
            "KOKKAK_SERVER__PUBLIC_BASE_URL",
            "KOKKAK_STORAGE__SIGNED_URL_SECRET",
            "KOKKAK_STORAGE__SIGNED_URL_TTL_SECS",
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
                trust_forwarded_for: true,
                public_base_url: String::new(),
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
                trust_forwarded_for: true,
                public_base_url: String::new(),
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
                trust_forwarded_for: true,
                public_base_url: String::new(),
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

    #[test]
    fn tls_default_is_disabled() {
        let t = TlsSettings::default();
        assert!(!t.enabled);
        assert_eq!(t.cert_path, None);
        assert_eq!(t.key_path, None);
        assert_eq!(t.redirect_from_port, 0);
        assert_eq!(t.hsts_max_age_secs, 0);

        assert!(!t.auto_reload);
    }

    #[test]
    fn tls_auto_reload_load_from_env_overrides() {
        let _guard = ENV_LOCK.lock().expect("mutex poisoned");
        clear_kokkak_env();
        std::env::set_var("KOKKAK_TLS__AUTO_RELOAD", "true");
        let s = Settings::load().expect("load should succeed");
        assert!(s.tls.auto_reload);
        clear_kokkak_env();
    }

    #[test]
    fn tls_default_settings_validate() {
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

        std::env::set_var("KOKKAK_TLS__ENABLED", "true");
        std::env::set_var("KOKKAK_TLS__CERT_PATH", "/etc/kokkak/cert.pem");
        std::env::set_var("KOKKAK_TLS__KEY_PATH", "/etc/kokkak/key.pem");
        std::env::set_var("KOKKAK_TLS__REDIRECT_FROM_PORT", "80");
        std::env::set_var("KOKKAK_TLS__HSTS_MAX_AGE_SECS", "31536000");
        std::env::set_var("KOKKAK_ENVIRONMENT", "production");

        std::env::set_var("KOKKAK_SERVER__ADDR", "127.0.0.1:8443");

        std::env::set_var(
            "KOKKAK_MIDDLEWARE__CORS_ALLOW_ORIGINS",
            "https://app.example.com",
        );

        std::env::set_var(
            "KOKKAK_STORAGE__SIGNED_URL_SECRET",
            "test-secret-with-at-least-32-bytes-yes",
        );

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

    #[test]
    fn production_with_public_bind_addr_fails_validation() {
        let s = Settings {
            environment: Environment::Production,
            server: ServerSettings {
                addr: "0.0.0.0:18080".into(),
                ..ServerSettings::default()
            },
            middleware: MiddlewareSettings {
                cors_allow_origins: vec!["https://app.example.com".into()],
                ..MiddlewareSettings::default()
            },
            ..Settings::default()
        };
        let err = s
            .validate()
            .expect_err("production + public bind must be rejected");
        assert!(
            err.to_string().contains("KOKKAK_SERVER__ADDR") && err.to_string().contains("loopback"),
            "error should point at bind_addr + loopback, got: {err}"
        );
    }

    #[test]
    fn production_with_loopback_bind_and_tls_off_validates() {
        let s = Settings {
            environment: Environment::Production,
            server: ServerSettings {
                addr: "127.0.0.1:18080".into(),
                ..ServerSettings::default()
            },
            storage: StorageSettings {
                signed_url_secret: "test-secret-with-at-least-32-bytes-yes".into(),
                ..StorageSettings::default()
            },
            middleware: MiddlewareSettings {
                cors_allow_origins: vec!["https://app.example.com".into()],
                ..MiddlewareSettings::default()
            },
            ..Settings::default()
        };
        assert!(s.validate().is_ok(), "got: {:?}", s.validate());
    }

    #[test]
    fn production_with_ipv6_loopback_bind_validates() {
        let s = Settings {
            environment: Environment::Production,
            server: ServerSettings {
                addr: "[::1]:18080".into(),
                ..ServerSettings::default()
            },
            storage: StorageSettings {
                signed_url_secret: "test-secret-with-at-least-32-bytes-yes".into(),
                ..StorageSettings::default()
            },
            middleware: MiddlewareSettings {
                cors_allow_origins: vec!["https://app.example.com".into()],
                ..MiddlewareSettings::default()
            },
            ..Settings::default()
        };
        assert!(s.validate().is_ok(), "got: {:?}", s.validate());
    }

    #[test]
    fn production_with_malformed_bind_addr_fails_validation() {
        let s = Settings {
            environment: Environment::Production,
            server: ServerSettings {
                addr: "not-a-socket-addr".into(),
                ..ServerSettings::default()
            },
            middleware: MiddlewareSettings {
                cors_allow_origins: vec!["https://app.example.com".into()],
                ..MiddlewareSettings::default()
            },
            ..Settings::default()
        };
        let err = s
            .validate()
            .expect_err("non-parseable bind addr must be rejected");
        assert!(
            err.to_string().contains("KOKKAK_SERVER__ADDR"),
            "error should mention bind addr, got: {err}"
        );
    }

    #[test]
    fn production_with_tls_enabled_but_missing_key_still_fails() {
        let s = Settings {
            environment: Environment::Production,
            server: ServerSettings {
                addr: "127.0.0.1:18080".into(),
                ..ServerSettings::default()
            },
            tls: TlsSettings {
                enabled: true,
                cert_path: Some("/etc/kokkak/cert.pem".into()),
                key_path: None,
                redirect_from_port: 0,
                hsts_max_age_secs: 0,
                auto_reload: false,
            },
            middleware: MiddlewareSettings {
                cors_allow_origins: vec!["https://app.example.com".into()],
                ..MiddlewareSettings::default()
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
        let s = Settings::default();
        assert_eq!(s.environment, Environment::Development);
        assert!(!s.tls.enabled);
        assert!(s.validate().is_ok());
    }

    #[test]
    fn trust_forwarded_for_defaults_to_true() {
        let s = Settings::default();
        assert!(s.server.trust_forwarded_for);
    }

    #[test]
    fn t23_public_base_url_empty_defaults_ok() {
        let s = Settings::default();

        assert_eq!(s.server.public_base_url, "");
        assert!(s.validate().is_ok());
    }

    #[test]
    fn t23_public_base_url_set_in_production_with_local_storage_validates() {
        let s = Settings {
            environment: Environment::Production,
            server: ServerSettings {
                addr: "127.0.0.1:18080".into(),
                public_base_url: "https://api.sdplao.com".into(),
                ..ServerSettings::default()
            },
            storage: StorageSettings {
                local_path: "/var/kokkak/uploads".into(),
                signed_url_secret: "test-secret-with-at-least-32-bytes-yes".into(),
                ..StorageSettings::default()
            },
            middleware: MiddlewareSettings {
                cors_allow_origins: vec!["https://www.sdplao.com".into()],
                ..MiddlewareSettings::default()
            },
            ..Settings::default()
        };
        assert!(s.validate().is_ok());
    }

    #[test]
    fn t23b_signed_url_secret_too_short_fails() {
        let mut s = StorageSettings {
            local_path: "/var/kokkak/uploads".into(),
            ..StorageSettings::default()
        };
        s.signed_url_secret = "too-short".into();
        let err = s.signed_url_knobs_valid().expect_err("short secret fails");
        assert!(
            err.to_string().contains("32 bytes"),
            "error should mention 32-byte minimum, got: {err}"
        );
    }

    #[test]
    fn t23b_signed_url_ttl_zero_fails() {
        let mut s = StorageSettings::default();
        s.signed_url_secret = "this-is-a-thirty-two-byte-test-secret-x".into();
        s.signed_url_ttl_secs = 0;
        let err = s.signed_url_knobs_valid().expect_err("ttl=0 fails");
        assert!(
            err.to_string().contains("SIGNED_URL_TTL_SECS"),
            "error should name the knob, got: {err}"
        );
    }

    #[test]
    fn t23b_signed_url_ttl_too_long_fails() {
        let mut s = StorageSettings::default();
        s.signed_url_secret = "this-is-a-thirty-two-byte-test-secret-x".into();
        s.signed_url_ttl_secs = 86400;
        let err = s.signed_url_knobs_valid().expect_err("ttl>1h fails");
        assert!(
            err.to_string().contains("between 60 and 3600"),
            "error should bound ttl, got: {err}"
        );
    }

    #[test]
    fn t23b_signed_url_knobs_happy() {
        let mut s = StorageSettings::default();
        s.signed_url_secret = "this-is-a-thirty-two-byte-test-secret-x".into();
        s.signed_url_ttl_secs = 600;
        s.signed_url_knobs_valid()
            .expect("32-byte secret + 600s ttl must validate");
    }

    #[test]
    fn t23_public_base_url_empty_in_production_with_local_storage_fails() {
        let s = Settings {
            environment: Environment::Production,
            server: ServerSettings {
                addr: "127.0.0.1:18080".into(),

                ..ServerSettings::default()
            },
            storage: StorageSettings {
                local_path: "/var/kokkak/uploads".into(),
                ..StorageSettings::default()
            },
            middleware: MiddlewareSettings {
                cors_allow_origins: vec!["https://www.sdplao.com".into()],
                ..MiddlewareSettings::default()
            },
            ..Settings::default()
        };
        let err = s
            .validate()
            .expect_err("production+Local+empty base URL must fail");
        assert!(
            err.to_string().contains("PUBLIC_BASE_URL"),
            "error should name the knob, got: {err}"
        );
    }

    #[test]
    fn t23_public_base_url_empty_with_memory_storage_is_allowed() {
        let s = Settings {
            environment: Environment::Production,
            server: ServerSettings {
                addr: "127.0.0.1:18080".into(),
                ..ServerSettings::default()
            },
            storage: StorageSettings {
                signed_url_secret: "test-secret-with-at-least-32-bytes-yes".into(),
                ..StorageSettings::default()
            },
            middleware: MiddlewareSettings {
                cors_allow_origins: vec!["https://www.sdplao.com".into()],
                ..MiddlewareSettings::default()
            },
            ..Settings::default()
        };

        assert!(s.validate().is_ok());
    }

    #[test]
    fn middleware_default_is_production_safe() {
        let s = Settings::default();
        assert!(
            s.middleware.cors_allow_origins.is_empty(),
            "default CORS allowlist must be empty (deny all)"
        );
        assert_eq!(s.middleware.request_timeout_secs, 30);
        assert!(s.middleware.compression_enabled);

        assert_eq!(s.middleware.request_body_limit_bytes, 16 * 1024 * 1024);
        assert_eq!(s.middleware.max_concurrency, 512);
        assert!(s.validate().is_ok());
    }

    #[test]
    fn t16_safety_knobs_default_positive() {
        let s = MiddlewareSettings::default();
        assert!(s.request_body_limit_bytes >= 1024);
        assert!(s.max_concurrency >= 1);
    }

    #[test]
    fn t16_zero_body_limit_fails_validation() {
        let s = Settings {
            middleware: MiddlewareSettings {
                request_body_limit_bytes: 0,
                ..MiddlewareSettings::default()
            },
            ..Settings::default()
        };
        let err = s.validate().expect_err("zero body limit must be rejected");
        assert!(err.to_string().contains("REQUEST_BODY_LIMIT_BYTES"));
    }

    #[test]
    fn t16_zero_concurrency_fails_validation() {
        let s = Settings {
            middleware: MiddlewareSettings {
                max_concurrency: 0,
                ..MiddlewareSettings::default()
            },
            ..Settings::default()
        };
        let err = s.validate().expect_err("zero concurrency must be rejected");
        assert!(err.to_string().contains("MAX_CONCURRENCY"));
    }

    #[test]
    fn middleware_load_from_env_overrides() {
        let _guard = ENV_LOCK.lock().expect("mutex poisoned");
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
        let s = Settings {
            environment: Environment::Production,

            server: ServerSettings {
                addr: "127.0.0.1:8443".into(),
                ..ServerSettings::default()
            },
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

            server: ServerSettings {
                addr: "127.0.0.1:8443".into(),
                ..ServerSettings::default()
            },

            tls: TlsSettings {
                enabled: true,
                cert_path: Some("/etc/kokkak/cert.pem".into()),
                key_path: Some("/etc/kokkak/key.pem".into()),
                redirect_from_port: 80,
                hsts_max_age_secs: 31_536_000,
                auto_reload: false,
            },
            storage: StorageSettings {
                signed_url_secret: "test-secret-with-at-least-32-bytes-yes".into(),
                ..StorageSettings::default()
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
    fn rate_limit_default_is_disabled() {
        let s = Settings::default();
        assert!(!s.middleware.rate_limit.enabled);
        assert_eq!(s.middleware.rate_limit.requests_per_second, 100);
        assert_eq!(s.middleware.rate_limit.burst_size, 200);
        assert!(s.validate().is_ok());
    }

    #[test]
    fn rate_limit_load_from_env_overrides() {
        let _guard = ENV_LOCK.lock().expect("mutex poisoned");
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
                    backend: RateLimitBackend::Memory,
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
                    backend: RateLimitBackend::Memory,
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
        let s = Settings {
            middleware: MiddlewareSettings {
                rate_limit: RateLimitSettings {
                    enabled: false,
                    backend: RateLimitBackend::Memory,
                    requests_per_second: 0,
                    burst_size: 0,
                },
                ..MiddlewareSettings::default()
            },
            ..Settings::default()
        };
        assert!(s.validate().is_ok());
    }

    #[test]
    fn rate_limit_backend_redis_without_redis_url_fails() {
        let s = Settings {
            middleware: MiddlewareSettings {
                rate_limit: RateLimitSettings {
                    enabled: true,
                    backend: RateLimitBackend::Redis,
                    requests_per_second: 100,
                    burst_size: 200,
                },
                ..MiddlewareSettings::default()
            },

            ..Settings::default()
        };
        let err = s
            .validate()
            .expect_err("backend=redis without KOKKAK_REDIS__URL must be rejected");
        assert!(
            err.to_string()
                .contains("KOKKAK_MIDDLEWARE__RATE_LIMIT__BACKEND"),
            "error should point at the backend knob, got: {err}"
        );
        assert!(
            err.to_string().contains("KOKKAK_REDIS__URL"),
            "error should hint at the missing redis URL, got: {err}"
        );
    }

    #[test]
    fn rate_limit_backend_redis_with_redis_url_validates() {
        let mut s = Settings::default();
        s.redis.url = "redis://127.0.0.1:6379".into();
        s.middleware.rate_limit = RateLimitSettings {
            enabled: true,
            backend: RateLimitBackend::Redis,
            requests_per_second: 100,
            burst_size: 200,
        };
        assert!(s.validate().is_ok());
    }

    #[test]
    fn rate_limit_backend_default_is_memory() {
        assert_eq!(RateLimitBackend::default(), RateLimitBackend::Memory);
        let s = Settings::default();
        assert_eq!(s.middleware.rate_limit.backend, RateLimitBackend::Memory);
    }

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
        let _guard = ENV_LOCK.lock().expect("mutex poisoned");
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
