

use std::path::{Component, Path, PathBuf};

use async_trait::async_trait;
use bytes::Bytes;
use kokkak_domain::{PutResult, Storage, StorageError, StorageKey};
use sha2::{Digest, Sha256};
use thiserror::Error;
use tokio::fs;
use tokio::io::AsyncWriteExt;

#[derive(Debug, Clone)]
pub struct LocalConfig {

    pub root: PathBuf,
}

impl LocalConfig {

    pub fn new(root: impl Into<PathBuf>) -> Result<Self, LocalError> {
        let root = root.into();
        if root.as_os_str().is_empty() {
            return Err(LocalError::Config("root path must not be empty".into()));
        }
        Ok(Self { root })
    }
}

#[derive(Debug, Error)]
pub enum LocalError {

    #[error("local storage config error: {0}")]
    Config(String),

    #[error("local storage backend error: {0}")]
    Backend(String),

    #[error("invalid storage key `{0}`: must be a relative path with no `..` segments")]
    InvalidKey(String),
}

impl From<LocalError> for StorageError {
    fn from(e: LocalError) -> Self {
        match e {
            LocalError::Config(m) | LocalError::Backend(m) => StorageError::Backend(m),
            LocalError::InvalidKey(m) => StorageError::Backend(m),
        }
    }
}

#[derive(Clone)]
pub struct LocalStorage {
    root: PathBuf,
}

impl LocalStorage {

    pub async fn new(cfg: &LocalConfig) -> Result<Self, LocalError> {
        fs::create_dir_all(&cfg.root)
            .await
            .map_err(|e| LocalError::Backend(format!("create root {}: {e}", cfg.root.display())))?;
        Ok(Self {
            root: cfg.root.clone(),
        })
    }

    fn resolve(&self, key: &StorageKey) -> Result<PathBuf, LocalError> {
        let raw = key.as_str();
        if raw.is_empty() {
            return Err(LocalError::InvalidKey("<empty>".into()));
        }
        let p = Path::new(raw);
        if p.is_absolute() {
            return Err(LocalError::InvalidKey(raw.to_string()));
        }
        for c in p.components() {
            match c {
                Component::ParentDir => return Err(LocalError::InvalidKey(raw.to_string())),
                Component::Prefix(_) | Component::RootDir => {
                    return Err(LocalError::InvalidKey(raw.to_string()));
                }
                _ => {}
            }
        }
        Ok(self.root.join(p))
    }
}

#[async_trait]
impl Storage for LocalStorage {
    async fn put(
        &self,
        key: &StorageKey,
        bytes: Bytes,
        expected_sha256: Option<&str>,
    ) -> Result<PutResult, StorageError> {
        let abs = self.resolve(key).map_err(StorageError::from)?;
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
        if let Some(parent) = abs.parent() {
            fs::create_dir_all(parent).await.map_err(|e| {
                StorageError::Backend(format!("create parent {}: {e}", parent.display()))
            })?;
        }

        let tmp = abs.with_extension(format!(
            "{}.tmp",
            abs.extension().and_then(|e| e.to_str()).unwrap_or("part")
        ));
        {
            let mut f = fs::File::create(&tmp)
                .await
                .map_err(|e| StorageError::Backend(format!("create {}: {e}", tmp.display())))?;
            f.write_all(&bytes)
                .await
                .map_err(|e| StorageError::Backend(format!("write {}: {e}", tmp.display())))?;
            f.sync_all()
                .await
                .map_err(|e| StorageError::Backend(format!("sync {}: {e}", tmp.display())))?;
        }
        fs::rename(&tmp, &abs)
            .await
            .map_err(|e| StorageError::Backend(format!("rename {}: {e}", abs.display())))?;
        Ok(PutResult {
            key: key.clone(),
            sha256: actual,
            size,
        })
    }

    async fn get(&self, key: &StorageKey) -> Result<Option<Bytes>, StorageError> {
        let abs = self.resolve(key).map_err(StorageError::from)?;
        match fs::read(&abs).await {
            Ok(v) => Ok(Some(Bytes::from(v))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(StorageError::Backend(format!(
                "read {}: {e}",
                abs.display()
            ))),
        }
    }

    async fn delete(&self, key: &StorageKey) -> Result<(), StorageError> {
        let abs = self.resolve(key).map_err(StorageError::from)?;
        match fs::remove_file(&abs).await {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(StorageError::Backend(format!(
                "delete {}: {e}",
                abs.display()
            ))),
        }
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

    async fn tmp_root(label: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "kokkak-local-storage-{label}-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&p).await.unwrap();
        p
    }

    #[tokio::test]
    async fn put_get_round_trip() {
        let root = tmp_root("round-trip").await;
        let s = LocalStorage::new(&LocalConfig::new(root.clone()).unwrap())
            .await
            .unwrap();
        let key: StorageKey = "users/abc/profile/uuid.jpg".into();
        let body = Bytes::from_static(b"hello world");
        let r = s.put(&key, body.clone(), None).await.unwrap();
        assert_eq!(r.size, 11);
        assert_eq!(r.key.as_str(), "users/abc/profile/uuid.jpg");
        assert!(root.join("users/abc/profile/uuid.jpg").is_file());
        let got = s.get(&key).await.unwrap().unwrap();
        assert_eq!(got, body);
    }

    #[tokio::test]
    async fn put_creates_intermediate_dirs() {
        let root = tmp_root("mkdir").await;
        let s = LocalStorage::new(&LocalConfig::new(root.clone()).unwrap())
            .await
            .unwrap();
        let key: StorageKey = "users/abc/attachments/id-card-front/deadbeef.jpg".into();
        s.put(&key, Bytes::from_static(b"x"), None).await.unwrap();
        assert!(root
            .join("users/abc/attachments/id-card-front/deadbeef.jpg")
            .is_file());
    }

    #[tokio::test]
    async fn put_verifies_expected_sha256() {
        let root = tmp_root("sha").await;
        let s = LocalStorage::new(&LocalConfig::new(root).unwrap())
            .await
            .unwrap();
        let key: StorageKey = "k".into();
        let body = Bytes::from_static(b"hi");
        let s256 = {
            let mut h = Sha256::new();
            h.update(&body);
            format!("{:x}", h.finalize())
        };
        s.put(&key, body.clone(), Some(&s256)).await.unwrap();
        let bad = s
            .put(
                &key,
                body,
                Some("0000000000000000000000000000000000000000000000000000000000000000"),
            )
            .await
            .unwrap_err();
        assert!(matches!(bad, StorageError::HashMismatch { .. }));
    }

    #[tokio::test]
    async fn delete_is_idempotent() {
        let root = tmp_root("delete").await;
        let s = LocalStorage::new(&LocalConfig::new(root).unwrap())
            .await
            .unwrap();
        let key: StorageKey = "users/abc/profile/missing.jpg".into();
        s.delete(&key).await.unwrap();
        s.delete(&key).await.unwrap();
        assert!(s.get(&key).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn rejects_path_traversal() {
        let root = tmp_root("traversal").await;
        let s = LocalStorage::new(&LocalConfig::new(root).unwrap())
            .await
            .unwrap();
        for bad in [
            "../etc/passwd",
            "users/../../escape",
            "/abs/path.jpg",
            "users/abc/../../../etc",
        ] {
            let key: StorageKey = bad.into();
            let r = s.put(&key, Bytes::from_static(b"x"), None).await;
            assert!(r.is_err(), "expected reject for key `{bad}`");
        }
    }

    #[tokio::test]
    async fn rejects_empty_key() {
        let root = tmp_root("empty").await;
        let s = LocalStorage::new(&LocalConfig::new(root).unwrap())
            .await
            .unwrap();
        let key = StorageKey(String::new());
        let r = s.put(&key, Bytes::from_static(b"x"), None).await;
        assert!(r.is_err());
    }

    #[tokio::test]
    async fn rejects_empty_root() {
        let r = LocalConfig::new("").unwrap_err();
        assert!(matches!(r, LocalError::Config(_)));
    }

    #[tokio::test]
    async fn presigned_returns_none() {
        let root = tmp_root("presign").await;
        let s = LocalStorage::new(&LocalConfig::new(root).unwrap())
            .await
            .unwrap();
        let key: StorageKey = "users/abc/profile/uuid.jpg".into();
        assert!(s.presigned_get_url(&key, 60).await.unwrap().is_none());
    }
}
