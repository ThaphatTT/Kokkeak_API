//! Order domain (คำสั่งจ้างงาน — M3).
//!
//! The full order lifecycle is large (M6+). M3 only ships the
//! **skeleton**: the entity shape, the basic status enum, and the
//! repository port. The matching / dispatch / commission logic lands
//! in later milestones (AGENTS.md § 6.4 build plan).

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::catalog::ServiceCategory;

/// High-level status of an order. Detailed state machine lives in
/// the future `OrderStage` aggregate; this enum is the simple
/// customer/technician-facing view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum OrderStatus {
    /// Customer created the order; waiting for technician.
    Pending,
    /// Technician accepted; on the way / working.
    Active,
    /// Work done, pending customer confirmation.
    Completed,
    /// Customer confirmed; order closed.
    Closed,
    /// Cancelled before close.
    Cancelled,
}

impl OrderStatus {
    /// Snake_case identifier.
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

/// One order / job (สัญญาจ้างหนึ่งงาน).
///
/// M3 keeps the field set minimal. M6+ adds: `OrderBody` (multi-line),
/// `Assignment` (technician dispatch), `Review`, `Addon` etc. The
/// schema mirrors the legacy `KOKKAK_ORDER` database tables
/// (AGENTS.md § 7.1) one-to-one for the Strangler migration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Order {
    /// Stable identifier.
    pub id: Uuid,
    /// Service category code (FK to `ServiceCategory::code`).
    pub service_code: String,
    /// Customer who placed the order.
    pub customer_id: Uuid,
    /// Technician who accepted the order, if any.
    pub technician_id: Option<Uuid>,
    /// Short description of the problem.
    pub description: String,
    /// Where the work happens.
    pub address: String,
    /// Quoted / agreed price in LAK. Money — `Decimal`, never `f64`.
    pub total: Decimal,
    /// Current status.
    pub status: OrderStatus,
    /// Customer note: `None` for now, future use (e.g. translated
    /// problem description per locale).
    pub created_at: DateTime<Utc>,
    /// Last modified timestamp (set on every status transition).
    pub updated_at: DateTime<Utc>,
}

impl Order {
    /// `true` when the order is still open and the customer can
    /// cancel it.
    pub fn is_cancellable(&self) -> bool {
        matches!(self.status, OrderStatus::Pending | OrderStatus::Active)
    }
}

/// Reference to a service category code (used in DTOs / use cases).
#[derive(Debug, Clone)]
pub struct ServiceRef<'a> {
    /// Code (`ServiceCategory::code`).
    pub code: &'a str,
    /// Cached category (optional — used to validate price).
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
