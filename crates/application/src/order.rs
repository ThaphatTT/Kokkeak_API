

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

    pub fn with_queue(mut self, queue: Arc<dyn QueuePort>) -> Self {
        self.queue = Some(queue);
        self
    }

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
                .and_then(|o| Cursor::encode(&serde_json::json!({ "after": o.created_at })).ok())
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
                .and_then(|o| Cursor::encode(&serde_json::json!({ "after": o.created_at })).ok())
                .map(|c| c.to_string())
        } else {
            None
        };
        Ok(OrderListPage { items, next_cursor })
    }

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
