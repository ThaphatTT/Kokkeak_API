

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use thiserror::Error;
use tokio::sync::Mutex;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct IdempotencyKey {

    pub consumer: String,

    pub message_id: String,
}

impl IdempotencyKey {

    pub fn new(consumer: impl Into<String>, message_id: impl Into<String>) -> Self {
        Self {
            consumer: consumer.into(),
            message_id: message_id.into(),
        }
    }

    pub fn cache_key(&self) -> String {
        format!("kokkak:idemp:{}:{}", self.consumer, self.message_id)
    }
}

#[derive(Debug, Error)]
pub enum IdempotencyError {

    #[error("idempotency backend error: {0}")]
    Backend(String),
}

#[async_trait]
pub trait Idempotency: Send + Sync {

    async fn claim(&self, key: &IdempotencyKey, ttl: Duration) -> Result<bool, IdempotencyError>;
}

pub struct InMemoryIdempotency {
    seen: Arc<Mutex<std::collections::HashSet<String>>>,
    max_entries: usize,
}

impl InMemoryIdempotency {

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

            let to_drop: Vec<String> = seen.iter().take(self.max_entries / 2).cloned().collect();
            for k in to_drop {
                seen.remove(&k);
            }
        }
        seen.insert(cache_key);
        Ok(true)
    }
}

pub struct RedisIdempotency {
    pool: deadpool_redis::Pool,
}

impl RedisIdempotency {

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

        assert!(cache.claim(&k3, Duration::from_secs(60)).await.unwrap());
    }

    #[tokio::test]
    async fn in_memory_cache_key_includes_consumer() {
        let key = IdempotencyKey::new("a", "x");
        assert_eq!(key.cache_key(), "kokkak:idemp:a:x");
    }
}
