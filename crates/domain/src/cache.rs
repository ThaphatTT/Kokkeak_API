

use std::time::Duration;

use async_trait::async_trait;
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CacheGroup {

    A,

    B,

    C,

    D,
}

impl CacheGroup {

    pub fn default_ttl(&self) -> Option<Duration> {
        match self {
            Self::A => Some(Duration::from_secs(3600)),
            Self::B => Some(Duration::from_secs(300)),
            Self::C => Some(Duration::from_secs(60)),
            Self::D => None,
        }
    }

    pub fn allows_caching(&self) -> bool {
        !matches!(self, Self::D)
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::A => "A",
            Self::B => "B",
            Self::C => "C",
            Self::D => "D",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CacheKey(String);

impl CacheKey {

    pub fn new(version: &str, domain: &str, entity: &str, id: &str) -> Self {
        Self(format!("kokkak:{version}:{domain}:{entity}:{id}"))
    }

    pub fn with_variant(mut self, variant: &str) -> Self {
        self.0.push(':');
        self.0.push_str(variant);
        self
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn from_wire(s: impl Into<String>) -> Self {
        Self(s.into())
    }
}

impl std::fmt::Display for CacheKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, Error)]
pub enum CacheError {

    #[error("cache backend error: {0}")]
    Backend(String),

    #[error("cache codec error: {0}")]
    Codec(String),

    #[error("cache timeout: {0}")]
    Timeout(String),
}

#[async_trait]
pub trait Cache: Send + Sync {

    async fn get(&self, key: &CacheKey) -> Result<Option<Vec<u8>>, CacheError>;

    async fn set(&self, key: &CacheKey, value: &[u8], ttl: Duration) -> Result<(), CacheError>;

    async fn delete(&self, key: &CacheKey) -> Result<bool, CacheError>;

    async fn invalidate(&self, key: &CacheKey) -> Result<(), CacheError>;

    async fn subscribe_invalidations(&self) -> Result<InvalidationStream, CacheError>;
}

pub type InvalidationStream =
    std::pin::Pin<Box<dyn futures::Stream<Item = CacheKey> + Send + Sync>>;

#[async_trait]
pub trait CacheExt: Cache {

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

            std::cmp::max(ttl / 4, Duration::from_secs(5))
        } else {
            ttl
        };

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
