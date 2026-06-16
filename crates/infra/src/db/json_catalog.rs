//! JSON-file-backed `ServiceRepository` (M3).

use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use kokkak_domain::{Cursor, RepoError, ServiceCategory, ServiceRepository};
use uuid::Uuid;

use crate::db::json::JsonStore;

#[derive(Clone)]
pub struct JsonServiceRepository {
    store: Arc<JsonStore<ServiceCategory>>,
}

impl JsonServiceRepository {
    pub async fn open(path: impl AsRef<Path>) -> Result<Self, RepoError> {
        let store = JsonStore::open(path.as_ref(), |s: &ServiceCategory| s.id.to_string())
            .await
            .map_err(|e| RepoError::Backend(e.to_string()))?;
        Ok(Self {
            store: Arc::new(store),
        })
    }
}

#[async_trait]
impl ServiceRepository for JsonServiceRepository {
    async fn find_by_id(&self, id: Uuid) -> Result<Option<ServiceCategory>, RepoError> {
        Ok(self.store.find(&id.to_string()).await)
    }

    async fn find_by_code(&self, code: &str) -> Result<Option<ServiceCategory>, RepoError> {
        Ok(self.store.find_by(|s| s.code == code).await)
    }

    async fn list_active(
        &self,
        after: Option<Cursor>,
        limit: u32,
    ) -> Result<Vec<ServiceCategory>, RepoError> {
        // Decode the cursor (sort_order of the last item in the
        // previous page).
        let after_sort = match after {
            Some(c) => Some(decode_cursor(&c)?),
            None => None,
        };
        let limit = limit.clamp(1, 200) as usize;
        // We can't filter inside `filter` (no `?` across closures), so
        // we pull a snapshot and process in-line. Still O(n) but
        // bounded by the active-category count (small).
        let snap = self.store.snapshot().await;
        let mut out: Vec<ServiceCategory> = snap
            .into_iter()
            .filter(|s| s.active)
            .filter(|s| match after_sort {
                Some(off) => s.sort_order > off,
                None => true,
            })
            .collect();
        out.sort_by_key(|s| s.sort_order);
        out.truncate(limit);
        Ok(out)
    }

    async fn insert(&self, service: &ServiceCategory) -> Result<(), RepoError> {
        if self.store.contains_key(&service.id.to_string()).await {
            return Err(RepoError::Conflict("id exists".into()));
        }
        if self
            .store
            .find_by(|s| s.code == service.code)
            .await
            .is_some()
        {
            return Err(RepoError::Conflict(format!(
                "code {} is already taken",
                service.code
            )));
        }
        self.store
            .upsert(service)
            .await
            .map_err(|e| RepoError::Backend(e.to_string()))?;
        Ok(())
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct CursorPayload {
    after_sort: i32,
}

fn decode_cursor(c: &Cursor) -> Result<i32, RepoError> {
    let p: CursorPayload = c
        .decode()
        .map_err(|e| RepoError::Backend(format!("invalid cursor: {e}")))?;
    Ok(p.after_sort)
}

/// Build a cursor for the next page given the last `sort_order` value.
pub fn encode_cursor(after_sort: i32) -> Result<Cursor, RepoError> {
    Cursor::encode(&CursorPayload { after_sort })
        .map_err(|e| RepoError::Backend(format!("cursor encode: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use std::str::FromStr;

    fn tmp(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join("kokkak_svc_repo_test").join(name)
    }

    fn sample(code: &str, sort_order: i32, active: bool) -> ServiceCategory {
        ServiceCategory {
            id: Uuid::new_v4(),
            code: code.into(),
            default_price: Some(rust_decimal::Decimal::from_str("100.00").unwrap()),
            warranty_days: 30,
            active,
            sort_order,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[tokio::test]
    async fn find_by_code() {
        let path = tmp("s1.json");
        let _ = std::fs::remove_file(&path);
        let repo = JsonServiceRepository::open(&path).await.unwrap();
        let s = sample("ac", 1, true);
        repo.insert(&s).await.unwrap();
        let got = repo.find_by_code("ac").await.unwrap().unwrap();
        assert_eq!(got.id, s.id);
    }

    #[tokio::test]
    async fn list_active_sorts_by_order_and_paginates() {
        let path = tmp("s2.json");
        let _ = std::fs::remove_file(&path);
        let repo = JsonServiceRepository::open(&path).await.unwrap();
        repo.insert(&sample("a", 30, true)).await.unwrap();
        repo.insert(&sample("b", 10, true)).await.unwrap();
        repo.insert(&sample("c", 20, true)).await.unwrap();
        repo.insert(&sample("d", 40, false)).await.unwrap(); // inactive

        let first = repo.list_active(None, 2).await.unwrap();
        assert_eq!(first.iter().map(|s| s.code.as_str()).collect::<Vec<_>>(), vec!["b", "c"]);

        let cursor = encode_cursor(first.last().unwrap().sort_order).unwrap();
        let second = repo.list_active(Some(cursor), 2).await.unwrap();
        assert_eq!(
            second.iter().map(|s| s.code.as_str()).collect::<Vec<_>>(),
            vec!["a"]
        );
    }

    #[tokio::test]
    async fn duplicate_code_returns_conflict() {
        let path = tmp("s3.json");
        let _ = std::fs::remove_file(&path);
        let repo = JsonServiceRepository::open(&path).await.unwrap();
        let s = sample("ac", 1, true);
        repo.insert(&s).await.unwrap();
        let mut s2 = sample("ac", 2, true);
        s2.id = Uuid::new_v4();
        let err = repo.insert(&s2).await.unwrap_err();
        assert!(matches!(err, RepoError::Conflict(_)));
    }
}
