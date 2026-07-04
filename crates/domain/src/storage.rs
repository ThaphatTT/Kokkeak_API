

use async_trait::async_trait;
use bytes::Bytes;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StorageKey(pub String);

impl StorageKey {

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for StorageKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<String> for StorageKey {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for StorageKey {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

#[derive(Debug, Error)]
pub enum StorageError {

    #[error("storage backend error: {0}")]
    Backend(String),

    #[error("blob too large: {0} bytes")]
    TooLarge(usize),

    #[error("sha256 mismatch: expected {expected}, got {actual}")]
    HashMismatch {

        expected: String,

        actual: String,
    },
}

#[derive(Debug, Clone)]
pub struct PutResult {

    pub key: StorageKey,

    pub sha256: String,

    pub size: usize,
}

#[async_trait]
pub trait Storage: Send + Sync {

    async fn put(
        &self,
        key: &StorageKey,
        bytes: Bytes,
        expected_sha256: Option<&str>,
    ) -> Result<PutResult, StorageError>;

    async fn get(&self, key: &StorageKey) -> Result<Option<Bytes>, StorageError>;

    async fn delete(&self, key: &StorageKey) -> Result<(), StorageError>;

    async fn presigned_get_url(
        &self,
        key: &StorageKey,
        ttl_secs: u32,
    ) -> Result<Option<String>, StorageError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storage_key_display_and_from() {
        let k: StorageKey = "users/avatars/1".into();
        assert_eq!(k.as_str(), "users/avatars/1");
        assert_eq!(k.to_string(), "users/avatars/1");
        let k2: StorageKey = String::from("k").into();
        assert_eq!(k2.as_str(), "k");
    }
}
