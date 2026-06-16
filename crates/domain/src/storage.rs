//! Object storage port (พอร์ต object storage — M9).
//!
//! Application code depends on this trait; concrete adapters
//! (in-memory, S3 / MinIO) live in `infra::storage`. The trait
//! stays deliberately small — just the operations the chat
//! attachments and the receipt-image flows need:
//!
//! - `put` an opaque blob, get back a `StorageKey`.
//! - `presigned_get_url` so a client can download without
//!   round-tripping the API.
//! - `delete` a blob by key (idempotent; missing = `Ok(())`).
//!
//! Per AGENTS.md § 11, blobs are content-addressed: the caller
//! supplies the SHA-256 hex digest and the adapter is allowed
//! to trust it (or verify it on the side).

use async_trait::async_trait;
use bytes::Bytes;
use thiserror::Error;

/// Opaque, adapter-defined blob reference (e.g. S3 object key).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StorageKey(pub String);

impl StorageKey {
    /// Borrow the underlying string.
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

/// Storage operation errors.
#[derive(Debug, Error)]
pub enum StorageError {
    /// Underlying backend (S3 / MinIO / filesystem) failure.
    #[error("storage backend error: {0}")]
    Backend(String),

    /// The blob is too large for this adapter (S3 single PUT
    /// limit is 5 GiB; multipart handles more in M12+).
    #[error("blob too large: {0} bytes")]
    TooLarge(usize),

    /// SHA-256 mismatch between the caller's digest and the
    /// actual bytes — surfaces tampering / corruption.
    #[error("sha256 mismatch: expected {expected}, got {actual}")]
    HashMismatch { expected: String, actual: String },
}

/// Result of a successful `put`.
#[derive(Debug, Clone)]
pub struct PutResult {
    /// Adapter-assigned key.
    pub key: StorageKey,
    /// SHA-256 hex digest of the stored bytes.
    pub sha256: String,
    /// Byte count actually stored.
    pub size: usize,
}

/// Port every object-storage adapter must satisfy.
#[async_trait]
pub trait Storage: Send + Sync {
    /// Store `bytes` under `key`. Returns the key + a digest /
    /// size summary. Implementations may verify `expected_sha256`
    /// when `Some`.
    async fn put(
        &self,
        key: &StorageKey,
        bytes: Bytes,
        expected_sha256: Option<&str>,
    ) -> Result<PutResult, StorageError>;

    /// Fetch a blob by key. Returns `Ok(None)` for missing keys.
    async fn get(&self, key: &StorageKey) -> Result<Option<Bytes>, StorageError>;

    /// Delete a blob. Missing keys are not an error.
    async fn delete(&self, key: &StorageKey) -> Result<(), StorageError>;

    /// Generate a short-lived `GetObject` URL the client can use
    /// to download directly. The `ttl_secs` is advisory; some
    /// adapters (in-memory) may return `None` and force the
    /// caller to use the API as a proxy.
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
