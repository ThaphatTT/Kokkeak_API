//! SQL Server client (M5).
//!
//! Wraps `tiberius` (TDS driver) + `bb8-tiberius` (connection pool).
//! Implements [`MssqlPool`] which exposes the underlying `bb8::Pool`
//! for direct access by the repository layer.
//!
//! See `AGENTS.md` § 7 for the multi-DB topology and pool sizing
//! rules.

use std::time::Duration;

use bb8::Pool;
use thiserror::Error;
use tiberius::{AuthMethod, Config};

use kokkak_common::config::DatabaseSettings;

use crate::db::migrate::MigrateError;

/// Errors raised by the SQL Server client.
#[derive(Debug, Error)]
pub enum MssqlError {
    #[error("invalid sqlserver url: {0}")]
    InvalidUrl(String),

    #[error("pool build failed: {0}")]
    PoolBuild(String),

    /// Underlying tiberius / TDS error.
    #[error("tiberius error: {0}")]
    Tiberius(String),

    #[error("health probe failed: {0}")]
    HealthProbe(String),

    #[error("sqlserver not configured (set KOKKAK_DATABASE__SQLSERVER_URL)")]
    NotConfigured,
}

/// Real bb8-backed connection pool (`Pool<bb8_tiberius::ConnectionManager>`).
pub type MssqlPool = Pool<bb8_tiberius::ConnectionManager>;

/// Build a SQL Server connection pool from settings.
///
/// The connection string may be:
/// - `jdbc:sqlserver://host:port;database=NAME;user=USER;password=PASS;encrypt=true;trustServerCertificate=true`
/// - `mssql://user:password@host:port/database?encrypt=true`
///
/// The JDBC form is the most flexible (full TDS option set). The
/// `mssql://` form is parsed via `tiberius::Config::from_jdbc_string`
/// after a small prefix rewrite.
pub async fn build_pool(settings: &DatabaseSettings) -> Result<MssqlPool, MssqlError> {
    if !settings.is_configured() {
        return Err(MssqlError::NotConfigured);
    }
    let url = rewrite_url(&settings.sqlserver_url);
    let config =
        Config::from_jdbc_string(&url).map_err(|e| MssqlError::InvalidUrl(e.to_string()))?;
    let manager = bb8_tiberius::ConnectionManager::new(config);
    let pool: MssqlPool = Pool::builder()
        .max_size(settings.pool_size)
        .connection_timeout(Duration::from_secs(settings.connect_timeout_secs))
        .build(manager)
        .await
        .map_err(|e| MssqlError::PoolBuild(e.to_string()))?;
    Ok(pool)
}

/// Cheap liveness probe — runs `SELECT 1` on a fresh connection.
pub async fn ping(pool: &MssqlPool) -> Result<(), MssqlError> {
    let mut conn = pool
        .get()
        .await
        .map_err(|e| MssqlError::HealthProbe(format!("acquire: {e}")))?;
    let _row = conn
        .query("SELECT 1", &[])
        .await
        .map_err(|e| MssqlError::HealthProbe(e.to_string()))?
        .into_row()
        .await
        .map_err(|e| MssqlError::HealthProbe(e.to_string()))?;
    Ok(())
}

/// Translate the `KOKKAK_DATABASE__SQLSERVER_URL` value to a JDBC
/// string tiberius understands.
fn rewrite_url(raw: &str) -> String {
    let trimmed = raw.trim();
    if let Some(rest) = trimmed.strip_prefix("mssql://") {
        // mssql://user:pass@host:port/db?query → jdbc:sqlserver://...
        format!("jdbc:sqlserver://{rest}")
    } else if let Some(rest) = trimmed.strip_prefix("sqlserver://") {
        format!("jdbc:sqlserver://{rest}")
    } else if let Some(rest) = trimmed.strip_prefix("jdbc:sqlserver://") {
        format!("jdbc:sqlserver://{rest}")
    } else {
        trimmed.to_string()
    }
}

/// Builder for `tiberius::Config` from a structured form (used in
/// tests where we don't want to parse a JDBC URL).
#[allow(dead_code)]
pub fn build_config(host: &str, port: u16, user: &str, pass: &str, db: &str) -> Config {
    let mut c = Config::new();
    c.host(host);
    c.port(port);
    c.database(db);
    c.authentication(AuthMethod::sql_server(user, pass));
    // encryption/trust-cert options are configured via JDBC URL
    // (e.g. `;encrypt=true;trustServerCertificate=true`). The
    // default is unencrypted, which is fine for dev.
    c
}

// `MssqlError -> MigrateError` is provided automatically by the
// `#[from]` attribute on `MigrateError::Mssql(MssqlError)`.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_rewrite_handles_mssql_prefix() {
        let r = rewrite_url("mssql://sa:secret@localhost:1433/KOKKAK_MASTER");
        assert!(r.starts_with("jdbc:sqlserver://"));
    }

    #[test]
    fn url_rewrite_passes_through_jdbc() {
        let r = rewrite_url("jdbc:sqlserver://host:1433;database=X;user=U;password=P");
        assert!(r.starts_with("jdbc:sqlserver://"));
    }

    #[test]
    fn url_rewrite_passes_through_sqlserver() {
        let r = rewrite_url("sqlserver://host:1433/db");
        assert!(r.starts_with("jdbc:sqlserver://"));
    }

    #[test]
    fn build_config_does_not_panic() {
        let _c = build_config("localhost", 1433, "sa", "secret", "KOKKAK");
    }
}
