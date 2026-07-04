

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::order::Order;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum PaymentStatus {

    Pending,

    Authorized,

    Captured,

    Failed,

    Refunded,
}

impl PaymentStatus {

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Authorized => "authorized",
            Self::Captured => "captured",
            Self::Failed => "failed",
            Self::Refunded => "refunded",
        }
    }

    pub fn is_settled(&self) -> bool {
        matches!(self, Self::Captured | Self::Refunded)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Payment {

    pub id: Uuid,

    pub order_id: Uuid,

    pub customer_id: Uuid,

    pub amount: Decimal,

    pub gateway_ref: String,

    pub status: PaymentStatus,

    pub currency: String,

    pub created_at: DateTime<Utc>,

    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Commission {

    pub id: Uuid,

    pub order_id: Uuid,

    pub technician_id: Uuid,

    pub gross: Decimal,

    pub amount: Decimal,

    pub rate: Decimal,

    pub net_to_tech: Decimal,

    pub computed_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Payout {

    pub id: Uuid,

    pub technician_id: Uuid,

    pub order_id: Uuid,

    pub amount: Decimal,

    pub status: PayoutStatus,

    pub created_at: DateTime<Utc>,

    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum PayoutStatus {

    Pending,

    Queued,

    Paid,

    Failed,
}

impl PayoutStatus {

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Queued => "queued",
            Self::Paid => "paid",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PaymentError {

    #[error("order {0} is not payable (status mismatch)")]
    OrderNotPayable(Uuid),

    #[error("payment {0} not found")]
    NotFound(Uuid),

    #[error("invalid amount: {0}")]
    InvalidAmount(String),

    #[error("payment backend error: {0}")]
    Backend(String),
}

pub mod commission {
    use super::{Decimal, Order};
    use rust_decimal::prelude::Zero;

    pub fn compute(order: &Order, rate: Decimal) -> (Decimal, Decimal) {
        if rate < Decimal::zero() {
            return (order.total, Decimal::zero());
        }
        let cap_rate = if rate > Decimal::from(1) {
            Decimal::from(1)
        } else {
            rate
        };
        let amount = (order.total * cap_rate).round_dp(2);
        let net = (order.total - amount).round_dp(2);
        (amount, net)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::order::OrderStatus;
    use chrono::Utc;
    use rust_decimal_macros::dec;
    use std::str::FromStr;

    fn order(total: &str) -> Order {
        Order {
            id: Uuid::new_v4(),
            service_code: "ac".into(),
            customer_id: Uuid::new_v4(),
            technician_id: Some(Uuid::new_v4()),
            description: "x".into(),
            address: "y".into(),
            total: Decimal::from_str(total).unwrap(),
            status: OrderStatus::Completed,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn payment_status_settled_only_for_captured_or_refunded() {
        assert!(!PaymentStatus::Pending.is_settled());
        assert!(!PaymentStatus::Authorized.is_settled());
        assert!(!PaymentStatus::Failed.is_settled());
        assert!(PaymentStatus::Captured.is_settled());
        assert!(PaymentStatus::Refunded.is_settled());
    }

    #[test]
    fn payment_status_as_str_is_snake_case() {
        assert_eq!(PaymentStatus::Pending.as_str(), "pending");
        assert_eq!(PaymentStatus::Captured.as_str(), "captured");
        assert_eq!(PaymentStatus::Refunded.as_str(), "refunded");
    }

    #[test]
    fn commission_50pct_splits_in_half() {
        let o = order("100.00");
        let (amount, net) = commission::compute(&o, dec!(0.50));
        assert_eq!(amount, dec!(50.00));
        assert_eq!(net, dec!(50.00));
    }

    #[test]
    fn commission_0pct_returns_zero_cut() {
        let o = order("123.45");
        let (amount, net) = commission::compute(&o, dec!(0));
        assert_eq!(amount, dec!(0));
        assert_eq!(net, dec!(123.45));
    }

    #[test]
    fn commission_100pct_caps_at_full() {
        let o = order("200.00");
        let (amount, net) = commission::compute(&o, dec!(1.5));
        assert_eq!(amount, dec!(200.00));
        assert_eq!(net, dec!(0));
    }

    #[test]
    fn commission_negative_rate_safely_handled() {
        let o = order("75.00");
        let (amount, net) = commission::compute(&o, dec!(-0.5));
        assert_eq!(amount, dec!(75.00));
        assert_eq!(net, dec!(0));
    }

    #[test]
    fn commission_rounds_to_two_decimals() {
        let o = order("100.00");

        let (amount, net) = commission::compute(&o, dec!(0.3333));
        assert_eq!(amount, dec!(33.33));
        assert_eq!(net, dec!(66.67));
    }
}
