

use std::sync::Arc;

use thiserror::Error;
use uuid::Uuid;

use kokkak_domain::Permission;

use crate::cache::permission_cache::{PermissionCacheError, RedisPermissionCache};
use crate::db::mssql_permission::{MssqlPermissionError, MssqlPermissionRepository};

#[derive(Debug, Error)]
pub enum PermissionError {

    #[error("permission check unavailable: {0}")]
    Unavailable(String),
}

impl From<MssqlPermissionError> for PermissionError {
    fn from(e: MssqlPermissionError) -> Self {
        PermissionError::Unavailable(e.to_string())
    }
}

impl From<PermissionCacheError> for PermissionError {
    fn from(e: PermissionCacheError) -> Self {
        PermissionError::Unavailable(e.to_string())
    }
}

pub struct PermissionChecker {
    repo: Arc<MssqlPermissionRepository>,
    cache: Arc<RedisPermissionCache>,
}

impl PermissionChecker {

    pub fn new(repo: Arc<MssqlPermissionRepository>, cache: Arc<RedisPermissionCache>) -> Self {
        Self { repo, cache }
    }

    pub async fn has_permission(
        &self,
        user_guid: Uuid,
        code: Permission,
    ) -> Result<bool, PermissionError> {

        match self.cache.get(user_guid, code).await {
            Ok(Some(v)) => {
                tracing::debug!(
                    user_guid = %user_guid,
                    code = code.code(),
                    result = v,
                    "permission_checker: cache hit"
                );
                return Ok(v);
            }
            Ok(None) => {
                tracing::debug!(
                    user_guid = %user_guid,
                    code = code.code(),
                    "permission_checker: cache miss"
                );
            }
            Err(e) => {

                tracing::warn!(
                    user_guid = %user_guid,
                    code = code.code(),
                    error = %e,
                    "permission_checker: cache error — falling through to DB"
                );
            }
        }

        let allowed = self.repo.has_permission(user_guid, code).await?;

        if let Err(e) = self.cache.set(user_guid, code, allowed).await {
            tracing::warn!(
                user_guid = %user_guid,
                code = code.code(),
                error = %e,
                "permission_checker: cache set failed (non-fatal)"
            );
        }

        Ok(allowed)
    }

    pub async fn invalidate_user(&self, user_guid: Uuid) -> Result<u64, PermissionError> {
        Ok(self.cache.invalidate_user(user_guid).await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::permission_cache::RedisPermissionCache;
    use crate::db::mssql_permission::MssqlPermissionRepository;
    use kokkak_domain::Permission;
    use uuid::Uuid;

    #[tokio::test]
    async fn db_unavailable_maps_to_unavailable_error() {

        let repo = Arc::new(MssqlPermissionRepository::disabled());
        let cache = Arc::new(RedisPermissionCache::disabled(300));
        let checker = PermissionChecker::new(repo, cache);

        let result = checker
            .has_permission(Uuid::new_v4(), Permission::UsersCreate)
            .await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            PermissionError::Unavailable(_)
        ));
    }
}
