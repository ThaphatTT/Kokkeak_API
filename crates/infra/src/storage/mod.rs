

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

#[derive(Debug, Error)]
pub enum BuildStorageError {

    #[error("local storage init failed: {0}")]
    Local(String),

    #[error("s3 storage init failed: {0}")]
    S3(String),
}

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
