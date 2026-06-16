//! JSON-file-backed `OrderRepository` (M3).

use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use kokkak_domain::{Cursor, Order, OrderRepository, RepoError};
use uuid::Uuid;

use crate::db::json::JsonStore;

#[derive(Clone)]
pub struct JsonOrderRepository {
    store: Arc<JsonStore<Order>>,
}

impl JsonOrderRepository {
    pub async fn open(path: impl AsRef<Path>) -> Result<Self, RepoError> {
        let store = JsonStore::open(path.as_ref(), |o: &Order| o.id.to_string())
            .await
            .map_err(|e| RepoError::Backend(e.to_string()))?;
        Ok(Self {
            store: Arc::new(store),
        })
    }
}

#[async_trait]
impl OrderRepository for JsonOrderRepository {
    async fn find_by_id(&self, id: Uuid) -> Result<Option<Order>, RepoError> {
        Ok(self.store.find(&id.to_string()).await)
    }

    async fn list_for_customer(
        &self,
        customer_id: Uuid,
        after: Option<Cursor>,
        limit: u32,
    ) -> Result<Vec<Order>, RepoError> {
        let after_time = match after {
            Some(c) => Some(decode_cursor(&c)?),
            None => None,
        };
        let limit = limit.clamp(1, 200) as usize;
        let snap = self.store.snapshot().await;
        let mut out: Vec<Order> = snap
            .into_iter()
            .filter(|o| o.customer_id == customer_id)
            .filter(|o| match after_time {
                Some(t) => o.created_at < t,
                None => true,
            })
            .collect();
        out.sort_by(|a, b| b.created_at.cmp(&a.created_at)); // newest first
        out.truncate(limit);
        Ok(out)
    }

    async fn list_for_technician(
        &self,
        technician_id: Uuid,
        after: Option<Cursor>,
        limit: u32,
    ) -> Result<Vec<Order>, RepoError> {
        let after_time = match after {
            Some(c) => Some(decode_cursor(&c)?),
            None => None,
        };
        let limit = limit.clamp(1, 200) as usize;
        let snap = self.store.snapshot().await;
        let mut out: Vec<Order> = snap
            .into_iter()
            .filter(|o| o.technician_id == Some(technician_id))
            .filter(|o| match after_time {
                Some(t) => o.created_at < t,
                None => true,
            })
            .collect();
        out.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        out.truncate(limit);
        Ok(out)
    }

    async fn insert(&self, order: &Order) -> Result<(), RepoError> {
        if self.store.contains_key(&order.id.to_string()).await {
            return Err(RepoError::Conflict("id exists".into()));
        }
        self.store
            .upsert(order)
            .await
            .map_err(|e| RepoError::Backend(e.to_string()))?;
        Ok(())
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct CursorPayload {
    after: chrono::DateTime<chrono::Utc>,
}

fn decode_cursor(c: &Cursor) -> Result<chrono::DateTime<chrono::Utc>, RepoError> {
    let p: CursorPayload = c
        .decode()
        .map_err(|e| RepoError::Backend(format!("invalid cursor: {e}")))?;
    Ok(p.after)
}

/// Build a cursor for the next page.
pub fn encode_cursor(after: chrono::DateTime<chrono::Utc>) -> Result<Cursor, RepoError> {
    Cursor::encode(&CursorPayload { after })
        .map_err(|e| RepoError::Backend(format!("cursor encode: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use kokkak_domain::OrderStatus;
    use rust_decimal::Decimal;
    use std::str::FromStr;

    fn tmp(name: &str) -> std::path::PathBuf {
        std::env::temp_dir()
            .join("kokkak_order_repo_test")
            .join(name)
    }

    fn sample(customer: Uuid, total: &str) -> Order {
        Order {
            id: Uuid::new_v4(),
            service_code: "ac".into(),
            customer_id: customer,
            technician_id: None,
            description: "test".into(),
            address: "addr".into(),
            total: Decimal::from_str(total).unwrap(),
            status: OrderStatus::Pending,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[tokio::test]
    async fn list_for_customer_returns_only_theirs_newest_first() {
        let path = tmp("o1.json");
        let _ = std::fs::remove_file(&path);
        let repo = JsonOrderRepository::open(&path).await.unwrap();
        let c1 = Uuid::new_v4();
        let c2 = Uuid::new_v4();
        let mut a = sample(c1, "100.00");
        a.created_at = chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap();
        let mut b = sample(c1, "200.00");
        b.created_at = chrono::DateTime::from_timestamp(1_700_001_000, 0).unwrap();
        let mut other = sample(c2, "300.00");
        other.created_at = chrono::DateTime::from_timestamp(1_700_002_000, 0).unwrap();
        repo.insert(&a).await.unwrap();
        repo.insert(&b).await.unwrap();
        repo.insert(&other).await.unwrap();
        let got = repo.list_for_customer(c1, None, 10).await.unwrap();
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].total.to_string(), "200.00"); // newest first
        assert_eq!(got[1].total.to_string(), "100.00");
    }

    #[tokio::test]
    async fn pagination_uses_created_at_cursor() {
        let path = tmp("o2.json");
        let _ = std::fs::remove_file(&path);
        let repo = JsonOrderRepository::open(&path).await.unwrap();
        let c1 = Uuid::new_v4();
        for i in 0..3 {
            let mut o = sample(c1, &format!("{}.00", 100 * (i + 1)));
            o.created_at = chrono::DateTime::from_timestamp(1_700_000_000 + i * 100, 0).unwrap();
            repo.insert(&o).await.unwrap();
        }
        let first = repo.list_for_customer(c1, None, 2).await.unwrap();
        assert_eq!(first.len(), 2);
        let cursor = encode_cursor(first.last().unwrap().created_at).unwrap();
        let second = repo.list_for_customer(c1, Some(cursor), 10).await.unwrap();
        assert_eq!(second.len(), 1);
    }
}
