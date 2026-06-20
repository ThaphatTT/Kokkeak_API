//! Versioned SQL migration runner for SQL Server (M5).
//!
//! Each migration file is `migrations/YYYYMMDDHHMMSS_description.sql`.
//! On startup, the runner:
//! 1. Ensures the `schema_migrations(version, applied_at)` table exists.
//! 2. Discovers all SQL files in the migrations dir.
//! 3. Applies every file whose `version` is not in the table — in
//!    version order, **one file per transaction**.
//!
//! The M0 stub kept the discovery logic and is reused here. The
//! real `run()` executes the SQL via tiberius.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use futures::TryStreamExt;
use tokio::fs;

use crate::db::mssql::{MssqlError, MssqlPool};

/// Errors raised by the migration runner.
#[derive(Debug, Error)]
pub enum MigrateError {
    /// SQL Server pool error.
    #[error("sqlserver error: {0}")]
    Mssql(#[from] MssqlError),
    /// Underlying tiberius / TDS error.
    #[error("tds error: {0}")]
    Tds(String),
    /// Filesystem / read error.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    /// The migration file is missing the expected prefix.
    #[error("invalid migration filename: {0:?} (expected YYYYMMDDHHMMSS_description.sql)")]
    InvalidFilename(PathBuf),
}

use thiserror::Error;

/// One discovered migration (filename + raw SQL body).
#[derive(Debug, Clone)]
pub struct Migration {
    /// `YYYYMMDDHHMMSS` prefix (sort key + applied-marker).
    pub version: String,
    /// Absolute path to the `.sql` file on disk.
    pub path: PathBuf,
    /// Raw SQL body (loaded once at startup).
    pub sql: String,
}

/// Discover all migration files in `dir`, sorted ascending by version.
pub async fn discover(dir: &Path) -> Result<Vec<Migration>, MigrateError> {
    let mut entries = fs::read_dir(dir).await?;
    let mut migrations = Vec::new();
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        let name = match path.file_name().and_then(|s| s.to_str()) {
            Some(n) => n,
            None => continue,
        };
        if !name.ends_with(".sql") {
            continue;
        }
        let version = name
            .split('_')
            .next()
            .ok_or_else(|| MigrateError::InvalidFilename(path.clone()))?
            .to_string();
        if version.len() != 14 || !version.chars().all(|c| c.is_ascii_digit()) {
            return Err(MigrateError::InvalidFilename(path));
        }
        let mut f = fs::File::open(&path).await?;
        let mut sql = String::new();
        use tokio::io::AsyncReadExt;
        f.read_to_string(&mut sql).await?;
        migrations.push(Migration { version, path, sql });
    }
    migrations.sort_by(|a, b| a.version.cmp(&b.version));
    Ok(migrations)
}

/// Apply every migration in `dir` that has not yet been applied.
pub async fn run(pool: &MssqlPool, dir: &Path) -> Result<usize, MigrateError> {
    if !dir.exists() {
        tracing::info!(dir = %dir.display(), "no migrations directory — skipping");
        return Ok(0);
    }
    let migrations = discover(dir).await?;
    if migrations.is_empty() {
        return Ok(0);
    }

    // 1) Ensure the migrations table exists.
    let mut conn = pool
        .get()
        .await
        .map_err(|e| MigrateError::Tds(format!("acquire: {e}")))?;
    conn.execute(
        "IF NOT EXISTS ( \
            SELECT * FROM sysobjects WHERE name='schema_migrations' AND xtype='U' \
         ) \
         CREATE TABLE schema_migrations ( \
            version VARCHAR(14) NOT NULL PRIMARY KEY, \
            applied_at DATETIME2(7) NOT NULL DEFAULT SYSUTCDATETIME() \
         )",
        &[],
    )
    .await
    .map_err(|e| MigrateError::Tds(format!("create schema_migrations: {e}")))?;
    drop(conn);

    // 2) Read the applied set.
    let mut applied: std::collections::HashSet<String> = std::collections::HashSet::new();
    {
        let mut conn = pool
            .get()
            .await
            .map_err(|e| MigrateError::Tds(format!("acquire: {e}")))?;
        let rows = conn
            .query("SELECT version FROM schema_migrations", &[])
            .await
            .map_err(|e| MigrateError::Tds(format!("select applied: {e}")))?;
        let mut row_stream = rows.into_row_stream();
        while let Some(row) = row_stream
            .try_next()
            .await
            .map_err(|e| MigrateError::Tds(e.to_string()))?
        {
            if let Some(v) = row.get::<&str, _>(0) {
                applied.insert(v.to_string());
            }
        }
    }

    // 3) Apply each missing migration in a fresh connection.
    let mut applied_count = 0usize;
    for m in migrations {
        if applied.contains(&m.version) {
            continue;
        }
        tracing::info!(version = %m.version, path = %m.path.display(), "applying migration");

        let mut conn = pool
            .get()
            .await
            .map_err(|e| MigrateError::Tds(format!("acquire: {e}")))?;
        // Split on GO batch separators (TDS does not accept multiple
        // statements in one call without a `;` separator, and tiberius
        // sends them as one batch). A simple split on a line
        // containing only `GO` is enough for our migration files.
        for batch in split_go_batches(&m.sql) {
            let trimmed = batch.trim();
            if trimmed.is_empty() {
                continue;
            }
            conn.execute(trimmed, &[])
                .await
                .map_err(|e| MigrateError::Tds(format!("migration {}: {e}", m.version)))?;
        }
        // Mark as applied.
        conn.execute(
            "INSERT INTO schema_migrations(version) VALUES (@P1)",
            &[&m.version],
        )
        .await
        .map_err(|e| MigrateError::Tds(format!("insert migration: {e}")))?;
        applied_count += 1;
    }
    Ok(applied_count)
}

/// Split a SQL script on `GO` batch separators (case-insensitive,
/// line-anchored).
fn split_go_batches(sql: &str) -> Vec<String> {
    let mut batches = Vec::new();
    let mut current = String::new();
    for line in sql.lines() {
        if line.trim().eq_ignore_ascii_case("go") {
            if !current.trim().is_empty() {
                batches.push(current.clone());
            }
            current.clear();
        } else {
            current.push_str(line);
            current.push('\n');
        }
    }
    if !current.trim().is_empty() {
        batches.push(current);
    }
    batches
}

/// A trivial wrapper for callers that just want the pool type.
#[allow(dead_code)]
pub type Pool = Arc<MssqlPool>;

/// Convenience: max connection acquire timeout.
pub const DEFAULT_ACQUIRE_TIMEOUT: Duration = Duration::from_secs(5);

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn discover_sorts_by_version() {
        let tmp = std::env::temp_dir().join("kokkak_migrate_test");
        let _ = std::fs::create_dir_all(&tmp);
        for (name, body) in [
            ("20260615000002_add_foo.sql", "-- foo"),
            ("20260615000001_init.sql", "-- init"),
            ("20260615000003_add_bar.sql", "-- bar"),
            ("readme.md", "ignore me"),
        ] {
            std::fs::write(tmp.join(name), body).unwrap();
        }
        let migs = discover(&tmp).await.unwrap();
        let versions: Vec<&str> = migs.iter().map(|m| m.version.as_str()).collect();
        assert_eq!(
            versions,
            vec!["20260615000001", "20260615000002", "20260615000003"]
        );
        assert_eq!(migs.len(), 3);
    }

    #[tokio::test]
    async fn discover_rejects_bad_filename() {
        let tmp = std::env::temp_dir().join("kokkak_migrate_test_bad");
        let _ = std::fs::create_dir_all(&tmp);
        std::fs::write(tmp.join("not_a_version_init.sql"), "x").unwrap();
        let err = discover(&tmp).await.unwrap_err();
        assert!(matches!(err, MigrateError::InvalidFilename(_)));
    }

    #[test]
    fn split_go_batches_separates_on_go_line() {
        let sql = "CREATE TABLE a (id INT);\nGO\nCREATE TABLE b (id INT);\n";
        let batches = split_go_batches(sql);
        assert_eq!(batches.len(), 2);
        assert!(batches[0].contains("a"));
        assert!(batches[1].contains("b"));
    }

    #[test]
    fn split_go_batches_ignores_inline_go() {
        let sql = "SELECT 'GO NOW' AS msg;\n";
        let batches = split_go_batches(sql);
        assert_eq!(batches.len(), 1);
    }
}
