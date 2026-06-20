//! Redis cache adapter (T07).
//!
//! Wraps `deadpool-redis` for the connection pool and the `redis` crate
//! for the wire protocol. Implements [`kokkak_domain::Cache`] and
//! publishes / subscribes to the `kokkak.cache.invalidate` channel so
//! peer instances can drop their L1 copies.
//!
//! See `AGENTS.md` § 7.9 (key convention, group policy) and § 9.4
//! (pub/sub backplane).
//!
//! ## TODO(M1.5)
//!
//! `subscribe_invalidations` currently returns an empty stream. The
//! pub/sub loop needs a dedicated long-lived connection (the deadpool
//! cannot multiplex SUBSCRIBE with normal commands). Add a background
//! `tokio::spawn` in `LayeredCache::new` that opens a raw `redis::Client`
//! connection, subscribes to the channel, and pushes messages into an
//! mpsc receiver the `LayeredCache` drains. See [`layer::LayeredCache`].

use std::time::Duration;

use async_trait::async_trait;
use deadpool_redis::{Config, Pool, Runtime};
use futures::stream;
use kokkak_common::config::RedisSettings;
use kokkak_domain::{Cache, CacheError, CacheKey, InvalidationStream};
use redis::AsyncCommands;
use thiserror::Error;

/// Channel name for cross-instance invalidation messages
/// (ชื่อ channel สำหรับส่งสัญญาณ invalidate ข้าม instance).
const INVALIDATE_CHANNEL: &str = "kokkak.cache.invalidate";

/// Errors raised by the Redis cache adapter
/// (ข้อผิดพลาดของ Redis cache adapter).
#[derive(Debug, Error)]
pub enum RedisCacheError {
    /// Pool creation / config failure.
    #[error("redis pool build failed: {0}")]
    PoolBuild(String),

    /// Underlying redis error.
    #[error("redis error: {0}")]
    Redis(#[from] redis::RedisError),

    /// Pool-exhaustion error from deadpool.
    #[error("redis pool error: {0}")]
    Pool(#[from] deadpool_redis::PoolError),
}

impl From<RedisCacheError> for CacheError {
    fn from(err: RedisCacheError) -> Self {
        CacheError::Backend(err.to_string())
    }
}

/// Redis-backed cache client
/// (cache client ที่ใช้ Redis เป็น backend).
#[derive(Clone, Debug)]
pub struct RedisCache {
    pool: Pool,
}

impl RedisCache {
    /// Build a new Redis cache from settings.
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
            "redis pool built"
        );

        Ok(Self { pool })
    }

    /// Borrow a connection from the pool (for advanced callers).
    pub async fn conn(&self) -> Result<deadpool_redis::Connection, RedisCacheError> {
        Ok(self.pool.get().await?)
    }

    /// Expose the pool directly (for adapters that need it, e.g. the
    /// worker's `RedisIdempotency` cache).
    pub fn pool(&self) -> deadpool_redis::Pool {
        self.pool.clone()
    }
}

#[async_trait]
impl Cache for RedisCache {
    async fn get(&self, key: &CacheKey) -> Result<Option<Vec<u8>>, CacheError> {
        let mut conn = self.pool.get().await.map_err(RedisCacheError::from)?;
        let value: Option<Vec<u8>> = conn
            .get(key.as_str())
            .await
            .map_err(RedisCacheError::from)?;
        Ok(value)
    }

    async fn set(&self, key: &CacheKey, value: &[u8], ttl: Duration) -> Result<(), CacheError> {
        let mut conn = self.pool.get().await.map_err(RedisCacheError::from)?;
        let ttl_secs = ttl.as_secs().max(1);
        let _: () = conn
            .set_ex(key.as_str(), value, ttl_secs)
            .await
            .map_err(RedisCacheError::from)?;
        Ok(())
    }

    async fn delete(&self, key: &CacheKey) -> Result<bool, CacheError> {
        let mut conn = self.pool.get().await.map_err(RedisCacheError::from)?;
        let removed: i64 = conn
            .del(key.as_str())
            .await
            .map_err(RedisCacheError::from)?;
        Ok(removed > 0)
    }

    async fn invalidate(&self, key: &CacheKey) -> Result<(), CacheError> {
        // Fire-and-forget; the subscriber on peer instances will drop
        // their L1 copies asynchronously.
        let mut conn = self.pool.get().await.map_err(RedisCacheError::from)?;
        let _: i64 = conn
            .publish(INVALIDATE_CHANNEL, key.as_str())
            .await
            .map_err(RedisCacheError::from)?;
        Ok(())
    }

    async fn subscribe_invalidations(&self) -> Result<InvalidationStream, CacheError> {
        // TODO(M1.5): open a dedicated SUBSCRIBE connection (deadpool-redis
        // cannot multiplex SUBSCRIBE on the command pool). Until then,
        // return an empty stream so the registry still constructs.
        Ok(Box::pin(stream::empty()))
    }
}

/// Ping the Redis server (returns when a `PING` round-trips).
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

    // ---- Live integration tests (opt-in via KOKKAK_REDIS__TEST_URL) ----
    //
    // These tests connect to a REAL Redis server. By default they
    // are skipped (the env var is unset in CI). To run them
    // against the local dev box:
    //
    // ```bash
    // KOKKAK_REDIS__TEST_URL=redis://10.0.200.83:6379 \
    //   cargo test -p kokkak-infra --lib cache::redis::tests::live_
    // ```
    //
    // M13: these are the M1-deferred smoke tests for the T07
    // wiring. They run against the operator's Redis when the
    // test URL is provided. They never run in the default CI
    // path (no env var = skip).

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
        // Round-trip: a key set right now must be readable. This
        // also proves the pool actually produces working
        // connections (the pool build alone can succeed against
        // a closed port).
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
