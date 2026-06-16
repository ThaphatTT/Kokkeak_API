//! Cache ports (พอร์ตแคช — T07 + T07A).
//!
//! Defines the contract every cache implementation must satisfy.
//! Concrete adapters (Redis, in-process moka, two-tier) live in `infra`.
//!
//! Per `AGENTS.md` § 7.9 / 9:
//! - Key convention: `kokkak:{ver}:{domain}:{entity}:{id}[:{variant}]`
//! - Group A (long TTL, 1-24h): master, catalog, config
//! - Group B (short TTL, 30s-5min): user profile, RBAC, rating
//! - Group C (Redis-as-source, volatile): rate-limit, OTP, GPS, idempotency
//! - Group D (NEVER cache): money, permission, transaction status
//! - Every write for A/B MUST invalidate the key.

use std::time::Duration;

use async_trait::async_trait;
use thiserror::Error;

/// Cache group / tier (กลุ่มแคช — กำหนดนโยบาย TTL).
///
/// The variant dictates how the cache should be used:
/// - `A` / `B`: cache-aside with TTL; **invalidate on write**.
/// - `C`: Redis is the source of truth (rate-limit, idempotency, ...).
/// - `D`: **never cache** — money, permission, transaction state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CacheGroup {
    /// Long-lived reference data (TTL 1h by default).
    /// Examples: master, catalog, config, geo.
    A,
    /// Short-lived user-scoped data (TTL 5min by default).
    /// Examples: user profile, RBAC permissions, ratings.
    B,
    /// Volatile session-ish data (TTL 60s default).
    /// Examples: rate-limit counters, OTP, GPS, idempotency keys.
    C,
    /// Forbidden to cache. Reads always hit the source of truth.
    /// Examples: order total, payment status, permission grants, doc running.
    D,
}

impl CacheGroup {
    /// Recommended TTL for this group (None = no caching).
    pub fn default_ttl(&self) -> Option<Duration> {
        match self {
            Self::A => Some(Duration::from_secs(3600)),
            Self::B => Some(Duration::from_secs(300)),
            Self::C => Some(Duration::from_secs(60)),
            Self::D => None,
        }
    }

    /// `true` when data in this group is allowed to be cached.
    pub fn allows_caching(&self) -> bool {
        !matches!(self, Self::D)
    }

    /// Short identifier used in metric labels.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::A => "A",
            Self::B => "B",
            Self::C => "C",
            Self::D => "D",
        }
    }
}

/// Cache key following the convention `kokkak:{ver}:{domain}:{entity}:{id}[:{variant}]`.
///
/// Build with [`CacheKey::new`] + optional [`CacheKey::with_variant`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CacheKey(String);

impl CacheKey {
    /// Build a new key.
    ///
    /// `version` is the schema version of the cached payload (bump it
    /// on incompatible shape changes to force a global refresh).
    pub fn new(version: &str, domain: &str, entity: &str, id: &str) -> Self {
        Self(format!("kokkak:{version}:{domain}:{entity}:{id}"))
    }

    /// Append a variant segment (e.g. `lang=lo`, `scope=mobile`).
    pub fn with_variant(mut self, variant: &str) -> Self {
        self.0.push(':');
        self.0.push_str(variant);
        self
    }

    /// Borrow the underlying string (e.g. for logging or hashing).
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Reconstruct a `CacheKey` from its wire form (e.g. after
    /// deserialising an invalidation pub/sub message). Accepts any
    /// string — the caller is responsible for filtering trusted sources.
    pub fn from_wire(s: impl Into<String>) -> Self {
        Self(s.into())
    }
}

impl std::fmt::Display for CacheKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Cache operation errors (ข้อผิดพลาดของแคช).
#[derive(Debug, Clone, Error)]
pub enum CacheError {
    /// Backend-specific failure (Redis disconnect, network, ...).
    #[error("cache backend error: {0}")]
    Backend(String),

    /// Serialization / deserialization failure.
    #[error("cache codec error: {0}")]
    Codec(String),

    /// Operation timed out.
    #[error("cache timeout: {0}")]
    Timeout(String),
}

/// Port every cache implementation must satisfy
/// (พอร์ตที่ทุก implementation ต้อง implement).
///
/// Two-tier implementations forward calls to L1 then L2 and publish
/// invalidation messages across instances. Single-tier implementations
/// skip the L1/publish steps.
#[async_trait]
pub trait Cache: Send + Sync {
    /// Look up `key`. Returns `None` when missing or expired.
    async fn get(&self, key: &CacheKey) -> Result<Option<Vec<u8>>, CacheError>;

    /// Store `value` under `key` with the given TTL. Past TTL the
    /// backend may evict the entry.
    async fn set(&self, key: &CacheKey, value: &[u8], ttl: Duration) -> Result<(), CacheError>;

    /// Delete `key`. Returns `true` when the key was present.
    async fn delete(&self, key: &CacheKey) -> Result<bool, CacheError>;

    /// Publish an invalidation message so peer instances drop their
    /// L1 copies. Adapters that do not participate in cross-instance
    /// invalidation can return `Ok(())` (single-process dev).
    async fn invalidate(&self, key: &CacheKey) -> Result<(), CacheError>;

    /// Subscribe to invalidation messages from peer instances.
    /// Returns a stream of keys that were invalidated elsewhere.
    async fn subscribe_invalidations(&self) -> Result<InvalidationStream, CacheError>;
}

/// A stream of cache-key invalidation events from peer instances
/// (สตรีมของ key ที่ instance อื่น invalidate).
///
/// Currently a thin alias; concrete impls use `tokio::sync::mpsc::Receiver`
/// or `async_nats::Subscriber`.
pub type InvalidationStream =
    std::pin::Pin<Box<dyn futures::Stream<Item = CacheKey> + Send + Sync>>;

/// Helper for the cache-aside pattern: read from cache, fall back to
/// `loader`, then store the result with `ttl`.
///
/// `get_or_load` is the **only** public entry point application code
/// should use — keeps the cache strategy (TTL jitter, negative cache,
/// single-flight) out of feature modules. Concrete implementations
/// live in `infra`; this default impl just calls the trait methods so
/// `Cache` can be used directly when the optimizations are not needed.
#[async_trait]
pub trait CacheExt: Cache {
    /// Cache-aside helper: `get` → on miss run `loader` → `set`.
    ///
    /// `loader` returns `(value, is_negative)`. When `is_negative` is
    /// `true`, the value is still cached but for a shorter TTL to
    /// avoid stampeding the source on a known-bad key.
    async fn get_or_load<F, Fut>(
        &self,
        key: &CacheKey,
        ttl: Duration,
        loader: F,
    ) -> Result<Vec<u8>, CacheError>
    where
        F: FnOnce() -> Fut + Send,
        Fut: std::future::Future<Output = (Vec<u8>, bool)> + Send,
    {
        if let Some(v) = self.get(key).await? {
            return Ok(v);
        }
        let (value, is_negative) = loader().await;
        let effective_ttl = if is_negative {
            // Negative cache: keep it short (1/4 of positive TTL, min 5s).
            std::cmp::max(ttl / 4, Duration::from_secs(5))
        } else {
            ttl
        };
        // Best-effort write — cache failures must not break the request.
        // The adapter is responsible for logging set failures (per AGENTS.md § 14).
        // We swallow the error here on purpose; callers already got the value.
        let _ = self.set(key, &value, effective_ttl).await;
        Ok(value)
    }
}

impl<T: Cache + ?Sized> CacheExt for T {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_group_default_ttls_are_in_expected_ranges() {
        assert!(CacheGroup::A.default_ttl().unwrap() >= Duration::from_secs(3600));
        assert!(CacheGroup::B.default_ttl().unwrap() <= Duration::from_secs(300));
        assert!(CacheGroup::C.default_ttl().unwrap() <= Duration::from_secs(60));
        assert!(CacheGroup::D.default_ttl().is_none());
    }

    #[test]
    fn only_group_d_forbids_caching() {
        assert!(CacheGroup::A.allows_caching());
        assert!(CacheGroup::B.allows_caching());
        assert!(CacheGroup::C.allows_caching());
        assert!(!CacheGroup::D.allows_caching());
    }

    #[test]
    fn cache_key_follows_convention() {
        let k = CacheKey::new("v1", "user", "profile", "abc-123");
        assert_eq!(k.as_str(), "kokkak:v1:user:profile:abc-123");
    }

    #[test]
    fn cache_key_with_variant_appends() {
        let k = CacheKey::new("v1", "catalog", "service_sub", "x").with_variant("lang=lo");
        assert_eq!(k.as_str(), "kokkak:v1:catalog:service_sub:x:lang=lo");
    }

    #[test]
    fn cache_key_with_two_variants() {
        let k = CacheKey::new("v1", "user", "profile", "abc")
            .with_variant("lang=lo")
            .with_variant("scope=mobile");
        assert_eq!(
            k.as_str(),
            "kokkak:v1:user:profile:abc:lang=lo:scope=mobile"
        );
    }

    #[test]
    fn cache_key_display_matches_as_str() {
        let k = CacheKey::new("v1", "user", "profile", "abc");
        assert_eq!(format!("{k}"), k.as_str());
    }
}
