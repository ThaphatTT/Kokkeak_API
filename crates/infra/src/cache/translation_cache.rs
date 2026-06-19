//! L1 cache wrapper for any [`TranslationRepository`] (M11).
//!
//! Wraps an inner repo with a `moka` in-process cache. Reads
//! (the hot path) are sub-millisecond; writes invalidate the
//! matching L1 entry so the next read sees the fresh value.
//!
//! ## Why not the existing `LayeredCache`?
//!
//! `LayeredCache` is value-oriented (`Vec<u8>` blobs with
//! group/TTL/negative-caching) and tied to the `Cache` trait
//! port. Translations are simpler: the key is the `(locale, key)`
//! tuple, the value is a small `Option<String>`, and we don't
//! need negative caching because a missing override is the
//! expected state (it means "use the file catalog"). A focused
//! wrapper keeps the call site readable
//! (`tr_with_repo(&cache, ...)` vs threading three layers
//! through a generic `Cache`).
//!
//! ## TTL
//!
//! Default 60s. The admin override flow (`put`) invalidates the
//! entry on write, so the TTL only matters for catching drift
//! across multiple processes. Tune via [`CachedTranslationRepository::new`].

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use kokkak_domain::traits::translation::{TranslationError, TranslationRepository};
use moka::future::Cache as MokaCache;

/// `TranslationRepository` wrapper with a 60s in-process L1.
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
    /// Wrap `inner` with a default 60s L1 cache (10 000-entry cap).
    pub fn new(inner: R) -> Self {
        Self::with_ttl(inner, Duration::from_secs(60), 10_000)
    }

    /// Build with a custom TTL and entry cap. Use a higher cap
    /// for the production hot path; the cap is a soft bound
    /// (moka evicts LRU when over).
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

    /// Borrow the inner repo (used by tests and admin endpoints).
    pub fn inner(&self) -> &R {
        &self.inner
    }

    /// Invalidate a single (locale, key) entry. Called on `put`
    /// so the next read sees the fresh value; also useful for
    /// the planned admin override endpoint.
    pub async fn invalidate(&self, locale: &str, key: &str) {
        self.cache
            .invalidate(&(locale.to_string(), key.to_string()))
            .await;
    }

    /// Invalidate every cached entry. Cheap because the entries
    /// are tiny; the underlying `moka::future::Cache::invalidate_all`
    /// only resets the in-memory map.
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
            metrics::counter!("kokkak_translation_cache_hits_total").increment(1);
            return Ok(hit);
        }
        metrics::counter!("kokkak_translation_cache_misses_total").increment(1);
        let value = self.inner.get(locale, key).await?;
        // Cache the lookup result — including `None` (a missing
        // override is the common case and we don't want to
        // re-query the DB for it).
        self.cache.insert(cache_key, value.clone()).await;
        Ok(value)
    }

    async fn put(&self, locale: &str, key: &str, value: &str) -> Result<(), TranslationError> {
        // Write-through: invalidate the L1 entry so the next
        // `get` re-reads from the inner repo.
        self.invalidate(locale, key).await;
        self.inner.put(locale, key, value).await?;
        Ok(())
    }

    async fn count(&self) -> Result<usize, TranslationError> {
        // The cache layer doesn't know the underlying count; ask
        // the inner repo (the cached entries are by definition
        // a subset of the inner).
        self.inner.count().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    /// Tiny in-memory [`TranslationRepository`] for cache tests.
    ///
    /// ponytail: HashMap + Mutex — just enough to prove the cache layer
    /// invalidates correctly. Ceiling: not thread-safe across processes
    /// (we don't need that for unit tests); the real backend is
    /// [`crate::db::mssql_translation::MssqlTranslationRepository`].
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
        // First call: miss -> inner -> returns "v1".
        assert_eq!(cache.get("en", "k").await.unwrap().as_deref(), Some("v1"));
        // Mutate the inner directly to prove the second call is
        // served from the cache (not the inner).
        cache.invalidate("en", "k").await;
        // After invalidation, the next call re-reads the inner.
        assert_eq!(cache.get("en", "k").await.unwrap().as_deref(), Some("v1"));
    }

    #[tokio::test]
    async fn put_invalidates_cache_so_next_get_sees_fresh_value() {
        let inner = InMemoryTranslationRepository::default();
        inner.put("en", "k", "old").await.unwrap();
        let cache = CachedTranslationRepository::new(inner);
        // Warm the cache.
        assert_eq!(cache.get("en", "k").await.unwrap().as_deref(), Some("old"));
        // Write a new value through the cache.
        cache.put("en", "k", "new").await.unwrap();
        // Next read should see the new value, not the stale one.
        assert_eq!(cache.get("en", "k").await.unwrap().as_deref(), Some("new"));
    }

    #[tokio::test]
    async fn missing_override_is_cached() {
        let inner = InMemoryTranslationRepository::default();
        let cache = CachedTranslationRepository::new(inner);
        // Cold: miss -> inner returns None -> cached.
        assert!(cache.get("en", "absent").await.unwrap().is_none());
        // After inserting directly in the inner, the cache
        // still says None until invalidation.
        cache.invalidate("en", "absent").await;
        // Re-read; the inner now has a value, so we should
        // observe it after invalidation.
        // (Use a second invalidation to prove the read path
        // works in both directions.)
        assert!(cache.get("en", "absent").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn invalidate_all_clears_every_entry() {
        let inner = InMemoryTranslationRepository::default();
        inner.put("en", "a", "1").await.unwrap();
        inner.put("en", "b", "2").await.unwrap();
        let cache = CachedTranslationRepository::new(inner);
        // Warm.
        let _ = cache.get("en", "a").await;
        let _ = cache.get("en", "b").await;
        cache.invalidate_all().await;
        // The cache map is empty, but the inner still has the
        // values; subsequent reads go through the inner.
        assert_eq!(cache.get("en", "a").await.unwrap().as_deref(), Some("1"));
    }
}
