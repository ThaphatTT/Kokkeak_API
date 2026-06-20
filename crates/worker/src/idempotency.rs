//! Idempotency for NATS consumers.
//!
//! Every consumer handler **MUST** check the message id against an
//! idempotency cache before performing any side-effect (AGENTS.md § 10).
//! The cache is keyed by the consumer name + NATS message id (a
//! server-assigned UUID).
//!
//! Two implementations are provided:
//! - `InMemoryIdempotency` — local cache, no external deps. Used when
//!   Redis is not configured.
//! - `RedisIdempotency` — backed by Redis `SETNX` with TTL. Preferred
//!   for production (multi-instance).

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use thiserror::Error;
use tokio::sync::Mutex;

/// Stable identifier for the (consumer, message) pair.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct IdempotencyKey {
    /// Consumer / subject name (e.g. `"noti.push"`).
    pub consumer: String,
    /// NATS message id (or any unique id from upstream).
    pub message_id: String,
}

impl IdempotencyKey {
    /// Build a new key from the consumer name + upstream message id.
    pub fn new(consumer: impl Into<String>, message_id: impl Into<String>) -> Self {
        Self {
            consumer: consumer.into(),
            message_id: message_id.into(),
        }
    }

    /// Wire format for the cache (stable, no collisions between
    /// consumers that happen to share message ids).
    pub fn cache_key(&self) -> String {
        format!("kokkak:idemp:{}:{}", self.consumer, self.message_id)
    }
}

/// Errors raised by the idempotency cache backend.
#[derive(Debug, Error)]
pub enum IdempotencyError {
    /// Cache backend failure (Redis down, etc.).
    #[error("idempotency backend error: {0}")]
    Backend(String),
}

/// Port every idempotency cache implements.
#[async_trait]
pub trait Idempotency: Send + Sync {
    /// Returns `true` when this is the **first** time we see the key
    /// (caller should run the side-effect). Returns `false` for a
    /// duplicate (caller must skip).
    async fn claim(&self, key: &IdempotencyKey, ttl: Duration) -> Result<bool, IdempotencyError>;
}

/// In-memory idempotency cache (single-process, no TTL — relies on
/// `tokio::sync::Mutex` and bounded by a constant LRU when needed).
pub struct InMemoryIdempotency {
    seen: Arc<Mutex<std::collections::HashSet<String>>>,
    max_entries: usize,
}

impl InMemoryIdempotency {
    /// Construct with a soft cap on cache size (`max_entries`); eviction
    /// is naive (drops half) — see M4 note about LRU.
    pub fn new(max_entries: usize) -> Self {
        Self {
            seen: Arc::new(Mutex::new(std::collections::HashSet::new())),
            max_entries,
        }
    }
}

#[async_trait]
impl Idempotency for InMemoryIdempotency {
    async fn claim(&self, key: &IdempotencyKey, _ttl: Duration) -> Result<bool, IdempotencyError> {
        let cache_key = key.cache_key();
        let mut seen = self.seen.lock().await;
        if seen.contains(&cache_key) {
            return Ok(false);
        }
        if seen.len() >= self.max_entries {
            // Naive eviction: clear half. Real impl: LRU.
            let to_drop: Vec<String> = seen.iter().take(self.max_entries / 2).cloned().collect();
            for k in to_drop {
                seen.remove(&k);
            }
        }
        seen.insert(cache_key);
        Ok(true)
    }
}

/// Redis-backed idempotency cache.
pub struct RedisIdempotency {
    pool: deadpool_redis::Pool,
}

impl RedisIdempotency {
    /// Wrap a deadpool-redis pool (production multi-instance cache).
    pub fn new(pool: deadpool_redis::Pool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl Idempotency for RedisIdempotency {
    async fn claim(&self, key: &IdempotencyKey, ttl: Duration) -> Result<bool, IdempotencyError> {
        let mut conn = self
            .pool
            .get()
            .await
            .map_err(|e| IdempotencyError::Backend(e.to_string()))?;
        let cache_key = key.cache_key();
        let ttl_secs = ttl.as_secs().max(1);
        let result: Option<String> = redis::cmd("SET")
            .arg(&cache_key)
            .arg("1")
            .arg("NX")
            .arg("EX")
            .arg(ttl_secs)
            .query_async(&mut conn)
            .await
            .map_err(|e| IdempotencyError::Backend(e.to_string()))?;
        // `SET ... NX` returns "OK" on first set, `nil` on duplicate.
        Ok(result.is_some())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn in_memory_first_claim_succeeds_second_fails() {
        let cache = InMemoryIdempotency::new(100);
        let key = IdempotencyKey::new("noti.push", "msg-1");
        assert!(cache.claim(&key, Duration::from_secs(60)).await.unwrap());
        assert!(!cache.claim(&key, Duration::from_secs(60)).await.unwrap());
    }

    #[tokio::test]
    async fn in_memory_different_keys_independent() {
        let cache = InMemoryIdempotency::new(100);
        let k1 = IdempotencyKey::new("noti.push", "msg-1");
        let k2 = IdempotencyKey::new("noti.push", "msg-2");
        let k3 = IdempotencyKey::new("comm.email", "msg-1");
        assert!(cache.claim(&k1, Duration::from_secs(60)).await.unwrap());
        assert!(cache.claim(&k2, Duration::from_secs(60)).await.unwrap());
        // Different consumer = independent.
        assert!(cache.claim(&k3, Duration::from_secs(60)).await.unwrap());
    }

    #[tokio::test]
    async fn in_memory_cache_key_includes_consumer() {
        let key = IdempotencyKey::new("a", "x");
        assert_eq!(key.cache_key(), "kokkak:idemp:a:x");
    }
}
