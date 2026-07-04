

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::catalog::ServiceCategory;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum OrderStatus {

    Pending,

    Active,

    Completed,

    Closed,

    Cancelled,
}

impl OrderStatus {

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Active => "active",
            Self::Completed => "completed",
            Self::Closed => "closed",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Order {

    pub id: Uuid,

    pub service_code: String,

    pub customer_id: Uuid,

    pub technician_id: Option<Uuid>,

    pub description: String,

    pub address: String,

    pub total: Decimal,

    pub status: OrderStatus,

    pub created_at: DateTime<Utc>,

    pub updated_at: DateTime<Utc>,
}

impl Order {

    pub fn is_cancellable(&self) -> bool {
        matches!(self.status, OrderStatus::Pending | OrderStatus::Active)
    }
}

#[derive(Debug, Clone)]
pub struct ServiceRef<'a> {

    pub code: &'a str,

    pub category: Option<&'a ServiceCategory>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn order_status_as_str_is_snake_case() {
        assert_eq!(OrderStatus::Pending.as_str(), "pending");
        assert_eq!(OrderStatus::Active.as_str(), "active");
        assert_eq!(OrderStatus::Completed.as_str(), "completed");
        assert_eq!(OrderStatus::Closed.as_str(), "closed");
        assert_eq!(OrderStatus::Cancelled.as_str(), "cancelled");
    }

    #[test]
    fn is_cancellable_only_for_open_statuses() {
        let mut o = Order {
            id: Uuid::new_v4(),
            service_code: "ac".into(),
            customer_id: Uuid::new_v4(),
            technician_id: None,
            description: "x".into(),
            address: "y".into(),
            total: Decimal::from_str("100.00").unwrap(),
            status: OrderStatus::Pending,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        assert!(o.is_cancellable());
        o.status = OrderStatus::Active;
        assert!(o.is_cancellable());
        o.status = OrderStatus::Completed;
        assert!(!o.is_cancellable());
        o.status = OrderStatus::Closed;
        assert!(!o.is_cancellable());
        o.status = OrderStatus::Cancelled;
        assert!(!o.is_cancellable());
    }
}
