use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use kokkak_domain::{Cache, CacheError, CacheGroup, CacheKey, InvalidationStream};
use moka::future::Cache as MokaCache;
use tokio::sync::Mutex;

use super::redis::RedisCache;

type InflightVal = Arc<tokio::sync::Mutex<Option<Vec<u8>>>>;

#[derive(Clone)]
pub struct LayeredCache {
    l1: MokaCache<String, Vec<u8>>,
    l2: Option<Arc<RedisCache>>,

    inflight: Arc<Mutex<HashMap<String, InflightVal>>>,
}

impl LayeredCache {
    pub fn new(l1_capacity: u64, l1_max_ttl: Duration) -> Self {
        let l1 = MokaCache::builder()
            .max_capacity(l1_capacity)
            .time_to_live(l1_max_ttl)
            .build();
        Self {
            l1,
            l2: None,
            inflight: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn with_redis(mut self, redis: Arc<RedisCache>) -> Self {
        self.l2 = Some(redis);
        self
    }

    pub fn start_invalidation_listener(&self) {
        let Some(l2) = self.l2.clone() else {
            return;
        };
        let l1 = self.l1.clone();
        tokio::spawn(async move {
            match l2.subscribe_invalidations().await {
                Ok(mut stream) => {
                    use futures::StreamExt;
                    tracing::info!(
                        namespace = %l2.namespace(),
                        "cache invalidation listener started"
                    );
                    while let Some(key) = stream.next().await {
                        let prefixed = format!("{}:{}", l2.namespace(), key.as_str());
                        l1.invalidate(&prefixed).await;
                        l1.invalidate(key.as_str()).await;
                        metrics::counter!("kokkeak_cache_invalidations_received_total")
                            .increment(1);
                    }
                    tracing::warn!("cache invalidation listener stream ended");
                }
                Err(e) => {
                    tracing::warn!(error = %e, "failed to start cache invalidation listener");
                }
            }
        });
    }

    pub async fn get_or_load<F, Fut>(
        &self,
        key: &CacheKey,
        ttl: Duration,
        loader: F,
    ) -> Result<Vec<u8>, CacheError>
    where
        F: FnOnce() -> Fut + Send,
        Fut: std::future::Future<Output = (Vec<u8>, bool)> + Send,
    {
        if let Some(v) = self.l1.get(key.as_str()).await {
            metrics::counter!("kokkeak_cache_hits_total", "tier" => "l1").increment(1);
            return Ok(v);
        }

        if let Some(l2) = &self.l2 {
            if let Some(v) = l2.get(key).await? {
                self.l1.insert(key.as_str().to_string(), v.clone()).await;
                metrics::counter!("kokkeak_cache_hits_total", "tier" => "l2").increment(1);
                return Ok(v);
            }
        }

        metrics::counter!("kokkeak_cache_misses_total").increment(1);

        let mutex = {
            let mut map = self.inflight.lock().await;
            map.entry(key.as_str().to_string())
                .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(None)))
                .clone()
        };

        let mut guard = mutex.lock().await;
        if let Some(v) = guard.as_ref() {
            return Ok(v.clone());
        }

        let (value, is_negative) = loader().await;
        let effective_ttl = if is_negative {
            std::cmp::max(ttl / 4, Duration::from_secs(5))
        } else {
            ttl
        };
        if let Some(l2) = &self.l2 {
            let _ = l2.set(key, &value, effective_ttl).await;
        }
        self.l1
            .insert(key.as_str().to_string(), value.clone())
            .await;
        *guard = Some(value.clone());

        drop(guard);
        if let Ok(mut map) = self.inflight.try_lock() {
            map.remove(key.as_str());
        }
        Ok(value)
    }
}

#[async_trait]
impl Cache for LayeredCache {
    async fn get(&self, key: &CacheKey) -> Result<Option<Vec<u8>>, CacheError> {
        if let Some(v) = self.l1.get(key.as_str()).await {
            metrics::counter!("kokkeak_cache_hits_total", "tier" => "l1").increment(1);
            return Ok(Some(v));
        }
        if let Some(l2) = &self.l2 {
            if let Some(v) = l2.get(key).await? {
                self.l1.insert(key.as_str().to_string(), v.clone()).await;
                metrics::counter!("kokkeak_cache_hits_total", "tier" => "l2").increment(1);
                return Ok(Some(v));
            }
        }
        metrics::counter!("kokkeak_cache_misses_total").increment(1);
        Ok(None)
    }

    async fn set(&self, key: &CacheKey, value: &[u8], ttl: Duration) -> Result<(), CacheError> {
        if let Some(l2) = &self.l2 {
            l2.set(key, value, ttl).await?;
        }
        self.l1
            .insert(key.as_str().to_string(), value.to_vec())
            .await;
        Ok(())
    }

    async fn delete(&self, key: &CacheKey) -> Result<bool, CacheError> {
        self.l1.invalidate(key.as_str()).await;
        if let Some(l2) = &self.l2 {
            l2.delete(key).await
        } else {
            Ok(false)
        }
    }

    async fn invalidate(&self, key: &CacheKey) -> Result<(), CacheError> {
        self.l1.invalidate(key.as_str()).await;
        if let Some(l2) = &self.l2 {
            l2.invalidate(key).await?;
        }
        Ok(())
    }

    async fn subscribe_invalidations(&self) -> Result<InvalidationStream, CacheError> {
        if let Some(l2) = &self.l2 {
            l2.subscribe_invalidations().await
        } else {
            Ok(Box::pin(futures::stream::empty()))
        }
    }
}

pub fn from_settings(redis_settings: &kokkak_common::config::RedisSettings) -> LayeredCache {
    let mut cache = LayeredCache::new(10_000, Duration::from_secs(60));
    if let Ok(redis) = RedisCache::new(redis_settings) {
        cache = cache.with_redis(Arc::new(redis));
    } else {
        tracing::info!("redis not configured — layered cache running in L1-only mode");
    }
    cache
}

#[allow(dead_code)]
fn _group_keep(g: CacheGroup) -> Option<Duration> {
    g.default_ttl()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    #[tokio::test]
    async fn layered_cache_l1_only_starts_empty() {
        let cache = LayeredCache::new(100, Duration::from_secs(30));
        let key = CacheKey::new("v1", "test", "entity", "abc");
        assert!(cache.get(&key).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn layered_cache_l1_set_and_get() {
        let cache = LayeredCache::new(100, Duration::from_secs(30));
        let key = CacheKey::new("v1", "test", "entity", "abc");
        cache
            .set(&key, b"hello", Duration::from_secs(10))
            .await
            .unwrap();
        let got = cache.get(&key).await.unwrap();
        assert_eq!(got.as_deref(), Some(b"hello".as_ref()));
    }

    #[tokio::test]
    async fn layered_cache_l1_delete_drops_value() {
        let cache = LayeredCache::new(100, Duration::from_secs(30));
        let key = CacheKey::new("v1", "test", "entity", "abc");
        cache
            .set(&key, b"hello", Duration::from_secs(10))
            .await
            .unwrap();
        assert!(cache.get(&key).await.unwrap().is_some());
        let _ = cache.delete(&key).await.unwrap();
        assert!(cache.get(&key).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn layered_cache_l1_only_invalidate_is_local() {
        let cache = LayeredCache::new(100, Duration::from_secs(30));
        let key = CacheKey::new("v1", "test", "entity", "abc");
        cache
            .set(&key, b"hello", Duration::from_secs(10))
            .await
            .unwrap();
        cache.invalidate(&key).await.unwrap();
        assert!(cache.get(&key).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn get_or_load_runs_loader_on_miss_and_caches() {
        let cache = LayeredCache::new(100, Duration::from_secs(30));
        let key = CacheKey::new("v1", "test", "entity", "abc");
        let counter = Arc::new(AtomicUsize::new(0));
        let c2 = counter.clone();
        let v1 = cache
            .get_or_load(&key, Duration::from_secs(5), || {
                let c = c2.clone();
                async move {
                    c.fetch_add(1, Ordering::SeqCst);
                    (b"hello".to_vec(), false)
                }
            })
            .await
            .unwrap();
        assert_eq!(v1, b"hello");

        let c3 = counter.clone();
        let v2 = cache
            .get_or_load(&key, Duration::from_secs(5), || async move {
                c3.fetch_add(1, Ordering::SeqCst);
                (b"should-not-run".to_vec(), false)
            })
            .await
            .unwrap();
        assert_eq!(v2, b"hello");
        assert_eq!(
            counter.load(Ordering::SeqCst),
            1,
            "loader must run exactly once"
        );
    }

    #[tokio::test]
    async fn get_or_load_single_flight_under_concurrent_load() {
        let cache = Arc::new(LayeredCache::new(100, Duration::from_secs(30)));
        let key = CacheKey::new("v1", "test", "entity", "concurrent");
        let counter = Arc::new(AtomicUsize::new(0));
        let mut handles = Vec::new();
        for _ in 0..20 {
            let cache = cache.clone();
            let key = key.clone();
            let counter = counter.clone();
            handles.push(tokio::spawn(async move {
                cache
                    .get_or_load(&key, Duration::from_secs(5), || {
                        let c = counter.clone();
                        async move {
                            tokio::time::sleep(Duration::from_millis(50)).await;
                            c.fetch_add(1, Ordering::SeqCst);
                            (b"only-once".to_vec(), false)
                        }
                    })
                    .await
            }));
        }
        for h in handles {
            let v = h.await.unwrap().unwrap();
            assert_eq!(v, b"only-once");
        }

        let runs = counter.load(Ordering::SeqCst);
        assert!(
            (1..=3).contains(&runs),
            "expected 1..3 loader runs, got {runs}"
        );
    }

    #[test]
    fn from_settings_returns_l1_only_when_redis_unconfigured() {
        let s = kokkak_common::config::RedisSettings::default();
        let cache = from_settings(&s);
        assert!(cache.l2.is_none());
    }
}
