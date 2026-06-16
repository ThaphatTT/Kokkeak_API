//! Order use cases (M3 + M6 create).
//!
//! Read-mostly: list orders for the current customer / technician
//! with keyset pagination. M6 adds the **create** use case which
//! persists a new order and (optionally) publishes a
//! `order.dispatch` message so the worker can fan out to candidates.

use std::sync::Arc;

use chrono::Utc;
use kokkak_domain::{Cursor, Order, OrderRepository, OrderStatus, QueuePort, RepoError};
use rust_decimal::Decimal;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct OrderListPage {
    pub items: Vec<Order>,
    pub next_cursor: Option<String>,
}

/// Input for the `create_order` use case (M6).
#[derive(Debug, Clone)]
pub struct CreateOrderInput {
    pub service_code: String,
    pub customer_id: Uuid,
    pub description: String,
    pub address: String,
    pub total: Decimal,
    pub order_lat: Option<f64>,
    pub order_lon: Option<f64>,
}

pub struct OrderService {
    orders: Arc<dyn OrderRepository>,
    queue: Option<Arc<dyn QueuePort>>,
}

impl OrderService {
    pub fn new(orders: Arc<dyn OrderRepository>) -> Self {
        Self {
            orders,
            queue: None,
        }
    }

    /// Attach a NATS queue so `create_order` can publish
    /// `order.dispatch` after persisting the new order.
    pub fn with_queue(mut self, queue: Arc<dyn QueuePort>) -> Self {
        self.queue = Some(queue);
        self
    }

    /// Borrow the underlying order repository (used by
    /// `PaymentService` to look up the order being paid).
    pub fn orders_repo(&self) -> Arc<dyn OrderRepository> {
        self.orders.clone()
    }

    pub async fn list_for_customer(
        &self,
        customer_id: Uuid,
        after: Option<String>,
        limit: u32,
    ) -> Result<OrderListPage, RepoError> {
        let cursor = match after {
            Some(s) => Some(
                s.parse::<Cursor>()
                    .map_err(|e| RepoError::Backend(format!("invalid cursor: {e}")))?,
            ),
            None => None,
        };
        let limit = limit.clamp(1, 200);
        let items = self
            .orders
            .list_for_customer(customer_id, cursor, limit)
            .await?;
        let next_cursor = if (items.len() as u32) == limit {
            items
                .last()
                .map(|o| Cursor::encode(&serde_json::json!({ "after": o.created_at })).ok())
                .flatten()
                .map(|c| c.to_string())
        } else {
            None
        };
        Ok(OrderListPage { items, next_cursor })
    }

    pub async fn list_for_technician(
        &self,
        technician_id: Uuid,
        after: Option<String>,
        limit: u32,
    ) -> Result<OrderListPage, RepoError> {
        let cursor = match after {
            Some(s) => Some(
                s.parse::<Cursor>()
                    .map_err(|e| RepoError::Backend(format!("invalid cursor: {e}")))?,
            ),
            None => None,
        };
        let limit = limit.clamp(1, 200);
        let items = self
            .orders
            .list_for_technician(technician_id, cursor, limit)
            .await?;
        let next_cursor = if (items.len() as u32) == limit {
            items
                .last()
                .map(|o| Cursor::encode(&serde_json::json!({ "after": o.created_at })).ok())
                .flatten()
                .map(|c| c.to_string())
        } else {
            None
        };
        Ok(OrderListPage { items, next_cursor })
    }

    /// Create a new order (M6). Persists the order, then publishes an
    /// `order.dispatch` message on the configured queue (if any) so
    /// the worker can fan out to candidate technicians.
    pub async fn create_order(&self, input: CreateOrderInput) -> Result<Order, RepoError> {
        if input.total < Decimal::ZERO {
            return Err(RepoError::Backend("total must be non-negative".into()));
        }
        let now = Utc::now();
        let order = Order {
            id: Uuid::new_v4(),
            service_code: input.service_code,
            customer_id: input.customer_id,
            technician_id: None,
            description: input.description,
            address: input.address,
            total: input.total,
            status: OrderStatus::Pending,
            created_at: now,
            updated_at: now,
        };
        self.orders.insert(&order).await?;

        // Best-effort publish — never block the request on queue
        // errors (AGENTS.md § 10: external work is async).
        if let Some(q) = &self.queue {
            let payload = serde_json::json!({
                "order_id": order.id,
                "service_code": order.service_code,
                "customer_id": order.customer_id,
                "lat": input.order_lat,
                "lon": input.order_lon,
            });
            if let Ok(s) = serde_json::to_vec(&payload) {
                if let Err(e) = q.publish("order.dispatch", &s).await {
                    tracing::warn!(order_id = %order.id, error = %e, "order.dispatch publish failed");
                }
            }
        }
        Ok(order)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use kokkak_domain::OrderStatus;
    use kokkak_infra::db::json_order::JsonOrderRepository;
    use rust_decimal::Decimal;
    use std::path::PathBuf;
    use std::str::FromStr;
    use uuid::Uuid;

    fn svc_path() -> PathBuf {
        let p = std::env::temp_dir()
            .join("kokkak_order_test")
            .join(format!("o-{}.json", Uuid::new_v4()));
        let _ = std::fs::create_dir_all(p.parent().unwrap());
        let _ = std::fs::remove_file(&p);
        p
    }

    fn sample(customer: Uuid) -> Order {
        Order {
            id: Uuid::new_v4(),
            service_code: "ac".into(),
            customer_id: customer,
            technician_id: None,
            description: "test".into(),
            address: "addr".into(),
            total: Decimal::from_str("100.00").unwrap(),
            status: OrderStatus::Pending,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[tokio::test]
    async fn list_for_customer_pagination() {
        let repo = JsonOrderRepository::open(svc_path()).await.unwrap();
        let customer = Uuid::new_v4();
        for i in 0..3 {
            let mut o = sample(customer);
            o.created_at = chrono::DateTime::from_timestamp(1_700_000_000 + i * 100, 0).unwrap();
            repo.insert(&o).await.unwrap();
        }
        let svc = OrderService::new(Arc::new(repo));
        let page = svc.list_for_customer(customer, None, 2).await.unwrap();
        assert_eq!(page.items.len(), 2);
        assert!(page.next_cursor.is_some());
        let page2 = svc
            .list_for_customer(customer, page.next_cursor.clone(), 2)
            .await
            .unwrap();
        assert_eq!(page2.items.len(), 1);
    }
}
