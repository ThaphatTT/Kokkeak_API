

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use kokkak_domain::{PutResult, Storage, StorageError, StorageKey};
use sha2::{Digest, Sha256};
use tokio::sync::RwLock;

#[derive(Clone, Default)]
pub struct MemoryStorage {
    blobs: Arc<RwLock<HashMap<String, Vec<u8>>>>,
}

impl MemoryStorage {

    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl Storage for MemoryStorage {
    async fn put(
        &self,
        key: &StorageKey,
        bytes: Bytes,
        expected_sha256: Option<&str>,
    ) -> Result<PutResult, StorageError> {
        let actual = {
            let mut h = Sha256::new();
            h.update(&bytes);
            format!("{:x}", h.finalize())
        };
        if let Some(expected) = expected_sha256 {
            if expected != actual {
                return Err(StorageError::HashMismatch {
                    expected: expected.to_string(),
                    actual,
                });
            }
        }
        let size = bytes.len();
        let mut g = self.blobs.write().await;
        g.insert(key.0.clone(), bytes.to_vec());
        Ok(PutResult {
            key: key.clone(),
            sha256: actual,
            size,
        })
    }

    async fn get(&self, key: &StorageKey) -> Result<Option<Bytes>, StorageError> {
        let g = self.blobs.read().await;
        Ok(g.get(&key.0).map(|v| Bytes::copy_from_slice(v)))
    }

    async fn delete(&self, key: &StorageKey) -> Result<(), StorageError> {
        let mut g = self.blobs.write().await;
        g.remove(&key.0);
        Ok(())
    }

    async fn presigned_get_url(
        &self,
        _key: &StorageKey,
        _ttl_secs: u32,
    ) -> Result<Option<String>, StorageError> {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn put_and_get_round_trip() {
        let s = MemoryStorage::new();
        let key: StorageKey = "test/blob".into();
        let body = Bytes::from_static(b"hello world");
        let r = s.put(&key, body.clone(), None).await.unwrap();
        assert_eq!(r.size, 11);
        let got = s.get(&key).await.unwrap().unwrap();
        assert_eq!(got, body);
    }

    #[tokio::test]
    async fn put_verifies_expected_sha256() {
        let s = MemoryStorage::new();
        let key: StorageKey = "k".into();
        let body = Bytes::from_static(b"hi");
        let s256 = {
            let mut h = Sha256::new();
            h.update(&body);
            format!("{:x}", h.finalize())
        };
        s.put(&key, body.clone(), Some(&s256)).await.unwrap();
        let bad = s.put(&key, body, Some("deadbeef")).await.unwrap_err();
        assert!(matches!(bad, StorageError::HashMismatch { .. }));
    }

    #[tokio::test]
    async fn delete_is_idempotent() {
        let s = MemoryStorage::new();
        let key: StorageKey = "k".into();
        s.delete(&key).await.unwrap();
        s.delete(&key).await.unwrap();
        assert!(s.get(&key).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn presigned_get_url_returns_none() {
        let s = MemoryStorage::new();
        let key: StorageKey = "k".into();
        assert!(s.presigned_get_url(&key, 60).await.unwrap().is_none());
    }
}
