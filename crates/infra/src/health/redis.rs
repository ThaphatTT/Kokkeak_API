

use std::sync::Arc;

use async_trait::async_trait;
use kokkak_domain::{HealthCheck, HealthError};

use crate::cache::redis::RedisCache;

pub struct RedisHealthCheck {
    cache: Arc<RedisCache>,
}

impl RedisHealthCheck {

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

async fn ping_from_conn(
    mut conn: deadpool_redis::Connection,
) -> Result<(), crate::cache::redis::RedisCacheError> {
    let _: String = redis::cmd("PING").query_async(&mut conn).await?;
    Ok(())
}
