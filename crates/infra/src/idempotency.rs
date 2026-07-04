

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use kokkak_domain::{CachedResponse, IdempotencyStore};
use tokio::sync::Mutex;

pub struct InMemoryIdempotencyStore {
    entries: Arc<Mutex<HashMap<String, Entry>>>,
    max_entries: usize,
}

struct Entry {
    response: CachedResponse,
    expires_at: Instant,
}

impl InMemoryIdempotencyStore {

    pub fn new(max_entries: usize) -> Self {
        Self {
            entries: Arc::new(Mutex::new(HashMap::new())),
            max_entries,
        }
    }
}

#[async_trait]
impl IdempotencyStore for InMemoryIdempotencyStore {
    async fn get(&self, key: &str) -> Option<CachedResponse> {
        let mut map = self.entries.lock().await;
        match map.get(key) {
            Some(entry) if entry.expires_at > Instant::now() => Some(entry.response.clone()),

            Some(_) => {
                map.remove(key);
                None
            }
            None => None,
        }
    }

    async fn put(&self, key: &str, response: CachedResponse, ttl: Duration) {
        let expires_at = Instant::now() + ttl;
        let mut map = self.entries.lock().await;

        if map.len() >= self.max_entries {

            let to_drop = map.len() / 2;
            let keys: Vec<String> = map.keys().take(to_drop).cloned().collect();
            for k in keys {
                map.remove(&k);
            }
        }

        map.insert(
            key.to_string(),
            Entry {
                response,
                expires_at,
            },
        );
    }

    fn len(&self) -> usize {

        match self.entries.try_lock() {
            Ok(map) => map.len(),
            Err(_) => 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn small_resp() -> CachedResponse {
        CachedResponse {
            status: 200,
            content_type: "application/json".into(),
            body: br#"{"ok":true}"#.to_vec(),
        }
    }

    #[tokio::test]
    async fn get_returns_none_on_miss() {
        let store = InMemoryIdempotencyStore::new(100);
        assert!(store.get("missing").await.is_none());
    }

    #[tokio::test]
    async fn put_then_get_returns_cached_response() {
        let store = InMemoryIdempotencyStore::new(100);
        let resp = small_resp();
        store.put("k1", resp.clone(), Duration::from_secs(60)).await;

        let got = store.get("k1").await.expect("cache hit");
        assert_eq!(got, resp);
    }

    #[tokio::test]
    async fn get_returns_none_after_ttl_expires() {
        let store = InMemoryIdempotencyStore::new(100);
        store
            .put("k1", small_resp(), Duration::from_millis(10))
            .await;

        tokio::time::sleep(Duration::from_millis(25)).await;
        assert!(
            store.get("k1").await.is_none(),
            "expired entry must be a miss"
        );
    }

    #[tokio::test]
    async fn over_cap_triggers_half_flush_eviction() {
        let store = InMemoryIdempotencyStore::new(4);
        for i in 0..4 {
            store
                .put(&format!("k{i}"), small_resp(), Duration::from_secs(60))
                .await;
        }
        assert_eq!(store.len(), 4);

        store.put("k4", small_resp(), Duration::from_secs(60)).await;
        assert!(
            store.len() <= 3,
            "post-eviction len must be < cap+1, got {}",
            store.len()
        );
    }

    #[tokio::test]
    async fn different_keys_are_independent() {
        let store = InMemoryIdempotencyStore::new(100);
        let a = CachedResponse {
            status: 200,
            content_type: "application/json".into(),
            body: br#"{"a":1}"#.to_vec(),
        };
        let b = CachedResponse {
            status: 201,
            content_type: "application/json".into(),
            body: br#"{"b":2}"#.to_vec(),
        };
        store.put("ka", a.clone(), Duration::from_secs(60)).await;
        store.put("kb", b.clone(), Duration::from_secs(60)).await;
        assert_eq!(store.get("ka").await, Some(a));
        assert_eq!(store.get("kb").await, Some(b));
    }

    #[tokio::test]
    async fn overwriting_same_key_replaces_response() {
        let store = InMemoryIdempotencyStore::new(100);
        let v1 = CachedResponse {
            status: 200,
            content_type: "application/json".into(),
            body: b"v1".to_vec(),
        };
        let v2 = CachedResponse {
            status: 200,
            content_type: "application/json".into(),
            body: b"v2".to_vec(),
        };
        store.put("k", v1, Duration::from_secs(60)).await;
        store.put("k", v2.clone(), Duration::from_secs(60)).await;
        assert_eq!(store.get("k").await, Some(v2));
    }
}
