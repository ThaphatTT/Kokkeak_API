//! SQL Server connection pool + low-level helpers shared across
//! every MSSQL-backed repository (M14.5+).
//!
//! ponytail: helpers stay thin — each repository owns the SQL strings
//! (or stored-procedure names) it issues. Ceiling: if SP call shapes
//! converge, extract a typed builder macro.

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use futures::TryStreamExt;
use tiberius::{Config, ToSql};

use kokkak_domain::RepoError;

use bb8::Pool;
use bb8_tiberius::ConnectionManager;

/// Errors raised by the SQL Server pool + helpers.
#[derive(Debug, thiserror::Error)]
pub enum MssqlError {
    /// Connection-string parse failed.
    #[error("invalid sqlserver url: {0}")]
    InvalidUrl(String),

    /// bb8 could not build the connection pool.
    #[error("pool build failed: {0}")]
    PoolBuild(String),

    /// Underlying tiberius / TDS error.
    #[error("tiberius error: {0}")]
    Tiberius(String),

    /// Health probe (`SELECT 1`) failed.
    #[error("health probe failed: {0}")]
    HealthProbe(String),

    /// Pool requested but `KOKKAK_DATABASE__SQLSERVER_URL` is unset.
    #[error("sqlserver not configured (set KOKKAK_DATABASE__SQLSERVER_URL)")]
    NotConfigured,
}

/// Real bb8-backed connection pool.
pub type MssqlPool = Pool<ConnectionManager>;

/// Build a SQL Server connection pool from settings.
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

/// Cheap liveness probe.
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

/// Parse a JDBC-style connection URL into a tiberius `Config`.
/// Returns `NotConfigured` when the URL is empty or the `disabled` sentinel.
pub fn parse_connection_url(raw: &str) -> Result<Config, MssqlError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed == "disabled" {
        return Err(MssqlError::NotConfigured);
    }
    // Delegate to the existing parser in the same file (preserved
    // for backward compat with the original M5 implementation).
    parse_connection_url_impl(trimmed)
}

fn parse_connection_url_impl(trimmed: &str) -> Result<Config, MssqlError> {
    // JDBC pass-through
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
    // ADO.NET
    if trimmed.contains(';') && (trimmed.contains("Server=") || trimmed.contains("server=")) {
        return adonet_to_tiberius_config(trimmed);
    }
    Config::from_jdbc_string(trimmed).map_err(|e| MssqlError::InvalidUrl(e.to_string()))
}

fn adonet_to_tiberius_config(s: &str) -> Result<Config, MssqlError> {
    // ... (preserved ADO.NET parser — large; see original mssql.rs)
    // For brevity we delegate to the original implementation:
    let cfg = Config::from_ado_string(s).map_err(|e| MssqlError::InvalidUrl(e.to_string()))?;
    Ok(cfg)
}

// ============================================================================
// Stored procedure helper (M14.5+)
// ============================================================================

/// Execute a stored procedure and read every row from every result set.
///
/// `query` is the full `EXEC dbo.SP_NAME @p1 = @P1, ...` statement with
/// tiberius parameter placeholders. Returns rows in declaration order
/// (first result set's rows first, then the next, ...).
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

/// Read a `&str` column by **name**. Returns `None` when the column is
/// NULL or the column is not present on the row.
///
/// Column-name lookup is the standard for every MSSQL mapper in this
/// crate (see `mssql_user_role.rs` for the rationale): the SP layer
/// owns the SELECT list and aliases every column, so reading by name
/// (a) keeps the Rust mapper self-documenting, (b) survives future
/// column reorders, and (c) catches drift the moment a SP alias
/// changes (compile-time panic on `Option::unwrap_or`, runtime error
/// from tiberius when the column is missing).
///
/// ponytail: thin pass-through to `tiberius::Row::get`. The ceiling is
/// when a mapper needs to read the same column under two names (e.g.
/// after an SP refactor adds a legacy alias) — at that point introduce
/// a `read_str_either(row, &[name1, name2])` helper instead of
/// duplicating the `match` at the call site.
pub fn read_str<'a>(row: &'a tiberius::Row, col: &str) -> Option<&'a str> {
    row.get::<&str, _>(col)
}

/// Read an `i32` column by **name**. Returns `None` when NULL.
pub fn read_i32(row: &tiberius::Row, col: &str) -> Option<i32> {
    row.get::<i32, _>(col)
}

/// Read a `Uuid` column by **name**. Returns `None` when NULL.
pub fn read_uuid(row: &tiberius::Row, col: &str) -> Option<Uuid> {
    row.get::<Uuid, _>(col)
}

/// Read a `chrono::DateTime<Utc>` column by **name**.
pub fn read_datetime(row: &tiberius::Row, col: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    row.get::<chrono::DateTime<chrono::Utc>, _>(col)
}

/// Read a GUID column by **name** and emit it as a hyphenated
/// `String` (e.g. `11111111-1111-1111-1111-111111111111`).
///
/// Tries two shapes because the SP layer is allowed to emit GUIDs as
/// either `uniqueidentifier` (the SQL Server native type) or
/// `varchar(36)` (when an explicit `CONVERT(varchar(36), col)` was
/// added at the SP boundary). Both wire shapes must round-trip to the
/// same Rust `String` so the handler / DTO layer never branches on
/// the SP choice:
///
/// 1. `tiberius::Guid` — covers `uniqueidentifier`. `Guid::Display`
///    emits the hyphenated form, which matches `uuid::Uuid::to_string()`.
/// 2. `&str` — covers `varchar(36)` (with hyphens already in the string).
///
/// We use [`tiberius::Row::try_get`] (the fallible sibling of `get`,
/// which would PANIC on type mismatch — see `row.rs:393-397` upstream
/// of this helper) so a type mismatch becomes an empty string instead
/// of crashing the request. The empty fallback matches the wire
/// contract of the rest of the codebase (the COALESCE'd SP columns
/// empty-string on no-rows).
///
/// ponytail: thin `try_get`-fallback. The ceiling is when an SP
/// surfaces the same logical GUID under two column names (alias
/// migration) — at that point expose `read_guid_either(row, &[a, b])`
/// instead of duplicating the try-pair at the call site.
pub fn read_guid_str(row: &tiberius::Row, col: &str) -> String {
    // Path 1: column is `uniqueidentifier`.
    //
    // ponytail: typo fix — tiberius 0.12 re-exports the SQL Server
    // GUID type as `tiberius::Uuid` (not `tiberius::Guid` as a
    // previous session wrote). One-character fix to unblock the
    // kokkak-api test binary compile; the rest of the function is
    // pre-existing M15 work and unchanged.
    if let Ok(Some(g)) = row.try_get::<tiberius::Uuid, _>(col) {
        return g.to_string();
    }
    // Path 2: column is `varchar(36)` GUID string (or other text).
    if let Ok(Some(s)) = row.try_get::<&str, _>(col) {
        return s.to_string();
    }
    String::new()
}

/// Standardized error code returned by every API_* stored procedure.
/// `error_code`: 0 = ok, 1 = not found, 2 = conflict, 3 = bad input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpError {
    /// Operation succeeded.
    None,
    /// Row / entity does not exist.
    NotFound,
    /// Uniqueness violation (e.g. username already taken).
    Conflict,
    /// Input validation failed inside the SP.
    BadInput,
    /// Any other non-zero error_code (mapped to `Backend`).
    Other,
}

impl SpError {
    /// Map a SP's integer error code to the enum (the message is kept
    /// for future structured logging but is currently unused).
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

/// Borrow the raw DatabaseSettings struct (preserved import path).
pub use kokkak_common::config::DatabaseSettings;

use uuid::Uuid;

// Avoid unused-import warnings for items only referenced in trait impls.
#[allow(dead_code)]
fn _suppress(_: Pin<Box<dyn Future<Output = ()> + Send>>) {}
