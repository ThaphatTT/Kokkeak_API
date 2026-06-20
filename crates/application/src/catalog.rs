//! Catalog use cases (M3).
//!
//! Read-mostly: list active service categories with keyset pagination.

use std::sync::Arc;

use kokkak_domain::{Cursor, RepoError, ServiceCategory, ServiceRepository};

/// One page of active service categories.
#[derive(Debug, Clone)]
pub struct ServiceListPage {
    /// Service categories in this page (sorted by `sort_order`).
    pub items: Vec<ServiceCategory>,
    /// Cursor for the next page; `None` when this is the last page.
    pub next_cursor: Option<String>,
}

/// Catalog use case bundle (M3 — read-mostly).
pub struct CatalogService {
    services: Arc<dyn ServiceRepository>,
}

impl CatalogService {
    /// Construct the service with a `ServiceRepository` port.
    pub fn new(services: Arc<dyn ServiceRepository>) -> Self {
        Self { services }
    }

    /// List active service categories with keyset pagination on `sort_order`.
    pub async fn list_active(
        &self,
        after: Option<String>,
        limit: u32,
    ) -> Result<ServiceListPage, RepoError> {
        let cursor = match after {
            Some(s) => Some(
                s.parse::<Cursor>()
                    .map_err(|e| RepoError::Backend(format!("invalid cursor: {e}")))?,
            ),
            None => None,
        };
        let limit = limit.clamp(1, 200);
        let items = self.services.list_active(cursor, limit).await?;
        // Build a next cursor from the last item's sort_order.
        let next_cursor = if (items.len() as u32) == limit {
            items.last().map(|i| {
                let payload = serde_json::json!({ "after_sort": i.sort_order });
                Cursor::encode(&payload)
                    .map(|c| c.to_string())
                    .unwrap_or_default()
            })
        } else {
            None
        };
        Ok(ServiceListPage { items, next_cursor })
    }

    /// Look up a single service category by its short code (e.g. `"AC_REPAIR"`).
    pub async fn find_by_code(&self, code: &str) -> Result<Option<ServiceCategory>, RepoError> {
        self.services.find_by_code(code).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use kokkak_domain::Cursor;
    use rust_decimal::Decimal;
    use std::collections::HashMap;
    use std::str::FromStr;
    use uuid::Uuid;

    /// In-memory mock of [`ServiceRepository`] for unit tests.
    ///
    /// ponytail: HashMap-backed, no async runtime — just enough for the
    /// pagination + insert tests in this file. Ceiling: doesn't model the
    /// `active=false` filter exhaustively (we always insert active
    /// samples); extend the predicate when a future test needs it.
    #[derive(Default)]
    struct MockServiceRepository {
        by_id: std::sync::Mutex<HashMap<Uuid, ServiceCategory>>,
        by_code: std::sync::Mutex<HashMap<String, Uuid>>,
    }

    #[async_trait::async_trait]
    impl ServiceRepository for MockServiceRepository {
        async fn find_by_id(&self, id: Uuid) -> Result<Option<ServiceCategory>, RepoError> {
            Ok(self.by_id.lock().unwrap().get(&id).cloned())
        }
        async fn find_by_code(&self, code: &str) -> Result<Option<ServiceCategory>, RepoError> {
            let by_code = self.by_code.lock().unwrap();
            let by_id = self.by_id.lock().unwrap();
            Ok(by_code.get(code).and_then(|id| by_id.get(id).cloned()))
        }
        async fn list_active(
            &self,
            after: Option<Cursor>,
            limit: u32,
        ) -> Result<Vec<ServiceCategory>, RepoError> {
            let by_id = self.by_id.lock().unwrap();
            let mut items: Vec<ServiceCategory> =
                by_id.values().filter(|s| s.active).cloned().collect();
            items.sort_by_key(|i| i.sort_order);
            if let Some(cursor) = after {
                if let Ok(payload) = cursor.decode::<serde_json::Value>() {
                    if let Some(n) = payload.get("after_sort").and_then(|v| v.as_i64()) {
                        items.retain(|i| (i.sort_order as i64) > n);
                    }
                }
            }
            items.truncate(limit as usize);
            Ok(items)
        }
        async fn insert(&self, service: &ServiceCategory) -> Result<(), RepoError> {
            let mut by_id = self.by_id.lock().unwrap();
            let mut by_code = self.by_code.lock().unwrap();
            if by_code.contains_key(&service.code) {
                return Err(RepoError::Conflict(format!("code {} taken", service.code)));
            }
            by_code.insert(service.code.clone(), service.id);
            by_id.insert(service.id, service.clone());
            Ok(())
        }
    }

    fn sample(code: &str, sort: i32) -> ServiceCategory {
        ServiceCategory {
            id: Uuid::new_v4(),
            code: code.into(),
            default_price: Some(Decimal::from_str("100.00").unwrap()),
            warranty_days: 30,
            active: true,
            sort_order: sort,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[tokio::test]
    async fn list_active_with_pagination() {
        let repo: Arc<dyn ServiceRepository> = Arc::new(MockServiceRepository::default());
        repo.insert(&sample("a", 10)).await.unwrap();
        repo.insert(&sample("b", 20)).await.unwrap();
        repo.insert(&sample("c", 30)).await.unwrap();
        let svc = CatalogService::new(repo);
        let page = svc.list_active(None, 2).await.unwrap();
        assert_eq!(page.items.len(), 2);
        assert!(page.next_cursor.is_some());
        let page2 = svc.list_active(page.next_cursor.clone(), 2).await.unwrap();
        assert_eq!(page2.items.len(), 1);
        assert!(page2.next_cursor.is_none());
    }
}
