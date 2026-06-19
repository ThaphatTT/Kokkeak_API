//! SQL Server client (M5 + M12 multi-DB).
//!
//! Wraps `tiberius` (TDS driver) + `bb8-tiberius` (connection pool).
//! Implements [`MssqlPool`] which exposes the underlying `bb8::Pool`
//! for direct access by the repository layer.
//!
//! ## Connection-string formats (M12)
//!
//! [`build_pool`] accepts three on-the-wire shapes, all driven by the
//! same `KOKKAK_DATABASE__SQLSERVER_URL` env var (or the per-DB
//! variants `KOKKAK_DATABASE__MASTER_URL`, `KOKKAK_DATABASE__ORDER_URL`,
//! ...). The parser is forgiving — it normalises to a JDBC-style
//! string and hands off to `tiberius::Config::from_jdbc_string`.
//!
//! | Form | Example | Origin |
//! |------|---------|--------|
//! | **ADO.NET** | `Server=10.0.200.83;Database=Kokak_DB;user id=sa;pwd=secret;TrustServerCertificate=True` | ASP.NET legacy (`appsettings.json` / web.config) |
//! | **URL** | `mssql://sa:secret@host:1433/DB?encrypt=true` | environment-friendly shorthand |
//! | **JDBC** | `jdbc:sqlserver://host:1433;database=DB;user=sa;password=secret` | what `tiberius` natively parses |
//!
//! The ADO.NET form tolerates whitespace around `=` (`user id = sa`)
//! and case-insensitive key names (`Server`, `server`, `SERVER`).
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
/// `settings.sqlserver_url` may be in ADO.NET, `mssql://`, `sqlserver://`
/// or `jdbc:sqlserver://` form. Empty / `"disabled"` is treated as
/// unconfigured.
pub async fn build_pool(settings: &DatabaseSettings) -> Result<MssqlPool, MssqlError> {
    if !settings.is_configured() {
        return Err(MssqlError::NotConfigured);
    }
    let config = parse_connection_url(&settings.sqlserver_url)?;
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

/// Parse any supported connection-string form into a `tiberius::Config`.
///
/// This is the single entry-point shared by [`build_pool`] and the
/// per-DB topology builder. M12 used to support 3 prefixes; it now
/// also handles the **ADO.NET** key=value form used by the legacy
/// ASP.NET backend.
pub fn parse_connection_url(raw: &str) -> Result<Config, MssqlError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed == "disabled" {
        return Err(MssqlError::NotConfigured);
    }

    // 1) JDBC: hand to tiberius directly.
    if let Some(rest) = trimmed.strip_prefix("jdbc:sqlserver://") {
        let jdbc = format!("jdbc:sqlserver://{rest}");
        return Config::from_jdbc_string(&jdbc).map_err(|e| MssqlError::InvalidUrl(e.to_string()));
    }

    // 2) URL-style: rewrite mssql:// / sqlserver:// → jdbc:sqlserver://
    //    The tiberius JDBC parser does NOT understand `user:pass@host`.
    //    We strip the userinfo + path/query and pass `host:port` only,
    //    then re-attach user/password/database/... as key=value pairs.
    if let Some(rest) = trimmed
        .strip_prefix("mssql://")
        .or_else(|| trimmed.strip_prefix("sqlserver://"))
    {
        let jdbc = url_form_to_jdbc(rest)?;
        return Config::from_jdbc_string(&jdbc).map_err(|e| MssqlError::InvalidUrl(e.to_string()));
    }

    // 3) ADO.NET: `key=value;key=value;...` — the legacy form.
    //    No scheme present and at least one `=` separator.
    if !trimmed.contains("://") && trimmed.contains('=') {
        let jdbc = ado_net_to_jdbc(trimmed)?;
        return Config::from_jdbc_string(&jdbc).map_err(|e| MssqlError::InvalidUrl(e.to_string()));
    }

    Err(MssqlError::InvalidUrl(format!(
        "unrecognised connection-string format: {trimmed:.40}"
    )))
}

/// Translate a `mssql://user:pass@host:port/db?query` URL to JDBC.
///
/// Splits the authority into host[:port] + userinfo, the path into
/// the database name, and the query into key=value pairs. We use a
/// hand-rolled split (not `url::Url`) to keep this module dep-free.
fn url_form_to_jdbc(rest: &str) -> Result<String, MssqlError> {
    // Split off query: `...?key=value&key=value`
    let (path_and_auth, query) = match rest.split_once('?') {
        Some((p, q)) => (p, Some(q)),
        None => (rest, None),
    };
    // Split off userinfo: `user:pass@host:port/db`
    let (userinfo, host_path) = match path_and_auth.rsplit_once('@') {
        Some((u, h)) => (Some(u), h),
        None => (None, path_and_auth),
    };
    // Split off path: `host:port` / `db`
    let (host_port, path) = match host_path.split_once('/') {
        Some((h, p)) => (h, Some(p)),
        None => (host_path, None),
    };
    if host_port.is_empty() {
        return Err(MssqlError::InvalidUrl("missing host".into()));
    }

    let (user, password) = match userinfo {
        Some(u) => match u.split_once(':') {
            Some((user, pw)) => (Some(user), Some(pw)),
            None => (Some(u), None),
        },
        None => (None, None),
    };

    let mut parts: Vec<String> = Vec::with_capacity(6);
    parts.push(format!("jdbc:sqlserver://{host_port}"));
    if let Some(db) = path {
        if !db.is_empty() {
            parts.push(format!("database={db}"));
        }
    }
    if let Some(u) = user {
        parts.push(format!("user={u}"));
    }
    if let Some(p) = password {
        parts.push(format!("password={p}"));
    }
    if let Some(q) = query {
        for kv in q.split('&') {
            if kv.is_empty() {
                continue;
            }
            // Translate common query keys.
            let (k, v) = match kv.split_once('=') {
                Some((k, v)) => (k, v),
                None => continue,
            };
            let key = k.to_ascii_lowercase();
            let val = v;
            let translated = match key.as_str() {
                "encrypt" => format!("encrypt={val}"),
                "trustcert" | "trustservercertificate" => {
                    format!("trustServerCertificate={val}")
                }
                _ => format!("{k}={v}"),
            };
            parts.push(translated);
        }
    }
    Ok(parts.join(";"))
}

/// Translate an ADO.NET / `SqlConnection`-style string to JDBC.
///
/// Supported keys (case-insensitive, whitespace around `=` ignored):
///
/// | ADO.NET key                  | JDBC target                       |
/// |------------------------------|-----------------------------------|
/// | `Server` / `Data Source`     | host[:port]                       |
/// | `Database` / `Initial Catalog` | database                        |
/// | `User ID` / `UID` / `user id` | user                             |
/// | `Password` / `Pwd`           | password                          |
/// | `Encrypt`                    | encrypt=true / false              |
/// | `TrustServerCertificate`     | trustServerCertificate=true / false|
/// | `Trusted_Connection`         | `integratedSecurity=true` if True |
///
/// Unknown keys are passed through verbatim to JDBC (tiberius may
/// understand them; if not, the user sees a clear error at
/// `Config::from_jdbc_string` time, not at parse time).
fn ado_net_to_jdbc(raw: &str) -> Result<String, MssqlError> {
    let mut host: Option<String> = None;
    let mut database: Option<String> = None;
    let mut user: Option<String> = None;
    let mut password: Option<String> = None;
    let mut extras: Vec<String> = Vec::new();

    for segment in raw.split(';') {
        let seg = segment.trim();
        if seg.is_empty() {
            continue;
        }
        let Some((k, v)) = seg.split_once('=') else {
            // Not a key=value segment — pass through to JDBC as-is so
            // tiberius can complain with a precise error.
            extras.push(seg.to_string());
            continue;
        };
        let key = k.trim().to_ascii_lowercase();
        let val = v.trim();

        match key.as_str() {
            "server" | "data source" | "addr" | "address" => {
                host = Some(val.to_string());
            }
            "database" | "initial catalog" => {
                database = Some(val.to_string());
            }
            "user id" | "uid" | "user" => {
                user = Some(val.to_string());
            }
            "password" | "pwd" => {
                password = Some(val.to_string());
            }
            "encrypt" => {
                extras.push(format!("encrypt={val}"));
            }
            "trustservercertificate" => {
                extras.push(format!("trustServerCertificate={val}"));
            }
            "trusted_connection" => {
                if val.eq_ignore_ascii_case("true") || val == "1" {
                    extras.push("integratedSecurity=true".to_string());
                }
            }
            // Connection Timeout / Connect Timeout → tiberius uses
            // the bb8 acquisition timeout, but pass through for
            // tiberius login timeout.
            "connection timeout" | "connect timeout" | "timeout" => {
                extras.push(format!("loginTimeout={val}"));
            }
            // Application Name / App — useful in SQL Server profiler.
            "application name" | "app" => {
                extras.push(format!("applicationName={val}"));
            }
            // Anything else: pass through verbatim. JDBC is more
            // permissive than tiberius' strict parser.
            _ => {
                extras.push(format!("{key}={val}"));
            }
        }
    }

    let host = host.ok_or_else(|| MssqlError::InvalidUrl("missing Server".into()))?;
    // JDBC form: jdbc:sqlserver://host;key=value;key=value
    let mut parts: Vec<String> = Vec::with_capacity(4 + extras.len());
    // `host` may carry a port: `Server=10.0.200.83,1433` (ADO.NET
    // uses comma). tiberius' jdbc parser wants a colon.
    let host = host.replace(',', ":");
    parts.push(format!("jdbc:sqlserver://{host}"));
    if let Some(db) = database {
        parts.push(format!("database={db}"));
    }
    if let Some(u) = user {
        parts.push(format!("user={u}"));
    }
    if let Some(p) = password {
        parts.push(format!("password={p}"));
    }
    parts.extend(extras);
    Ok(parts.join(";"))
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

    // ---------------- parse_connection_url ----------------

    #[test]
    fn parse_jdbc_passes_through() {
        let c = parse_connection_url(
            "jdbc:sqlserver://localhost:1433;database=KOKKAK;user=sa;password=secret",
        )
        .expect("jdbc parses");
        assert_eq!(c.get_addr(), "localhost:1433");
    }

    #[test]
    fn parse_mssql_prefix_rewrites_to_jdbc() {
        let c = parse_connection_url("mssql://sa:secret@localhost:1433/KOKKAK")
            .expect("mssql:// parses");
        assert_eq!(c.get_addr(), "localhost:1433");
    }

    #[test]
    fn parse_sqlserver_prefix_rewrites_to_jdbc() {
        let c = parse_connection_url("sqlserver://sa:secret@localhost:1433/KOKKAK")
            .expect("sqlserver:// parses");
        assert_eq!(c.get_addr(), "localhost:1433");
    }

    #[test]
    fn parse_mssql_url_with_query() {
        let c = parse_connection_url(
            "mssql://sa:secret@localhost:1433/KOKKAK?encrypt=true&TrustServerCertificate=true",
        )
        .expect("mssql with query parses");
        assert_eq!(c.get_addr(), "localhost:1433");
    }

    #[test]
    fn parse_mssql_url_no_userinfo() {
        // Bare host:port, no user/pass.
        let c = parse_connection_url("mssql://localhost:1433/KOKKAK")
            .expect("mssql no userinfo parses");
        assert_eq!(c.get_addr(), "localhost:1433");
    }

    #[test]
    fn url_form_to_jdbc_round_trip() {
        let jdbc = url_form_to_jdbc("sa:secret@localhost:1433/KOKKAK").unwrap();
        assert_eq!(
            jdbc,
            "jdbc:sqlserver://localhost:1433;database=KOKKAK;user=sa;password=secret"
        );
    }

    #[test]
    fn url_form_to_jdbc_with_query() {
        let jdbc = url_form_to_jdbc(
            "sa:secret@localhost:1433/KOKKAK?encrypt=true&TrustServerCertificate=true",
        )
        .unwrap();
        assert!(jdbc.contains("encrypt=true"));
        assert!(jdbc.contains("trustServerCertificate=true"));
    }

    #[test]
    fn url_form_to_jdbc_missing_host_fails() {
        let err = url_form_to_jdbc("/KOKKAK").unwrap_err();
        assert!(matches!(err, MssqlError::InvalidUrl(_)));
    }

    #[test]
    fn parse_ado_net_minimal() {
        let c = parse_connection_url("Server=localhost;Database=KOKKAK;User Id=sa;Password=secret")
            .expect("ado.net minimal parses");
        assert_eq!(c.get_addr(), "localhost:1433");
    }

    #[test]
    fn parse_ado_net_with_spaces_around_equals() {
        // The user's legacy form has `user id =sa` style.
        let c = parse_connection_url(
            "Server=10.0.200.83;Database=Kokak_DB;user id =sa; pwd=123456;Trusted_Connection=False;TrustServerCertificate=True",
        )
        .expect("legacy form parses");
        assert_eq!(c.get_addr(), "10.0.200.83:1433");
    }

    #[test]
    fn parse_ado_net_with_comma_port() {
        // ADO.NET uses comma: `Server=host,1433`
        let c = parse_connection_url(
            "Server=db.example.com,1500;Database=App;User Id=app;Password=p;TrustServerCertificate=true",
        )
        .expect("comma port parses");
        assert_eq!(c.get_addr(), "db.example.com:1500");
    }

    #[test]
    fn parse_ado_net_with_integrated_security() {
        // `Trusted_Connection=True` → integratedSecurity=true
        let c = parse_connection_url("Server=localhost;Database=KOKKAK;Trusted_Connection=True")
            .expect("trusted_connection parses");
        assert!(!c.get_addr().is_empty());
    }

    #[test]
    fn parse_ado_net_missing_server_fails() {
        let err = parse_connection_url("Database=KOKKAK;User Id=sa;Password=p").unwrap_err();
        assert!(matches!(err, MssqlError::InvalidUrl(_)));
    }

    #[test]
    fn parse_empty_returns_not_configured() {
        let err = parse_connection_url("").unwrap_err();
        assert!(matches!(err, MssqlError::NotConfigured));
    }

    #[test]
    fn parse_disabled_sentinel_returns_not_configured() {
        let err = parse_connection_url("disabled").unwrap_err();
        assert!(matches!(err, MssqlError::NotConfigured));
    }

    #[test]
    fn parse_garbage_returns_invalid_url() {
        let err = parse_connection_url("not a connection string").unwrap_err();
        assert!(matches!(err, MssqlError::InvalidUrl(_)));
    }

    // ---------------- ado_net_to_jdbc unit ----------------

    #[test]
    fn ado_net_to_jdbc_round_trip_user_pasted_form() {
        let jdbc = ado_net_to_jdbc(
            "Server=10.0.200.83;Database=Kokak_DB;user id =sa; pwd=123456;Trusted_Connection=False;TrustServerCertificate=True",
        )
        .unwrap();
        assert!(jdbc.starts_with("jdbc:sqlserver://10.0.200.83"));
        assert!(jdbc.contains("database=Kokak_DB"));
        assert!(jdbc.contains("user=sa"));
        assert!(jdbc.contains("password=123456"));
        assert!(jdbc.contains("trustServerCertificate=True"));
        // Trusted_Connection=False is a no-op (only emits when True).
        assert!(!jdbc.contains("integratedSecurity"));
    }

    #[test]
    fn ado_net_to_jdbc_preserves_unknown_keys() {
        let jdbc = ado_net_to_jdbc(
            "Server=localhost;Database=K;User Id=sa;Password=p;Application Name=myapp;Foo=bar",
        )
        .unwrap();
        // pass-through keys are lowercased per match arm.
        assert!(jdbc.contains("applicationName=myapp"));
        assert!(jdbc.contains("foo=bar"));
    }

    #[test]
    fn ado_net_to_jdbc_handles_trailing_semicolons() {
        let jdbc = ado_net_to_jdbc("Server=localhost;Database=K;User Id=sa;Password=p;;").unwrap();
        assert!(jdbc.contains("database=K"));
    }

    #[test]
    fn build_config_does_not_panic() {
        let _c = build_config("localhost", 1433, "sa", "secret", "KOKKAK");
    }
}
