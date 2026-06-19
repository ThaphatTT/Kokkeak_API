//! `HealthCheck` for Redis (เช็คสถานะ Redis).

use std::sync::Arc;

use async_trait::async_trait;
use kokkak_domain::{HealthCheck, HealthError};

use crate::cache::redis::RedisCache;

/// `HealthCheck` that sends a Redis `PING`.
pub struct RedisHealthCheck {
    cache: Arc<RedisCache>,
}

impl RedisHealthCheck {
    /// Wrap an existing cache client.
    pub fn new(cache: Arc<RedisCache>) -> Self {
        Self { cache }
    }
}

#[async_trait]
impl HealthCheck for RedisHealthCheck {
    fn name(&self) -> &str {
        "redis"
    }

    async fn check(&self) -> Result<(), HealthError> {
        let conn = self
            .cache
            .conn()
            .await
            .map_err(|e| HealthError::Failed(e.to_string()))?;
        ping_from_conn(conn)
            .await
            .map_err(|e| HealthError::Failed(e.to_string()))
    }
}

/// Send a `PING` to an already-acquired Redis connection.
async fn ping_from_conn(
    mut conn: deadpool_redis::Connection,
) -> Result<(), crate::cache::redis::RedisCacheError> {
    let _: String = redis::cmd("PING").query_async(&mut conn).await?;
    Ok(())
}
