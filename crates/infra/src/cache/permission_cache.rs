//! Redis cache for permission checks (M15-prep).
//!
//! Keys: `kokkak:v1:perm:{user_guid}:{code}` — matches the AGENTS.md
//! §9.2 `kokkak:{ver}:{domain}:{entity}:{id}:{variant}` shape.
//!
//! Values: `"1"` (allowed) / `"0"` (denied). Plain ASCII, no JSON —
//! the cache is read-mostly and the wire cost matters.
//!
//! Invalidation: SCAN + DEL on `kokkak:v1:perm:{user_guid}:*` —
//! **never** `KEYS` (blocks Redis). Called from the admin
//! permission-update flow after a role/permission/override change.
//!
//! ponytail: thin wrapper. Ceiling: at >1000 active users the
//! per-invalidate SCAN becomes noticeable — switch to a per-user
//! SET (`kokkak:v1:perm_keys:{user_guid}` → set of cache keys) that
//! invalidation reads via SMEMBERS + DEL pipeline.

use deadpool_redis::Pool;
use thiserror::Error;
use uuid::Uuid;

use kokkak_domain::Permission;

/// Errors raised by [`RedisPermissionCache`]. Always maps to the
/// checker's [`crate::permission_checker::PermissionError::Unavailable`]
/// at the layer above — the distinction between pool / command / disabled
/// only matters for logging.
#[derive(Debug, Error)]
pub enum PermissionCacheError {
    /// Underlying deadpool-redis pool acquisition / connection failure.
    #[error("redis pool: {0}")]
    Pool(#[from] deadpool_redis::PoolError),
    /// Underlying redis-rs command error (GET / SET / SCAN / DEL).
    #[error("redis: {0}")]
    Redis(#[from] redis::RedisError),
    /// Cache is disabled (no Redis pool configured) — every call short-circuits
    /// to this. [`crate::permission_checker::PermissionChecker`] treats it as
    /// a cache miss and falls through to the DB.
    #[error("permission cache disabled (no redis pool)")]
    Disabled,
}

/// Redis-backed permission cache (M15-prep).
///
/// Stores `kokkak:v1:perm:{user_guid}:{code}` → `"1"` / `"0"` (allow / deny)
/// with TTL = `ttl_secs`. `pool = None` flips this into a fail-open stub
/// (every call returns [`PermissionCacheError::Disabled`]).
#[derive(Clone)]
pub struct RedisPermissionCache {
    /// `None` → every call returns `PermissionCacheError::Disabled`.
    /// [`PermissionChecker`] treats this as a cache miss/error and
    /// falls through to the DB (fail-open per AGENTS.md §9.3 group C).
    pool: Option<Pool>,
    ttl_secs: u64,
}

impl RedisPermissionCache {
    /// Build a cache backed by `pool` with the given `ttl_secs`.
    pub fn new(pool: Pool, ttl_secs: u64) -> Self {
        Self {
            pool: Some(pool),
            ttl_secs,
        }
    }

    /// Stub for tests / unconfigured environments — every call
    /// returns `PermissionCacheError::Disabled`. The
    /// [`PermissionChecker`] fail-open behaviour means this
    /// gracefully degrades to "always check the DB".
    pub fn disabled(ttl_secs: u64) -> Self {
        Self {
            pool: None,
            ttl_secs,
        }
    }

    fn key(user_guid: Uuid, code: Permission) -> String {
        format!("kokkak:v1:perm:{user_guid}:{}", code.code())
    }

    fn pattern_for_user(user_guid: Uuid) -> String {
        format!("kokkak:v1:perm:{user_guid}:*")
    }

    /// Returns `Some(true)` if cached allow, `Some(false)` if cached
    /// deny, `None` if the key is absent (or cache is disabled).
    pub async fn get(
        &self,
        user_guid: Uuid,
        code: Permission,
    ) -> Result<Option<bool>, PermissionCacheError> {
        let pool = self.pool.as_ref().ok_or(PermissionCacheError::Disabled)?;
        let mut conn = pool.get().await?;
        let v: Option<String> = redis::cmd("GET")
            .arg(Self::key(user_guid, code))
            .query_async(&mut *conn)
            .await?;
        Ok(v.map(|s| s == "1"))
    }

    /// Write a single `(user_guid, code) → value` entry to Redis with TTL.
    /// Stores `"1"` for allow, `"0"` for deny — kept ASCII so the wire cost
    /// matches a tiny KV (the cache is read-mostly).
    pub async fn set(
        &self,
        user_guid: Uuid,
        code: Permission,
        value: bool,
    ) -> Result<(), PermissionCacheError> {
        let pool = self.pool.as_ref().ok_or(PermissionCacheError::Disabled)?;
        let mut conn = pool.get().await?;
        let _: () = redis::cmd("SET")
            .arg(Self::key(user_guid, code))
            .arg(if value { "1" } else { "0" })
            .arg("EX")
            .arg(self.ttl_secs)
            .query_async(&mut *conn)
            .await?;
        Ok(())
    }

    /// Drop every cached permission entry for `user_guid`.
    /// Returns the number of keys removed (best-effort, no rollback).
    pub async fn invalidate_user(&self, user_guid: Uuid) -> Result<u64, PermissionCacheError> {
        let pool = self.pool.as_ref().ok_or(PermissionCacheError::Disabled)?;
        let mut conn = pool.get().await?;
        let pattern = Self::pattern_for_user(user_guid);
        let mut cursor: u64 = 0;
        let mut deleted: u64 = 0;
        loop {
            let (next_cursor, keys): (u64, Vec<String>) = redis::cmd("SCAN")
                .arg(cursor)
                .arg("MATCH")
                .arg(&pattern)
                .arg("COUNT")
                .arg(100)
                .query_async(&mut *conn)
                .await?;
            if !keys.is_empty() {
                let removed: u64 = redis::cmd("DEL").arg(&keys).query_async(&mut *conn).await?;
                deleted += removed;
            }
            if next_cursor == 0 {
                break;
            }
            cursor = next_cursor;
        }
        Ok(deleted)
    }
}
