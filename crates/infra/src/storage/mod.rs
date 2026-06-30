//! Object-storage adapters (M9).
//!
//! Three adapters satisfy [`kokkak_domain::Storage`]:
//!
//! - [`memory::MemoryStorage`] — in-process `HashMap`. Used in
//!   dev / tests; no presigned URLs (returns `None`).
//! - [`s3::S3Storage`] — S3 / S3-compatible (MinIO) via
//!   `rust-s3`. Presigned `GetObject` URLs are generated
//!   client-side (HMAC-SHA1 over the canonical request).
//! - [`local::LocalStorage`] — local filesystem. Used during the
//!   Strangler transition and for ad-hoc dev runs without
//!   MinIO. `presigned_get_url` returns `None`; callers must
//!   already have a path to read.
//!
//! The `Storage` port lives in `kokkak_domain::storage`; the
//! application layer is oblivious to the concrete adapter. The
//! factory [`build_from_settings`] picks the adapter from the
//! [`StorageSettings`] env knobs (S3 wins, then local, then memory).
//!
//! [`build_from_settings`]: build_from_settings
//! [`StorageSettings`]: kokkak_common::config::StorageSettings

pub mod keys;
pub mod local;
pub mod memory;
pub mod s3;

pub use keys::{user_attachment, user_bank_book, user_profile, UserAttachment};
pub use local::{LocalConfig, LocalError, LocalStorage};
pub use memory::MemoryStorage;
pub use s3::{S3Config, S3Error, S3Storage};

use std::sync::Arc;

use kokkak_common::config::{StorageAdapterKind, StorageSettings};
use kokkak_domain::Storage;
use thiserror::Error;

/// Errors raised by [`build_from_settings`].
#[derive(Debug, Error)]
pub enum BuildStorageError {
    /// The local root was set but the directory could not be
    /// created (permission denied, bad path, ...).
    #[error("local storage init failed: {0}")]
    Local(String),
    /// S3 was selected but config is missing or invalid
    /// (e.g. access key without secret, or vice versa).
    #[error("s3 storage init failed: {0}")]
    S3(String),
}

/// Build the `Storage` adapter the rest of the app sees.
///
/// Selection rule (matches `StorageSettings::adapter_kind`):
/// 1. `KOKKAK_STORAGE__S3_BUCKET` set → `S3Storage` (production).
/// 2. `KOKKAK_STORAGE__LOCAL_PATH` set → `LocalStorage` (Strangler
///    transition + dev without MinIO).
/// 3. Otherwise → `MemoryStorage` (non-persistent; tests only).
///
/// ponytail: the function is `async` because `LocalStorage::new`
/// runs `create_dir_all`. S3's `S3Storage::new` is sync; memory
/// is sync. Keeping the signature uniform avoids branch-on-result
/// at the call site.
pub async fn build_from_settings(
    cfg: &StorageSettings,
) -> Result<(Arc<dyn Storage>, StorageAdapterKind), BuildStorageError> {
    match cfg.adapter_kind() {
        StorageAdapterKind::S3 => {
            let s3 = S3Config {
                endpoint: cfg.s3_endpoint.clone(),
                region: cfg.s3_region.clone(),
                bucket: cfg.s3_bucket.clone(),
                access_key: cfg.s3_access_key.clone(),
                secret_key: cfg.s3_secret_key.clone(),
                path_style: cfg.s3_path_style,
            };
            let storage = S3Storage::new(&s3).map_err(|e| BuildStorageError::S3(e.to_string()))?;
            Ok((Arc::new(storage), StorageAdapterKind::S3))
        }
        StorageAdapterKind::Local => {
            let local = LocalConfig::new(cfg.local_path.clone())
                .map_err(|e| BuildStorageError::Local(e.to_string()))?;
            let storage = LocalStorage::new(&local)
                .await
                .map_err(|e| BuildStorageError::Local(e.to_string()))?;
            Ok((Arc::new(storage), StorageAdapterKind::Local))
        }
        StorageAdapterKind::Memory => {
            Ok((Arc::new(MemoryStorage::new()), StorageAdapterKind::Memory))
        }
    }
}
