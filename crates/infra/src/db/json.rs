//! Generic JSON-file-backed store (เก็บข้อมูลจำลองในไฟล์ JSON — M1.5 / M2 / M3).
//!
//! Acts as a placeholder for the real tiberius / MongoDB repositories
//! while we wire M2 / M3 use cases. Persists each entity collection
//! to a single JSON file in the configured data directory.
//!
//! ## Semantics
//!
//! - **In-memory** `Vec<T>` for reads; every mutation is **persisted
//!   atomically** to disk (write to `.tmp`, then rename).
//! - **Concurrency**: a single `tokio::sync::Mutex` guards the
//!   in-memory state. Reads are O(n) over the map but the JSON-DB is
//!   for dev / smoke tests; production uses SQL Server + MongoDB.
//! - **Indexing**: the caller provides a `key(&T) -> String` extractor
//!   at `open` time. We maintain a `HashMap<String, usize>` mapping
//!   `key -> position` for O(1) lookups and uniqueness checks.
//!
//! ## Replacement plan (M5+)
//!
//! `JsonStore<T>` is the only place in `infra` that knows the JSON
//! layout. Swap it with `MssqlUserRepository`, `MongoChatRepository`
//! etc. without touching `application` / `api`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{de::DeserializeOwned, Serialize};
use thiserror::Error;
use tokio::sync::Mutex;

#[derive(Debug, Error)]
pub enum JsonStoreError {
    /// Serialization / deserialization failure.
    #[error("codec error: {0}")]
    Codec(String),

    /// Underlying IO failure.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// Atomic write failed (rename etc.).
    #[error("write error: {0}")]
    Write(String),
}

impl From<serde_json::Error> for JsonStoreError {
    fn from(err: serde_json::Error) -> Self {
        Self::Codec(err.to_string())
    }
}

type KeyFn<T> = Arc<dyn Fn(&T) -> String + Send + Sync>;

#[derive()]
struct Inner<T> {
    items: Vec<T>,
    index: HashMap<String, usize>,
    key_fn: KeyFn<T>,
}

impl<T> Inner<T> {
    fn rebuild_index(&mut self) {
        self.index.clear();
        for (i, item) in self.items.iter().enumerate() {
            let k = (self.key_fn)(item);
            self.index.insert(k, i);
        }
    }
}

/// JSON file-backed store (generic over the entity type `T`).
///
/// Cheap to clone — the inner state is wrapped in `Arc<Mutex<...>>`.
#[derive(Clone)]
pub struct JsonStore<T> {
    path: PathBuf,
    state: Arc<Mutex<Inner<T>>>,
}

impl<T> JsonStore<T>
where
    T: Serialize + DeserializeOwned + Clone + 'static,
{
    /// Build a store backed by `path`. Creates the parent directory if
    /// missing and loads existing data when the file is present.
    ///
    /// `key_fn` extracts the stable key (id, code, email, ...) used
    /// for lookups and uniqueness checks.
    pub async fn open<F>(path: impl Into<PathBuf>, key_fn: F) -> Result<Self, JsonStoreError>
    where
        F: Fn(&T) -> String + Send + Sync + 'static,
    {
        let path = path.into();
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let key_fn: KeyFn<T> = Arc::new(key_fn);
        let mut inner = if path.exists() {
            let bytes = tokio::fs::read(&path).await?;
            if bytes.is_empty() {
                Inner {
                    items: Vec::new(),
                    index: HashMap::new(),
                    key_fn: key_fn.clone(),
                }
            } else {
                let items: Vec<T> = serde_json::from_slice(&bytes)?;
                let mut index = HashMap::with_capacity(items.len());
                for (i, item) in items.iter().enumerate() {
                    let k = key_fn(item);
                    index.insert(k, i);
                }
                Inner {
                    items,
                    index,
                    key_fn: key_fn.clone(),
                }
            }
        } else {
            Inner {
                items: Vec::new(),
                index: HashMap::new(),
                key_fn: key_fn.clone(),
            }
        };
        // Defensive rebuild in case of duplicate keys.
        inner.rebuild_index();
        Ok(Self {
            path,
            state: Arc::new(Mutex::new(inner)),
        })
    }

    /// Path to the underlying JSON file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Number of items in the store.
    pub async fn len(&self) -> usize {
        self.state.lock().await.items.len()
    }

    /// `true` iff the store is empty.
    pub async fn is_empty(&self) -> bool {
        self.state.lock().await.items.is_empty()
    }

    /// Read-only access to the items (caller iterates a snapshot).
    pub async fn snapshot(&self) -> Vec<T> {
        self.state.lock().await.items.clone()
    }

    /// Insert (or replace by key). Returns `true` if a row was already
    /// present (i.e. replaced).
    pub async fn upsert(&self, item: &T) -> Result<bool, JsonStoreError> {
        let mut guard = self.state.lock().await;
        let key = (guard.key_fn)(item);
        if let Some(&i) = guard.index.get(&key) {
            guard.items[i] = item.clone();
            Self::persist(&self.path, &guard.items).await?;
            Ok(true)
        } else {
            guard.items.push(item.clone());
            let i = guard.items.len() - 1;
            guard.index.insert(key, i);
            Self::persist(&self.path, &guard.items).await?;
            Ok(false)
        }
    }

    /// Look up by key.
    pub async fn find(&self, key: &str) -> Option<T> {
        let guard = self.state.lock().await;
        let i = *guard.index.get(key)?;
        guard.items.get(i).cloned()
    }

    /// Find by predicate.
    pub async fn find_by<F>(&self, predicate: F) -> Option<T>
    where
        F: Fn(&T) -> bool,
    {
        let guard = self.state.lock().await;
        guard.items.iter().find(|x| predicate(x)).cloned()
    }

    /// Filter + sort.
    pub async fn filter<F, S>(&self, predicate: F, sorter: S) -> Vec<T>
    where
        F: Fn(&T) -> bool,
        S: Fn(&T, &T) -> std::cmp::Ordering,
    {
        let guard = self.state.lock().await;
        let mut out: Vec<T> = guard
            .items
            .iter()
            .filter(|x| predicate(x))
            .cloned()
            .collect();
        out.sort_by(sorter);
        out
    }

    /// Remove by key. Returns `true` if a row was removed.
    pub async fn remove(&self, key: &str) -> Result<bool, JsonStoreError> {
        let mut guard = self.state.lock().await;
        let Some(&i) = guard.index.get(key) else {
            return Ok(false);
        };
        guard.items.remove(i);
        guard.rebuild_index();
        Self::persist(&self.path, &guard.items).await?;
        Ok(true)
    }

    /// Returns `true` if the key is already present.
    pub async fn contains_key(&self, key: &str) -> bool {
        self.state.lock().await.index.contains_key(key)
    }

    /// Atomically write the items array to disk (write to .tmp, rename).
    async fn persist(path: &Path, items: &[T]) -> Result<(), JsonStoreError> {
        let tmp = path.with_extension("json.tmp");
        let bytes = serde_json::to_vec_pretty(items)?;
        tokio::fs::write(&tmp, &bytes)
            .await
            .map_err(|e| JsonStoreError::Write(e.to_string()))?;
        // rename is atomic on POSIX; on Windows it is best-effort.
        // For the JSON-DB simulation that's good enough.
        tokio::fs::rename(&tmp, path)
            .await
            .map_err(|e| JsonStoreError::Write(e.to_string()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    struct Item {
        id: String,
        value: i32,
    }

    fn key_fn(i: &Item) -> String {
        i.id.clone()
    }

    fn tmp_path(name: &str) -> PathBuf {
        std::env::temp_dir().join("kokkak_json_test").join(name)
    }

    #[tokio::test]
    async fn open_creates_parent_dir_and_empty_store() {
        let path = tmp_path("a.json");
        let _ = std::fs::remove_file(&path);
        let store: JsonStore<Item> = JsonStore::open(&path, key_fn).await.unwrap();
        assert!(store.is_empty().await);
        assert_eq!(store.len().await, 0);
    }

    #[tokio::test]
    async fn upsert_inserts_and_replaces() {
        let path = tmp_path("b.json");
        let _ = std::fs::remove_file(&path);
        let store: JsonStore<Item> = JsonStore::open(&path, key_fn).await.unwrap();
        // First insert: false (new).
        assert!(!store
            .upsert(&Item {
                id: "x".into(),
                value: 1
            })
            .await
            .unwrap());
        // Re-insert same key: true (replaced).
        assert!(store
            .upsert(&Item {
                id: "x".into(),
                value: 2
            })
            .await
            .unwrap());
        let got = store.find("x").await.unwrap();
        assert_eq!(got.value, 2);
        assert_eq!(store.len().await, 1);
    }

    #[tokio::test]
    async fn remove_drops_entry_and_rebuilds_index() {
        let path = tmp_path("c.json");
        let _ = std::fs::remove_file(&path);
        let store: JsonStore<Item> = JsonStore::open(&path, key_fn).await.unwrap();
        store
            .upsert(&Item {
                id: "a".into(),
                value: 1,
            })
            .await
            .unwrap();
        store
            .upsert(&Item {
                id: "b".into(),
                value: 2,
            })
            .await
            .unwrap();
        store
            .upsert(&Item {
                id: "c".into(),
                value: 3,
            })
            .await
            .unwrap();
        // Remove middle entry to force index rebuild.
        assert!(store.remove("b").await.unwrap());
        assert!(store.find("b").await.is_none());
        assert_eq!(store.len().await, 2);
        // Make sure remaining keys are still findable.
        assert!(store.find("a").await.is_some());
        assert!(store.find("c").await.is_some());
    }

    #[tokio::test]
    async fn persist_writes_readable_file() {
        let path = tmp_path("d.json");
        let _ = std::fs::remove_file(&path);
        let store: JsonStore<Item> = JsonStore::open(&path, key_fn).await.unwrap();
        store
            .upsert(&Item {
                id: "x".into(),
                value: 42,
            })
            .await
            .unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let items: Vec<Item> = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, "x");
    }

    #[tokio::test]
    async fn reload_after_restart_sees_persisted_items() {
        let path = tmp_path("e.json");
        let _ = std::fs::remove_file(&path);
        // Phase 1: open + upsert.
        {
            let store: JsonStore<Item> = JsonStore::open(&path, key_fn).await.unwrap();
            store
                .upsert(&Item {
                    id: "x".into(),
                    value: 1,
                })
                .await
                .unwrap();
        }
        // Phase 2: reopen + look up.
        let store2: JsonStore<Item> = JsonStore::open(&path, key_fn).await.unwrap();
        let got = store2.find("x").await.unwrap();
        assert_eq!(got.value, 1);
    }

    #[tokio::test]
    async fn filter_and_sort_work() {
        let path = tmp_path("f.json");
        let _ = std::fs::remove_file(&path);
        let store: JsonStore<Item> = JsonStore::open(&path, key_fn).await.unwrap();
        store
            .upsert(&Item {
                id: "a".into(),
                value: 3,
            })
            .await
            .unwrap();
        store
            .upsert(&Item {
                id: "b".into(),
                value: 1,
            })
            .await
            .unwrap();
        store
            .upsert(&Item {
                id: "c".into(),
                value: 2,
            })
            .await
            .unwrap();
        let sorted: Vec<Item> = store.filter(|_| true, |x, y| x.value.cmp(&y.value)).await;
        assert_eq!(
            sorted.iter().map(|i| i.value).collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
    }
}
