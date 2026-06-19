//! Catalog use cases (M3).
//!
//! Read-mostly: list active service categories with keyset pagination.

use std::sync::Arc;

use kokkak_domain::{Cursor, RepoError, ServiceCategory, ServiceRepository};

/// One page of active service categories.
#[derive(Debug, Clone)]
pub struct ServiceListPage {
    pub items: Vec<ServiceCategory>,
    pub next_cursor: Option<String>,
}

pub struct CatalogService {
    services: Arc<dyn ServiceRepository>,
}

impl CatalogService {
    pub fn new(services: Arc<dyn ServiceRepository>) -> Self {
        Self { services }
    }

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

    pub async fn find_by_code(&self, code: &str) -> Result<Option<ServiceCategory>, RepoError> {
        self.services.find_by_code(code).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use kokkak_infra::db::json_catalog::JsonServiceRepository;
    use rust_decimal::Decimal;
    use std::path::PathBuf;
    use std::str::FromStr;
    use uuid::Uuid;

    fn svc_path() -> PathBuf {
        let p = std::env::temp_dir()
            .join("kokkak_catalog_test")
            .join(format!("c-{}.json", Uuid::new_v4()));
        let _ = std::fs::create_dir_all(p.parent().unwrap());
        let _ = std::fs::remove_file(&p);
        p
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
        let repo = JsonServiceRepository::open(svc_path()).await.unwrap();
        repo.insert(&sample("a", 10)).await.unwrap();
        repo.insert(&sample("b", 20)).await.unwrap();
        repo.insert(&sample("c", 30)).await.unwrap();
        let svc = CatalogService::new(Arc::new(repo));
        let page = svc.list_active(None, 2).await.unwrap();
        assert_eq!(page.items.len(), 2);
        assert!(page.next_cursor.is_some());
        let page2 = svc.list_active(page.next_cursor.clone(), 2).await.unwrap();
        assert_eq!(page2.items.len(), 1);
        assert!(page2.next_cursor.is_none());
    }
}
