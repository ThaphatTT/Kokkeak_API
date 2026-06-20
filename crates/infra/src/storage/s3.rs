//! S3 / MinIO `Storage` adapter (M9).
//!
//! Uses the pure-Rust `rust-s3` crate (tokio + rustls) to
//! PUT/GET/DELETE blobs against any S3-compatible endpoint
//! (AWS, MinIO, Cloudflare R2, ...). `presigned_get_url` is
//! implemented natively by `rust-s3` (sync HMAC over the
//! canonical request) — the API does not need to proxy
//! downloads.
//!
//! The `Storage` port lives in `kokkak_domain::storage`. This
//! adapter is wired in `api::main` when
//! `KOKKAK_STORAGE__S3_BUCKET` is set (M9); otherwise the
//! dev-friendly `MemoryStorage` is used.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use kokkak_domain::{PutResult, Storage, StorageError, StorageKey};
use s3::creds::Credentials;
use s3::Bucket;
use sha2::{Digest, Sha256};
use thiserror::Error;

/// S3 connection settings (subset of the env-driven config
/// — see `AGENTS.md` § 7.1 / M9).
#[derive(Debug, Clone)]
pub struct S3Config {
    /// S3 endpoint URL (e.g. `https://s3.amazonaws.com`,
    /// `http://minio.local:9000`).
    pub endpoint: String,
    /// Region (`us-east-1` for MinIO).
    pub region: String,
    /// Bucket name.
    pub bucket: String,
    /// Access key id.
    pub access_key: String,
    /// Secret access key.
    pub secret_key: String,
    /// `true` for MinIO / non-AWS endpoints.
    pub path_style: bool,
}

impl S3Config {
    /// Build a `Bucket` handle from this config.
    pub fn bucket(&self) -> Result<Box<Bucket>, S3Error> {
        let region = s3::Region::Custom {
            region: self.region.clone(),
            endpoint: self.endpoint.clone(),
        };
        let creds = Credentials::new(
            Some(&self.access_key),
            Some(&self.secret_key),
            None,
            None,
            None,
        )
        .map_err(|e| S3Error::Config(e.to_string()))?;
        let b =
            Bucket::new(&self.bucket, region, creds).map_err(|e| S3Error::Config(e.to_string()))?;
        let b = if self.path_style {
            b.with_path_style()
        } else {
            b
        };
        Ok(Box::new(b))
    }
}

/// Errors raised by the S3 adapter.
#[derive(Debug, Error)]
pub enum S3Error {
    /// Configuration / startup failure.
    #[error("s3 config error: {0}")]
    Config(String),
    /// Underlying driver failure.
    #[error("s3 backend error: {0}")]
    Backend(String),
    /// Hash mismatch.
    #[error("s3 hash mismatch: expected {expected}, got {actual}")]
    HashMismatch {
        /// Digest the caller claimed (hex).
        expected: String,
        /// Digest computed from the bytes actually stored (hex).
        actual: String,
    },
}

impl From<S3Error> for StorageError {
    fn from(e: S3Error) -> Self {
        match e {
            S3Error::HashMismatch { expected, actual } => {
                StorageError::HashMismatch { expected, actual }
            }
            S3Error::Config(m) | S3Error::Backend(m) => StorageError::Backend(m),
        }
    }
}

/// S3-backed `Storage`.
#[derive(Clone)]
pub struct S3Storage {
    bucket: Arc<Bucket>,
}

impl S3Storage {
    /// Build from config.
    pub fn new(cfg: &S3Config) -> Result<Self, S3Error> {
        let bucket = cfg.bucket()?;
        Ok(Self {
            bucket: Arc::from(bucket),
        })
    }
}

#[async_trait]
impl Storage for S3Storage {
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
        let body = bytes.to_vec();
        let key_s = key.0.clone();
        let bucket = self.bucket.clone();
        bucket
            .put_object(&key_s, &body)
            .await
            .map_err(|e| StorageError::Backend(e.to_string()))?;
        Ok(PutResult {
            key: key.clone(),
            sha256: actual,
            size,
        })
    }

    async fn get(&self, key: &StorageKey) -> Result<Option<Bytes>, StorageError> {
        let key_s = key.0.clone();
        let bucket = self.bucket.clone();
        match bucket.get_object(&key_s).await {
            Ok(r) => Ok(Some(Bytes::from(r.bytes().to_vec()))),
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("404") || msg.to_lowercase().contains("nosuchkey") {
                    Ok(None)
                } else {
                    Err(StorageError::Backend(msg))
                }
            }
        }
    }

    async fn delete(&self, key: &StorageKey) -> Result<(), StorageError> {
        let key_s = key.0.clone();
        let bucket = self.bucket.clone();
        if let Err(e) = bucket.delete_object(&key_s).await {
            let msg = e.to_string();
            if !(msg.contains("404") || msg.to_lowercase().contains("nosuchkey")) {
                return Err(StorageError::Backend(msg));
            }
        }
        Ok(())
    }

    async fn presigned_get_url(
        &self,
        key: &StorageKey,
        ttl_secs: u32,
    ) -> Result<Option<String>, StorageError> {
        // `presign_get` is sync (HMAC over the canonical
        // request) and takes `&self`; cloning the Arc gives
        // us a stable handle.
        let bucket = self.bucket.clone();
        let key_s = key.0.clone();
        let extra: HashMap<String, String> = HashMap::new();
        let url = bucket
            .presign_get(&key_s, ttl_secs, Some(extra))
            .map_err(|e| StorageError::Backend(e.to_string()))?;
        Ok(Some(url))
    }
}
