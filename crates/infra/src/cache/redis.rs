use std::time::Duration;

use async_trait::async_trait;
use deadpool_redis::{Config, Pool, Runtime};
use futures::StreamExt;
use kokkak_common::config::RedisSettings;
use kokkak_domain::{Cache, CacheError, CacheKey, InvalidationStream};
use redis::AsyncCommands;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum RedisCacheError {
    #[error("redis pool build failed: {0}")]
    PoolBuild(String),

    #[error("redis error: {0}")]
    Redis(#[from] redis::RedisError),

    #[error("redis pool error: {0}")]
    Pool(#[from] deadpool_redis::PoolError),
}

impl From<RedisCacheError> for CacheError {
    fn from(err: RedisCacheError) -> Self {
        CacheError::Backend(err.to_string())
    }
}

#[derive(Clone, Debug)]
pub struct RedisCache {
    pool: Pool,
    url: String,
    namespace: String,
}

impl RedisCache {
    pub fn new(settings: &RedisSettings) -> Result<Self, RedisCacheError> {
        if !settings.is_configured() {
            return Err(RedisCacheError::PoolBuild(
                "redis not configured (set KOKKAK_REDIS__URL)".into(),
            ));
        }

        let cfg = Config::from_url(&settings.url);
        let pool = cfg
            .create_pool(Some(Runtime::Tokio1))
            .map_err(|e| RedisCacheError::PoolBuild(e.to_string()))?;

        tracing::info!(
            url = %settings.url,
            pool_size = settings.pool_size,
            namespace = %settings.namespace,
            "redis pool built"
        );

        Ok(Self {
            pool,
            url: settings.url.clone(),
            namespace: settings.namespace.clone(),
        })
    }

    pub async fn conn(&self) -> Result<deadpool_redis::Connection, RedisCacheError> {
        Ok(self.pool.get().await?)
    }

    pub fn pool(&self) -> deadpool_redis::Pool {
        self.pool.clone()
    }

    pub fn namespace(&self) -> &str {
        &self.namespace
    }

    fn prefixed_key(&self, key: &CacheKey) -> String {
        format!("{}:{}", self.namespace, key.as_str())
    }

    fn invalidate_channel(&self) -> String {
        format!("{}.cache.invalidate", self.namespace)
    }
}

#[async_trait]
impl Cache for RedisCache {
    async fn get(&self, key: &CacheKey) -> Result<Option<Vec<u8>>, CacheError> {
        let mut conn = self.pool.get().await.map_err(RedisCacheError::from)?;
        let pk = self.prefixed_key(key);
        let value: Option<Vec<u8>> = conn.get(&pk).await.map_err(RedisCacheError::from)?;
        Ok(value)
    }

    async fn set(&self, key: &CacheKey, value: &[u8], ttl: Duration) -> Result<(), CacheError> {
        let mut conn = self.pool.get().await.map_err(RedisCacheError::from)?;
        let pk = self.prefixed_key(key);
        let ttl_secs = ttl.as_secs().max(1);
        let _: () = conn
            .set_ex(&pk, value, ttl_secs)
            .await
            .map_err(RedisCacheError::from)?;
        Ok(())
    }

    async fn delete(&self, key: &CacheKey) -> Result<bool, CacheError> {
        let mut conn = self.pool.get().await.map_err(RedisCacheError::from)?;
        let pk = self.prefixed_key(key);
        let removed: i64 = conn.del(&pk).await.map_err(RedisCacheError::from)?;
        Ok(removed > 0)
    }

    async fn invalidate(&self, key: &CacheKey) -> Result<(), CacheError> {
        let mut conn = self.pool.get().await.map_err(RedisCacheError::from)?;
        let channel = self.invalidate_channel();
        let _: i64 = conn
            .publish(&channel, key.as_str())
            .await
            .map_err(RedisCacheError::from)?;
        Ok(())
    }

    async fn subscribe_invalidations(&self) -> Result<InvalidationStream, CacheError> {
        let client = redis::Client::open(self.url.as_str())
            .map_err(|e| CacheError::Backend(e.to_string()))?;
        let mut pubsub = client
            .get_async_pubsub()
            .await
            .map_err(|e| CacheError::Backend(e.to_string()))?;
        let channel = self.invalidate_channel();
        pubsub
            .subscribe(&channel)
            .await
            .map_err(|e| CacheError::Backend(e.to_string()))?;

        let ns = self.namespace.clone();
        let s = async_stream::stream! {
            let mut msg_stream = pubsub.on_message();
            while let Some(msg) = msg_stream.next().await {
                if let Ok(payload) = msg.get_payload::<String>() {
                    if let Some(raw) = payload.strip_prefix(&format!("{}:", ns)) {
                        yield CacheKey::from_wire(raw);
                    } else {
                        yield CacheKey::from_wire(payload);
                    }
                }
            }
        };

        Ok(Box::pin(s))
    }
}

pub async fn ping(pool: &Pool) -> Result<(), RedisCacheError> {
    let mut conn = pool.get().await?;
    let _: String = redis::cmd("PING").query_async(&mut conn).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_placeholder_url_is_rejected() {
        let s = RedisSettings::default();
        let err = RedisCache::new(&s).unwrap_err();
        assert!(
            err.to_string().contains("not configured"),
            "expected not-configured error, got: {err}"
        );
    }

    fn live_url() -> Option<String> {
        std::env::var("KOKKAK_REDIS__TEST_URL")
            .ok()
            .filter(|s| !s.trim().is_empty())
    }

    #[test]
    fn live_pool_build_succeeds() {
        let Some(url) = live_url() else {
            eprintln!("skipping: set KOKKAK_REDIS__TEST_URL to run");
            return;
        };
        let s = RedisSettings {
            url,
            ..RedisSettings::default()
        };
        let cache = RedisCache::new(&s).expect("pool build against live redis must succeed");

        let rt = tokio::runtime::Runtime::new().expect("rt");
        rt.block_on(async {
            let key = CacheKey::new("v1", "test", "live", "pool_build");
            let _: () = cache
                .set(&key, b"hello", Duration::from_secs(60))
                .await
                .expect("set");
            let got = cache.get(&key).await.expect("get");
            assert_eq!(got.as_deref(), Some(b"hello".as_slice()));
            let _ = cache.delete(&key).await;
        });
    }

    #[test]
    fn live_ping_roundtrips() {
        let Some(url) = live_url() else {
            eprintln!("skipping: set KOKKAK_REDIS__TEST_URL to run");
            return;
        };
        let s = RedisSettings {
            url,
            ..RedisSettings::default()
        };
        let cache = RedisCache::new(&s).expect("pool build");
        let pool = cache.pool();
        let rt = tokio::runtime::Runtime::new().expect("rt");
        rt.block_on(async {
            ping(&pool).await.expect("PING must succeed");
        });
    }
}
