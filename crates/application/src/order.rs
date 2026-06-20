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

/// One page of orders for the customer / technician list routes.
#[derive(Debug, Clone)]
pub struct OrderListPage {
    /// Orders in this page (sorted by `created_at` desc).
    pub items: Vec<Order>,
    /// Cursor for the next page; `None` when this is the last page.
    pub next_cursor: Option<String>,
}

/// Input for the `create_order` use case (M6).
#[derive(Debug, Clone)]
pub struct CreateOrderInput {
    /// Short service category code (e.g. `"AC_REPAIR"`).
    pub service_code: String,
    /// Customer placing the order.
    pub customer_id: Uuid,
    /// Short description of the problem (free text).
    pub description: String,
    /// Where the work happens (free text; full address).
    pub address: String,
    /// Quoted / agreed total in LAK (`Decimal`, never `f64`).
    pub total: Decimal,
    /// Work-site latitude (optional; drives Haversine distance in matching).
    pub order_lat: Option<f64>,
    /// Work-site longitude (optional).
    pub order_lon: Option<f64>,
}

/// Order use case bundle (M6).
pub struct OrderService {
    orders: Arc<dyn OrderRepository>,
    /// Optional NATS producer for `order.dispatch` (set via `with_queue`).
    queue: Option<Arc<dyn QueuePort>>,
}

impl OrderService {
    /// Construct the service with the order repository. The NATS queue
    /// is `None` by default — use [`Self::with_queue`] to attach it.
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

    /// List a customer's own orders (newest first, keyset pagination).
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
                .and_then(|o| Cursor::encode(&serde_json::json!({ "after": o.created_at })).ok())
                .map(|c| c.to_string())
        } else {
            None
        };
        Ok(OrderListPage { items, next_cursor })
    }

    /// List orders assigned to a technician (newest first, keyset pagination).
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
                .and_then(|o| Cursor::encode(&serde_json::json!({ "after": o.created_at })).ok())
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
    use kokkak_domain::{Cursor, OrderStatus};
    use rust_decimal::Decimal;
    use std::collections::HashMap;
    use std::str::FromStr;
    use uuid::Uuid;

    /// In-memory mock of [`OrderRepository`] for unit tests.
    ///
    /// ponytail: HashMap-backed, no async runtime — just enough for the
    /// pagination test below. Ceiling: doesn't model `list_for_technician`
    /// (the unit test only covers customer pagination); extend when a
    /// future test needs the technician view.
    #[derive(Default)]
    struct MockOrderRepository {
        by_id: std::sync::Mutex<HashMap<Uuid, Order>>,
    }

    #[async_trait::async_trait]
    impl OrderRepository for MockOrderRepository {
        async fn find_by_id(&self, id: Uuid) -> Result<Option<Order>, RepoError> {
            Ok(self.by_id.lock().unwrap().get(&id).cloned())
        }
        async fn list_for_customer(
            &self,
            customer_id: Uuid,
            after: Option<Cursor>,
            limit: u32,
        ) -> Result<Vec<Order>, RepoError> {
            let by_id = self.by_id.lock().unwrap();
            let mut items: Vec<Order> = by_id
                .values()
                .filter(|o| o.customer_id == customer_id)
                .cloned()
                .collect();
            // Production lists most-recent-first (OrderService cursor
            // keys off `created_at`); mirror that ordering here.
            items.sort_by_key(|b| std::cmp::Reverse(b.created_at));
            if let Some(cursor) = after {
                if let Ok(payload) = cursor.decode::<serde_json::Value>() {
                    if let Some(s) = payload.get("after").and_then(|v| v.as_str()) {
                        if let Ok(ts) = chrono::DateTime::parse_from_rfc3339(s) {
                            let cutoff: chrono::DateTime<Utc> = ts.with_timezone(&Utc);
                            items.retain(|o| o.created_at < cutoff);
                        }
                    }
                }
            }
            items.truncate(limit as usize);
            Ok(items)
        }
        async fn list_for_technician(
            &self,
            _technician_id: Uuid,
            _after: Option<Cursor>,
            _limit: u32,
        ) -> Result<Vec<Order>, RepoError> {
            Ok(vec![])
        }
        async fn insert(&self, order: &Order) -> Result<(), RepoError> {
            let mut by_id = self.by_id.lock().unwrap();
            if by_id.contains_key(&order.id) {
                return Err(RepoError::Conflict(format!("order {} exists", order.id)));
            }
            by_id.insert(order.id, order.clone());
            Ok(())
        }
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
        let repo: Arc<dyn OrderRepository> = Arc::new(MockOrderRepository::default());
        let customer = Uuid::new_v4();
        for i in 0..3 {
            let mut o = sample(customer);
            o.created_at = chrono::DateTime::from_timestamp(1_700_000_000 + i * 100, 0).unwrap();
            repo.insert(&o).await.unwrap();
        }
        let svc = OrderService::new(repo);
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
