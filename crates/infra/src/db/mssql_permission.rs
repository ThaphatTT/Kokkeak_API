//! SQL Server-backed permission repository (M15-prep).
//!
//! Calls the inline TVF `dbo.FN_SECURITY_USER_HAS_PERMISSION` directly —
//! it encapsulates the role + override + deny-wins logic. **TVFs must be
//! invoked via `SELECT col FROM fn(...)`**, never `EXEC` (SQL Server
//! raises error 2809 otherwise — "is a table valued function object").
//!
//! ponytail: thin pass-through. Ceiling: when batch-checks land
//! (one user, N codes per request), add `effective_permissions(user_guid)`
//! that returns a resultset + groups on the Rust side.

use thiserror::Error;
use tiberius::ToSql;
use uuid::Uuid;

use kokkak_domain::{Permission, RepoError};

use crate::db::mssql::{exec_sp, MssqlPool};

/// Repository-side error. Always maps to [`RepoError::Backend`] on
/// the way out — the distinction between "DB pool exhausted" vs
/// "TVF returned a column I didn't expect" matters only for logging.
#[derive(Debug, Error)]
pub enum MssqlPermissionError {
    /// Catch-all for pool / connection / TVF / column-mapping failures.
    /// The wrapped string is the underlying error message verbatim.
    #[error("mssql permission repository: {0}")]
    Backend(String),
}

impl From<MssqlPermissionError> for RepoError {
    fn from(e: MssqlPermissionError) -> Self {
        RepoError::Backend(e.to_string())
    }
}

/// SQL Server repository for runtime permission checks (M15-prep).
///
/// Wraps the `[dbo].[API_PERMISSION_HAS_PERMISSION]` TVF. `pool = None`
/// flips this into a fail-secure stub (every call returns
/// `MssqlPermissionError::Backend("disabled")`) so tests + dev builds
/// ship without a real SQL Server.
#[derive(Clone)]
pub struct MssqlPermissionRepository {
    /// `None` → every call returns `Backend("disabled")` (fail-secure).
    /// Used by tests + dev builds that ship without a SQL Server URL.
    pool: Option<MssqlPool>,
}

impl MssqlPermissionRepository {
    /// Build a repository backed by `pool`.
    pub fn new(pool: MssqlPool) -> Self {
        Self { pool: Some(pool) }
    }

    /// Stub for tests / unconfigured environments — every call
    /// returns `Backend("disabled")`. [`PermissionChecker`] maps
    /// this to `PermissionError::Unavailable`, which handlers treat
    /// as `Denied` (fail-secure).
    pub fn disabled() -> Self {
        Self { pool: None }
    }

    /// Resolve a single `(user_guid, code)` pair against the DB.
    ///
    /// Returns `Ok(true)` if the user has the permission,
    /// `Ok(false)` otherwise. A DB error returns
    /// `MssqlPermissionError::Backend`.
    pub async fn has_permission(
        &self,
        user_guid: Uuid,
        code: Permission,
    ) -> Result<bool, MssqlPermissionError> {
        let pool = self
            .pool
            .as_ref()
            .ok_or_else(|| MssqlPermissionError::Backend("disabled (no sqlserver pool)".into()))?;

        // AGENTS.md §7.4 + KG fact `rule: Always Uuid::to_string()→String
        // before bind as &dyn ToSql in exec_sp`. The TVF accepts varchar(36)
        // for both params (per DBA convention), so bind as String.
        let user_guid_str = user_guid.to_string();
        let code_str = code.code();
        let rows = exec_sp(
            pool,
            r#"
                SELECT is_allowed
                FROM dbo.FN_SECURITY_USER_HAS_PERMISSION(@P1, @P2)
            "#,
            &[&user_guid_str as &dyn ToSql, &code_str as &dyn ToSql],
        )
        .await
        .map_err(|e| MssqlPermissionError::Backend(e.to_string()))?;

        let row = rows.first().ok_or_else(|| {
            MssqlPermissionError::Backend("FN_SECURITY_USER_HAS_PERMISSION returned no row".into())
        })?;

        // BIT column. tiberius surfaces BIT directly as `bool`
        // (NOT i32 — see `tiberius::ColumnType::Bit`). Read as bool
        // via `try_get::<bool, _>()` and let the failure bubble up
        // if a future SP migration changes the type.
        let is_allowed = row
            .try_get::<bool, _>("is_allowed")
            .map_err(|e| {
                MssqlPermissionError::Backend(format!("failed to read `is_allowed` as bool: {e}"))
            })?
            .ok_or_else(|| {
                MssqlPermissionError::Backend(
                    "FN_SECURITY_USER_HAS_PERMISSION row missing `is_allowed`".into(),
                )
            })?;

        Ok(is_allowed)
    }
}

#[cfg(test)]
mod tests {
    //! Unit tests would require a live SQL Server + seeded
    //! user/role/permission data. They live in
    //! `crates/infra/tests/integration_permission_sqlserver.rs`
    //! (opt-in via `KOKKAK_DATABASE__SQLSERVER_URL`).

    #[test]
    fn disabled_repo_always_returns_backend_error() {
        let repo = super::MssqlPermissionRepository::disabled();
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(
            repo.has_permission(uuid::Uuid::new_v4(), kokkak_domain::Permission::UsersCreate),
        );
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("disabled"));
    }
}
