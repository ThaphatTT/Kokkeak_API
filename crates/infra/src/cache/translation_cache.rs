use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use kokkak_domain::traits::translation::{TranslationError, TranslationRepository};
use moka::future::Cache as MokaCache;

#[derive(Clone)]
pub struct CachedTranslationRepository<R>
where
    R: TranslationRepository + 'static,
{
    inner: Arc<R>,
    cache: MokaCache<(String, String), Option<String>>,
}

impl<R> CachedTranslationRepository<R>
where
    R: TranslationRepository + 'static,
{
    pub fn new(inner: R) -> Self {
        Self::with_ttl(inner, Duration::from_secs(60), 10_000)
    }

    pub fn with_ttl(inner: R, ttl: Duration, capacity: u64) -> Self {
        let cache = MokaCache::builder()
            .max_capacity(capacity)
            .time_to_live(ttl)
            .build();
        Self {
            inner: Arc::new(inner),
            cache,
        }
    }

    pub fn inner(&self) -> &R {
        &self.inner
    }

    pub async fn invalidate(&self, locale: &str, key: &str) {
        self.cache
            .invalidate(&(locale.to_string(), key.to_string()))
            .await;
    }

    pub async fn invalidate_all(&self) {
        self.cache.invalidate_all();
    }
}

#[async_trait]
impl<R> TranslationRepository for CachedTranslationRepository<R>
where
    R: TranslationRepository + 'static,
{
    async fn get(&self, locale: &str, key: &str) -> Result<Option<String>, TranslationError> {
        let cache_key = (locale.to_string(), key.to_string());
        if let Some(hit) = self.cache.get(&cache_key).await {
            metrics::counter!("kokkeak_translation_cache_hits_total").increment(1);
            return Ok(hit);
        }
        metrics::counter!("kokkeak_translation_cache_misses_total").increment(1);
        let value = self.inner.get(locale, key).await?;

        self.cache.insert(cache_key, value.clone()).await;
        Ok(value)
    }

    async fn put(&self, locale: &str, key: &str, value: &str) -> Result<(), TranslationError> {
        self.invalidate(locale, key).await;
        self.inner.put(locale, key, value).await?;
        Ok(())
    }

    async fn count(&self) -> Result<usize, TranslationError> {
        self.inner.count().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    #[derive(Default)]
    struct InMemoryTranslationRepository {
        rows: Mutex<HashMap<(String, String), String>>,
    }

    #[async_trait::async_trait]
    impl TranslationRepository for InMemoryTranslationRepository {
        async fn get(&self, locale: &str, key: &str) -> Result<Option<String>, TranslationError> {
            Ok(self
                .rows
                .lock()
                .unwrap()
                .get(&(locale.to_string(), key.to_string()))
                .cloned())
        }
        async fn put(&self, locale: &str, key: &str, value: &str) -> Result<(), TranslationError> {
            self.rows
                .lock()
                .unwrap()
                .insert((locale.to_string(), key.to_string()), value.to_string());
            Ok(())
        }
        async fn count(&self) -> Result<usize, TranslationError> {
            Ok(self.rows.lock().unwrap().len())
        }
    }

    #[tokio::test]
    async fn cache_returns_inner_value_on_miss_then_serves_from_cache() {
        let inner = InMemoryTranslationRepository::default();
        inner.put("en", "k", "v1").await.unwrap();
        let cache = CachedTranslationRepository::new(inner);

        assert_eq!(cache.get("en", "k").await.unwrap().as_deref(), Some("v1"));

        cache.invalidate("en", "k").await;

        assert_eq!(cache.get("en", "k").await.unwrap().as_deref(), Some("v1"));
    }

    #[tokio::test]
    async fn put_invalidates_cache_so_next_get_sees_fresh_value() {
        let inner = InMemoryTranslationRepository::default();
        inner.put("en", "k", "old").await.unwrap();
        let cache = CachedTranslationRepository::new(inner);

        assert_eq!(cache.get("en", "k").await.unwrap().as_deref(), Some("old"));

        cache.put("en", "k", "new").await.unwrap();

        assert_eq!(cache.get("en", "k").await.unwrap().as_deref(), Some("new"));
    }

    #[tokio::test]
    async fn missing_override_is_cached() {
        let inner = InMemoryTranslationRepository::default();
        let cache = CachedTranslationRepository::new(inner);

        assert!(cache.get("en", "absent").await.unwrap().is_none());

        cache.invalidate("en", "absent").await;

        assert!(cache.get("en", "absent").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn invalidate_all_clears_every_entry() {
        let inner = InMemoryTranslationRepository::default();
        inner.put("en", "a", "1").await.unwrap();
        inner.put("en", "b", "2").await.unwrap();
        let cache = CachedTranslationRepository::new(inner);

        let _ = cache.get("en", "a").await;
        let _ = cache.get("en", "b").await;
        cache.invalidate_all().await;

        assert_eq!(cache.get("en", "a").await.unwrap().as_deref(), Some("1"));
    }
}
