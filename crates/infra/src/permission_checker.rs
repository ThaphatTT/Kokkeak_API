//! Permission check service (M15-prep).
//!
//! Composes [`MssqlPermissionRepository`] (DB) with
//! [`RedisPermissionCache`] (cache). Used by handlers that need
//! fine-grained per-permission gating (instead of coarse role
//! gating).
//!
//! ponytail: thin composition. Ceiling: when N codes are checked
//! per request, switch to `effective_permissions(user_guid) ->
//! HashSet<Permission>` to avoid N round-trips. AGENTS.md M15-prep
//! backlog covers this.

use std::sync::Arc;

use thiserror::Error;
use uuid::Uuid;

use kokkak_domain::Permission;

use crate::cache::permission_cache::{PermissionCacheError, RedisPermissionCache};
use crate::db::mssql_permission::{MssqlPermissionError, MssqlPermissionRepository};

/// Errors surfaced by [`PermissionChecker`]. Handlers map both variants
/// to a denied response (fail-secure per AGENTS.md §21.2).
#[derive(Debug, Error)]
pub enum PermissionError {
    /// Either the DB TVF failed or the cache layer raised a non-fatal
    /// error that bubbled up. The wrapped string carries the underlying
    /// message verbatim (logged at WARN level before this is returned).
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

/// Cache + DB composition for runtime permission checks.
///
/// Resolution order in `has_permission`:
/// 1. Cache GET. Hit → return.
/// 2. Cache MISS → DB call (TVF via SP wrapper).
/// 3. SET cache with result (best-effort).
///
/// Failure semantics:
/// - Cache error → log WARN, fall through to DB.
/// - DB error → return `PermissionError::Unavailable` (fail-secure;
///   handlers map this to `Denied`).
pub struct PermissionChecker {
    repo: Arc<MssqlPermissionRepository>,
    cache: Arc<RedisPermissionCache>,
}

impl PermissionChecker {
    /// Wire the DB repo + Redis cache together.
    pub fn new(repo: Arc<MssqlPermissionRepository>, cache: Arc<RedisPermissionCache>) -> Self {
        Self { repo, cache }
    }

    /// Resolve whether `user_guid` holds the given permission `code`.
    ///
    /// Cache-first (read-through): HIT → return; MISS → DB TVF → SET cache.
    /// Cache errors are fail-open (log + continue to DB); DB errors are
    /// fail-secure (return [`PermissionError::Unavailable`]).
    pub async fn has_permission(
        &self,
        user_guid: Uuid,
        code: Permission,
    ) -> Result<bool, PermissionError> {
        // 1. Cache GET (best-effort).
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
                // Fail-open cache: log + continue to DB.
                tracing::warn!(
                    user_guid = %user_guid,
                    code = code.code(),
                    error = %e,
                    "permission_checker: cache error — falling through to DB"
                );
            }
        }

        // 2. DB call.
        let allowed = self.repo.has_permission(user_guid, code).await?;

        // 3. SET cache (best-effort; cache errors don't fail the check).
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

    /// Drop every cached permission entry for `user_guid`.
    /// Called from the admin permission-update flow (M15) after a
    /// role/permission/override change.
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
        // Fail-secure: with no DB pool, has_permission must NOT
        // accidentally return Ok(false) — it must surface Unavailable
        // so the handler maps to Denied (admin endpoints 403).
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
