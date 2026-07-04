

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use futures::TryStreamExt;
use tiberius::{Config, ToSql};

use kokkak_domain::RepoError;

use bb8::Pool;
use bb8_tiberius::ConnectionManager;

#[derive(Debug, thiserror::Error)]
pub enum MssqlError {

    #[error("invalid sqlserver url: {0}")]
    InvalidUrl(String),

    #[error("pool build failed: {0}")]
    PoolBuild(String),

    #[error("tiberius error: {0}")]
    Tiberius(String),

    #[error("health probe failed: {0}")]
    HealthProbe(String),

    #[error("sqlserver not configured (set KOKKAK_DATABASE__SQLSERVER_URL)")]
    NotConfigured,
}

pub type MssqlPool = Pool<ConnectionManager>;

pub async fn build_pool(settings: &DatabaseSettings) -> Result<MssqlPool, MssqlError> {
    if !settings.is_configured() {
        return Err(MssqlError::NotConfigured);
    }
    let config = parse_connection_url(&settings.sqlserver_url)?;
    let manager = ConnectionManager::new(config);
    let pool: MssqlPool = Pool::builder()
        .max_size(settings.pool_size)
        .connection_timeout(Duration::from_secs(settings.connect_timeout_secs))
        .build(manager)
        .await
        .map_err(|e| MssqlError::PoolBuild(e.to_string()))?;
    Ok(pool)
}

pub async fn ping(pool: &MssqlPool) -> Result<(), MssqlError> {
    let mut conn = pool
        .get()
        .await
        .map_err(|e| MssqlError::HealthProbe(format!("acquire: {e}")))?;
    conn.query("SELECT 1", &[])
        .await
        .map_err(|e| MssqlError::HealthProbe(e.to_string()))?
        .into_row()
        .await
        .map_err(|e| MssqlError::HealthProbe(e.to_string()))?;
    Ok(())
}

pub fn parse_connection_url(raw: &str) -> Result<Config, MssqlError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed == "disabled" {
        return Err(MssqlError::NotConfigured);
    }

    parse_connection_url_impl(trimmed)
}

fn parse_connection_url_impl(trimmed: &str) -> Result<Config, MssqlError> {

    if let Some(rest) = trimmed.strip_prefix("jdbc:sqlserver://") {
        let jdbc = format!("jdbc:sqlserver://{rest}");
        return Config::from_jdbc_string(&jdbc).map_err(|e| MssqlError::InvalidUrl(e.to_string()));
    }
    if let Some(rest) = trimmed.strip_prefix("mssql://") {
        let jdbc = format!("jdbc:sqlserver://{rest}");
        return Config::from_jdbc_string(&jdbc).map_err(|e| MssqlError::InvalidUrl(e.to_string()));
    }
    if let Some(rest) = trimmed.strip_prefix("sqlserver://") {
        let jdbc = format!("jdbc:sqlserver://{rest}");
        return Config::from_jdbc_string(&jdbc).map_err(|e| MssqlError::InvalidUrl(e.to_string()));
    }

    if trimmed.contains(';') && (trimmed.contains("Server=") || trimmed.contains("server=")) {
        return adonet_to_tiberius_config(trimmed);
    }
    Config::from_jdbc_string(trimmed).map_err(|e| MssqlError::InvalidUrl(e.to_string()))
}

fn adonet_to_tiberius_config(s: &str) -> Result<Config, MssqlError> {

    let cfg = Config::from_ado_string(s).map_err(|e| MssqlError::InvalidUrl(e.to_string()))?;
    Ok(cfg)
}

pub async fn exec_sp(
    pool: &MssqlPool,
    query: &str,
    params: &[&dyn ToSql],
) -> Result<Vec<tiberius::Row>, RepoError> {
    let mut conn = pool
        .get()
        .await
        .map_err(|e| RepoError::Backend(format!("acquire: {e}")))?;
    let rows = conn
        .query(query, params)
        .await
        .map_err(|e| RepoError::Backend(e.to_string()))?;
    let mut stream = rows.into_row_stream();
    let mut out = Vec::new();
    while let Some(row) = stream
        .try_next()
        .await
        .map_err(|e| RepoError::Backend(e.to_string()))?
    {
        out.push(row);
    }
    Ok(out)
}

pub fn read_str<'a>(row: &'a tiberius::Row, col: &str) -> Option<&'a str> {
    row.get::<&str, _>(col)
}

pub fn read_i32(row: &tiberius::Row, col: &str) -> Option<i32> {
    row.get::<i32, _>(col)
}

pub fn read_uuid(row: &tiberius::Row, col: &str) -> Option<Uuid> {
    row.get::<Uuid, _>(col)
}

pub fn read_datetime(row: &tiberius::Row, col: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    row.get::<chrono::DateTime<chrono::Utc>, _>(col)
}

pub fn read_decimal(row: &tiberius::Row, col: &str) -> Option<rust_decimal::Decimal> {
    row.get::<rust_decimal::Decimal, _>(col)
}

pub fn read_guid_str(row: &tiberius::Row, col: &str) -> String {

    if let Ok(Some(g)) = row.try_get::<tiberius::Uuid, _>(col) {
        return g.to_string();
    }

    if let Ok(Some(s)) = row.try_get::<&str, _>(col) {
        return s.to_string();
    }
    String::new()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpError {

    None,

    NotFound,

    Conflict,

    BadInput,

    Other,
}

impl SpError {

    pub fn from_code(code: i32, _msg: &str) -> Self {
        match code {
            0 => Self::None,
            1 => Self::NotFound,
            2 => Self::Conflict,
            3 => Self::BadInput,
            _ => Self::Other,
        }
    }
}

pub use kokkak_common::config::DatabaseSettings;

use uuid::Uuid;

#[allow(dead_code)]
fn _suppress(_: Pin<Box<dyn Future<Output = ()> + Send>>) {}
