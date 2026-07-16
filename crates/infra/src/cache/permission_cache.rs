use deadpool_redis::Pool;
use thiserror::Error;
use uuid::Uuid;

use kokkak_domain::Permission;

#[derive(Debug, Error)]
pub enum PermissionCacheError {
    #[error("redis pool: {0}")]
    Pool(#[from] deadpool_redis::PoolError),

    #[error("redis: {0}")]
    Redis(#[from] redis::RedisError),

    #[error("permission cache disabled (no redis pool)")]
    Disabled,
}

#[derive(Clone)]
pub struct RedisPermissionCache {
    pool: Option<Pool>,
    ttl_secs: u64,
    namespace: String,
}

impl RedisPermissionCache {
    pub fn new(pool: Pool, ttl_secs: u64, namespace: String) -> Self {
        Self {
            pool: Some(pool),
            ttl_secs,
            namespace,
        }
    }

    pub fn disabled(ttl_secs: u64) -> Self {
        Self {
            pool: None,
            ttl_secs,
            namespace: "kokkeak-production".into(),
        }
    }

    fn key(&self, user_guid: Uuid, code: Permission) -> String {
        format!("{}:perm:{}:{}", self.namespace, user_guid, code.code())
    }

    fn pattern_for_user(&self, user_guid: Uuid) -> String {
        format!("{}:perm:{}:*", self.namespace, user_guid)
    }

    pub async fn get(
        &self,
        user_guid: Uuid,
        code: Permission,
    ) -> Result<Option<bool>, PermissionCacheError> {
        let pool = self.pool.as_ref().ok_or(PermissionCacheError::Disabled)?;
        let mut conn = pool.get().await?;
        let v: Option<String> = redis::cmd("GET")
            .arg(self.key(user_guid, code))
            .query_async(&mut *conn)
            .await?;
        Ok(v.map(|s| s == "1"))
    }

    pub async fn set(
        &self,
        user_guid: Uuid,
        code: Permission,
        value: bool,
    ) -> Result<(), PermissionCacheError> {
        let pool = self.pool.as_ref().ok_or(PermissionCacheError::Disabled)?;
        let mut conn = pool.get().await?;
        let _: () = redis::cmd("SET")
            .arg(self.key(user_guid, code))
            .arg(if value { "1" } else { "0" })
            .arg("EX")
            .arg(self.ttl_secs)
            .query_async(&mut *conn)
            .await?;
        Ok(())
    }

    pub async fn invalidate_user(&self, user_guid: Uuid) -> Result<u64, PermissionCacheError> {
        let pool = self.pool.as_ref().ok_or(PermissionCacheError::Disabled)?;
        let mut conn = pool.get().await?;
        let pattern = self.pattern_for_user(user_guid);
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
