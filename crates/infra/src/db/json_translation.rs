//! JSON-file-backed `TranslationRepository` (M11).
//!
//! The dev / e2e / fallback store. Persists every override to a
//! single JSON file on `put`; reads are O(1) in-memory lookups.
//!
//! ## File format
//!
//! ```json
//! {
//!   "th": {
//!     "err_auth.invalid_credentials": "อีเมลหรือรหัสผ่านไม่ถูกต้อง",
//!     "err_repo.not_found": "ไม่พบ: {0}"
//!   },
//!   "lo": { ... }
//! }
//! ```
//!
//! ## Replacement plan
//!
//! M12+ will add `MssqlTranslationRepository` against a
//! `translation(locale, key, value, updated_at)` table. The
//! port (`kokkak_domain::traits::translation::TranslationRepository`)
//! is the only thing callers know about, so the swap is local
//! to `repo_factory::from_settings`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use kokkak_domain::traits::translation::{TranslationError, TranslationRepository};
use thiserror::Error;
use tokio::sync::RwLock;

/// Errors raised while opening or persisting the JSON store.
#[derive(Debug, Error)]
pub enum JsonTranslationError {
    /// IO / serialization failure.
    #[error("translation json store: {0}")]
    Io(#[from] std::io::Error),

    /// Serde failure.
    #[error("translation json store: codec error: {0}")]
    Codec(#[from] serde_json::Error),
}

impl From<JsonTranslationError> for TranslationError {
    fn from(e: JsonTranslationError) -> Self {
        TranslationError::Backend(e.to_string())
    }
}

type Store = HashMap<String, HashMap<String, String>>;

/// In-process translation override store, persisted to a single
/// JSON file on `put`. Cheap to clone — inner state is behind
/// `Arc<RwLock<...>>`.
#[derive(Clone)]
pub struct JsonTranslationRepository {
    path: PathBuf,
    state: Arc<RwLock<Store>>,
}

impl JsonTranslationRepository {
    /// Open a store backed by `path`. Creates the parent
    /// directory if missing; loads existing data when the file
    /// is present. A missing / empty file starts the store
    /// empty.
    pub async fn open(path: impl Into<PathBuf>) -> Result<Self, JsonTranslationError> {
        let path = path.into();
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let store: Store = if path.exists() {
            let bytes = tokio::fs::read(&path).await?;
            if bytes.is_empty() {
                Store::new()
            } else {
                serde_json::from_slice(&bytes)?
            }
        } else {
            Store::new()
        };
        Ok(Self {
            path,
            state: Arc::new(RwLock::new(store)),
        })
    }

    /// Build an in-memory store (no file backing). Useful for
    /// tests that don't need persistence.
    pub fn in_memory() -> Self {
        Self {
            path: PathBuf::new(),
            state: Arc::new(RwLock::new(Store::new())),
        }
    }

    /// Path to the underlying JSON file (empty for the
    /// in-memory variant).
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Total number of overrides across all locales.
    pub async fn len(&self) -> usize {
        let guard = self.state.read().await;
        guard.values().map(|m| m.len()).sum()
    }

    /// `true` iff no overrides are currently held.
    pub async fn is_empty(&self) -> bool {
        self.len().await == 0
    }

    /// Atomically persist the in-memory map to disk. No-op for
    /// the in-memory variant (`path == ""`).
    async fn persist(&self) -> Result<(), JsonTranslationError> {
        if self.path.as_os_str().is_empty() {
            return Ok(());
        }
        let snapshot = self.state.read().await.clone();
        let bytes = serde_json::to_vec_pretty(&snapshot)?;
        let tmp = self.path.with_extension("json.tmp");
        tokio::fs::write(&tmp, &bytes).await?;
        tokio::fs::rename(&tmp, &self.path).await?;
        Ok(())
    }
}

#[async_trait]
impl TranslationRepository for JsonTranslationRepository {
    async fn get(&self, locale: &str, key: &str) -> Result<Option<String>, TranslationError> {
        let guard = self.state.read().await;
        Ok(guard.get(locale).and_then(|m| m.get(key).cloned()))
    }

    async fn put(&self, locale: &str, key: &str, value: &str) -> Result<(), TranslationError> {
        {
            let mut guard = self.state.write().await;
            let bucket = guard.entry(locale.to_string()).or_default();
            bucket.insert(key.to_string(), value.to_string());
        }
        self.persist().await?;
        Ok(())
    }

    async fn count(&self) -> Result<usize, TranslationError> {
        Ok(self.len().await)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn tmp_path(name: &str) -> PathBuf {
        std::env::temp_dir()
            .join("kokkak_translation_test")
            .join(name)
    }

    #[tokio::test]
    async fn in_memory_starts_empty() {
        let repo = JsonTranslationRepository::in_memory();
        assert!(repo.is_empty().await);
        assert_eq!(repo.count().await.unwrap(), 0);
        assert!(repo.get("en", "err.x").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn put_then_get_round_trip() {
        let repo = JsonTranslationRepository::in_memory();
        repo.put("th", "err.x", "ทดสอบ").await.unwrap();
        assert_eq!(
            repo.get("th", "err.x").await.unwrap().as_deref(),
            Some("ทดสอบ")
        );
        // Locale isolation: missing locale returns None.
        assert!(repo.get("lo", "err.x").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn put_overwrites_value() {
        let repo = JsonTranslationRepository::in_memory();
        repo.put("en", "k", "v1").await.unwrap();
        repo.put("en", "k", "v2").await.unwrap();
        assert_eq!(repo.get("en", "k").await.unwrap().as_deref(), Some("v2"));
        assert_eq!(repo.count().await.unwrap(), 1);
    }

    #[tokio::test]
    async fn file_persistence_round_trip() {
        let path = tmp_path("persist.json");
        let _ = std::fs::remove_file(&path);
        // Phase 1: write.
        {
            let repo = JsonTranslationRepository::open(&path).await.unwrap();
            repo.put("th", "err.x", "ทดสอบ").await.unwrap();
        }
        // Phase 2: re-open and read.
        {
            let repo = JsonTranslationRepository::open(&path).await.unwrap();
            assert_eq!(
                repo.get("th", "err.x").await.unwrap().as_deref(),
                Some("ทดสอบ")
            );
            assert_eq!(repo.count().await.unwrap(), 1);
        }
    }

    #[tokio::test]
    async fn put_persists_immediately() {
        // After every `put`, the on-disk file must reflect the
        // change so a crash doesn't lose the override.
        let path = tmp_path("immediate.json");
        let _ = std::fs::remove_file(&path);
        let repo = JsonTranslationRepository::open(&path).await.unwrap();
        repo.put("en", "k", "v").await.unwrap();
        // Read the raw file from outside the lock.
        let bytes = std::fs::read(&path).unwrap();
        let v: Store = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v.get("en").unwrap().get("k").unwrap(), "v");
    }

    #[tokio::test]
    async fn missing_file_opens_as_empty() {
        let path = tmp_path("missing.json");
        let _ = std::fs::remove_file(&path);
        let repo = JsonTranslationRepository::open(&path).await.unwrap();
        assert!(repo.is_empty().await);
    }
}
