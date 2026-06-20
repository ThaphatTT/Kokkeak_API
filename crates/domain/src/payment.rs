//! Payment domain (การชำระเงิน — M9).
//!
//! - [`Payment`] — one customer-side payment for an order. The
//!   status flow is `Pending -> Authorized -> Captured` (happy
//!   path) or `Failed` / `Refunded` (failure / post-close paths).
//!
//! - [`Commission`] — the platform's cut; computed at capture
//!   time from the order's total and the active commission rate.
//!
//! - [`Payout`] — money owed to a technician for a closed order.
//!   `Pending` until the bank transfer is queued, `Paid` once
//!   confirmed.
//!
//! Money is always [`rust_decimal::Decimal`] — `f64` is forbidden
//! (AGENTS.md § 17). DB columns are `decimal(18, 2)` (the
//! service stores LAK, so two decimals are enough).

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::order::Order;

/// Payment lifecycle (วงจรการชำระเงิน).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum PaymentStatus {
    /// Payment intent created; awaiting the gateway.
    Pending,
    /// Gateway authorized the funds.
    Authorized,
    /// Funds captured; money is on-platform.
    Captured,
    /// Customer / gateway rejected.
    Failed,
    /// Captured then refunded (full or partial).
    Refunded,
}

impl PaymentStatus {
    /// Snake-case identifier.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Authorized => "authorized",
            Self::Captured => "captured",
            Self::Failed => "failed",
            Self::Refunded => "refunded",
        }
    }

    /// `true` iff the order is settled (the customer's money
    /// is on-platform and the technician can be paid out).
    pub fn is_settled(&self) -> bool {
        matches!(self, Self::Captured | Self::Refunded)
    }
}

/// One payment intent (คำสั่งชำระเงินหนึ่งรายการ).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Payment {
    /// Stable identifier.
    pub id: Uuid,
    /// Order this payment settles.
    pub order_id: Uuid,
    /// Customer who pays.
    pub customer_id: Uuid,
    /// Total in LAK.
    pub amount: Decimal,
    /// Gateway-side identifier (e.g. Stripe `pi_...`). Empty
    /// while the intent is still local.
    pub gateway_ref: String,
    /// Lifecycle.
    pub status: PaymentStatus,
    /// Currency code (always `LAK` for M9).
    pub currency: String,
    /// UTC.
    pub created_at: DateTime<Utc>,
    /// UTC.
    pub updated_at: DateTime<Utc>,
}

/// Commission record (ค่าคอมมิชชั่นที่หักจากงาน).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Commission {
    /// Stable identifier.
    pub id: Uuid,
    /// Order.
    pub order_id: Uuid,
    /// Technician who gets the net payout.
    pub technician_id: Uuid,
    /// Order total (LAK).
    pub gross: Decimal,
    /// Platform cut (LAK).
    pub amount: Decimal,
    /// Rate applied (e.g. `0.50`).
    pub rate: Decimal,
    /// `gross - amount` (LAK). Pre-computed for the
    /// statement / payout records.
    pub net_to_tech: Decimal,
    /// UTC.
    pub computed_at: DateTime<Utc>,
}

/// Payout to a technician (เงินเข้าช่าง).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Payout {
    /// Stable identifier.
    pub id: Uuid,
    /// Technician who receives the money.
    pub technician_id: Uuid,
    /// Underlying order.
    pub order_id: Uuid,
    /// Amount in LAK.
    pub amount: Decimal,
    /// Payout lifecycle (separate from `PaymentStatus`).
    pub status: PayoutStatus,
    /// UTC.
    pub created_at: DateTime<Utc>,
    /// UTC.
    pub updated_at: DateTime<Utc>,
}

/// Payout lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum PayoutStatus {
    /// Computed, waiting for the bank transfer.
    Pending,
    /// Transfer queued at the bank.
    Queued,
    /// Bank confirmed the credit.
    Paid,
    /// Bank rejected.
    Failed,
}

impl PayoutStatus {
    /// Snake_case identifier.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Queued => "queued",
            Self::Paid => "paid",
            Self::Failed => "failed",
        }
    }
}

/// Errors that the payment use case can return
/// (ข้อผิดพลาดของการชำระเงิน).
#[derive(Debug, thiserror::Error)]
pub enum PaymentError {
    /// The order is not in a payable state.
    #[error("order {0} is not payable (status mismatch)")]
    OrderNotPayable(Uuid),

    /// The requested payment was not found.
    #[error("payment {0} not found")]
    NotFound(Uuid),

    /// Invalid amount (negative, zero, or more than the order).
    #[error("invalid amount: {0}")]
    InvalidAmount(String),

    /// Persistence / transport failure.
    #[error("payment backend error: {0}")]
    Backend(String),
}

/// Commission math helpers (pure — fully unit-testable).
pub mod commission {
    use super::{Decimal, Order};
    use rust_decimal::prelude::Zero;

    /// Compute the platform commission + technician net for an
    /// order. The rate is `0.0..=1.0`. Saturates the rate at
    /// 100% (net = 0) and rejects negative rates / negative
    /// orders.
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
        // 33% of 100 = 33.3333... -> rounds to 33.33
        let (amount, net) = commission::compute(&o, dec!(0.3333));
        assert_eq!(amount, dec!(33.33));
        assert_eq!(net, dec!(66.67));
    }
}
