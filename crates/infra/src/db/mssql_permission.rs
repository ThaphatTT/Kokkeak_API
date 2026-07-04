

use thiserror::Error;
use tiberius::ToSql;
use uuid::Uuid;

use kokkak_domain::{Permission, RepoError};

use crate::db::mssql::{exec_sp, MssqlPool};

#[derive(Debug, Error)]
pub enum MssqlPermissionError {

    #[error("mssql permission repository: {0}")]
    Backend(String),
}

impl From<MssqlPermissionError> for RepoError {
    fn from(e: MssqlPermissionError) -> Self {
        RepoError::Backend(e.to_string())
    }
}

#[derive(Clone)]
pub struct MssqlPermissionRepository {

    pool: Option<MssqlPool>,
}

impl MssqlPermissionRepository {

    pub fn new(pool: MssqlPool) -> Self {
        Self { pool: Some(pool) }
    }

    pub fn disabled() -> Self {
        Self { pool: None }
    }

    pub async fn has_permission(
        &self,
        user_guid: Uuid,
        code: Permission,
    ) -> Result<bool, MssqlPermissionError> {
        let pool = self
            .pool
            .as_ref()
            .ok_or_else(|| MssqlPermissionError::Backend("disabled (no sqlserver pool)".into()))?;

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
