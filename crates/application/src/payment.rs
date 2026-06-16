//! Payment use cases (M9).
//!
//! 1. `create_payment` — opens a payment intent for an order.
//! 2. `confirm_payment` — moves a pending payment to
//!    `Captured`, computes the commission, queues the payout.
//! 3. `list_payments` / `list_payouts` — read-side projections.
//! 4. `mark_payout_paid` — admin-only hook for the worker that
//!    confirms the bank transfer.
//!
//! The "gateway" is still a stub for M9 — the worker integration
//! is scheduled for M12. Until then, `confirm_payment` flips the
//! status locally so the dev / e2e flow is end-to-end.

use std::sync::Arc;

use chrono::Utc;
use kokkak_domain::{
    commission, Commission, Order, OrderRepository, Payment, PaymentError, PaymentRepoError,
    PaymentRepository, PaymentStatus, Payout, PayoutStatus, RepoError,
};
use rust_decimal::Decimal;
use uuid::Uuid;

/// Default platform commission rate (50% — matches the legacy
/// `system_commission` table — AGENTS.md § 20.4). The rate is
/// fixed for M9; an admin override lands in M12.
const DEFAULT_COMMISSION_RATE: Decimal = rust_decimal_macros::dec!(0.50);

/// Input for `create_payment`.
#[derive(Debug, Clone)]
pub struct CreatePaymentInput {
    pub order_id: Uuid,
    pub customer_id: Uuid,
    /// Optional override (otherwise the order's total is used).
    pub amount: Option<Decimal>,
}

/// Input for `confirm_payment`.
#[derive(Debug, Clone)]
pub struct ConfirmPaymentInput {
    pub payment_id: Uuid,
    /// Optional gateway reference (set by the gateway webhook
    /// in M12+; the dev flow passes `None`).
    pub gateway_ref: Option<String>,
}

/// Result of `confirm_payment` (everything the dev / admin UI
/// needs to render a "thank you" screen).
#[derive(Debug, Clone)]
pub struct ConfirmPaymentResult {
    pub payment: Payment,
    pub commission: Commission,
    pub payout: Payout,
}

pub struct PaymentService {
    payments: Arc<dyn PaymentRepository>,
    orders: Arc<dyn OrderRepository>,
}

impl PaymentService {
    pub fn new(payments: Arc<dyn PaymentRepository>, orders: Arc<dyn OrderRepository>) -> Self {
        Self { payments, orders }
    }

    /// Open a new payment intent for an order.
    pub async fn create_payment(&self, input: CreatePaymentInput) -> Result<Payment, PaymentError> {
        if let Some(existing) = self
            .payments
            .find_payment_by_order(input.order_id)
            .await
            .map_err(repo_err)?
        {
            // Re-use the existing intent — multiple create
            // calls for the same order are idempotent.
            return Ok(existing);
        }
        let order = self
            .orders
            .find_by_id(input.order_id)
            .await
            .map_err(|e| PaymentError::Backend(e.to_string()))?
            .ok_or(PaymentError::NotFound(input.order_id))?;
        let amount = input.amount.unwrap_or(order.total);
        if amount <= Decimal::ZERO {
            return Err(PaymentError::InvalidAmount(format!(
                "amount must be positive (got {amount})"
            )));
        }
        let now = Utc::now();
        let p = Payment {
            id: Uuid::new_v4(),
            order_id: input.order_id,
            customer_id: input.customer_id,
            amount,
            gateway_ref: String::new(),
            status: PaymentStatus::Pending,
            currency: "LAK".into(),
            created_at: now,
            updated_at: now,
        };
        self.payments.insert_payment(&p).await.map_err(repo_err)?;
        Ok(p)
    }

    /// Capture the funds and compute commission + queue payout.
    pub async fn confirm_payment(
        &self,
        input: ConfirmPaymentInput,
    ) -> Result<ConfirmPaymentResult, PaymentError> {
        let mut payment = self
            .payments
            .find_payment(input.payment_id)
            .await
            .map_err(repo_err)?
            .ok_or(PaymentError::NotFound(input.payment_id))?;
        if !matches!(
            payment.status,
            PaymentStatus::Pending | PaymentStatus::Authorized
        ) {
            return Err(PaymentError::InvalidAmount(format!(
                "payment already in status {}",
                payment.status.as_str()
            )));
        }
        let order = self
            .orders
            .find_by_id(payment.order_id)
            .await
            .map_err(|e| PaymentError::Backend(e.to_string()))?
            .ok_or(PaymentError::NotFound(payment.order_id))?;
        let technician_id = order
            .technician_id
            .ok_or(PaymentError::OrderNotPayable(payment.order_id))?;

        // Compute commission (deterministic, unit-tested).
        let (cut, net) = commission::compute(&order, DEFAULT_COMMISSION_RATE);

        // 1. Update payment status.
        self.payments
            .update_payment_status(
                payment.id,
                PaymentStatus::Captured,
                input.gateway_ref.as_deref(),
            )
            .await
            .map_err(repo_err)?;
        payment.status = PaymentStatus::Captured;
        if let Some(g) = input.gateway_ref {
            payment.gateway_ref = g;
        }

        // 2. Persist commission (idempotent on order_id: overwrite).
        let now = Utc::now();
        let comm = Commission {
            id: Uuid::new_v4(),
            order_id: order.id,
            technician_id,
            gross: order.total,
            amount: cut,
            rate: DEFAULT_COMMISSION_RATE,
            net_to_tech: net,
            computed_at: now,
        };
        self.payments
            .insert_commission(&comm)
            .await
            .map_err(repo_err)?;

        // 3. Queue payout.
        let payout = Payout {
            id: Uuid::new_v4(),
            technician_id,
            order_id: order.id,
            amount: net,
            status: PayoutStatus::Pending,
            created_at: now,
            updated_at: now,
        };
        self.payments
            .insert_payout(&payout)
            .await
            .map_err(repo_err)?;

        Ok(ConfirmPaymentResult {
            payment,
            commission: comm,
            payout,
        })
    }

    /// List the customer's payments (newest first).
    pub async fn list_payments_for(
        &self,
        customer_id: Uuid,
        limit: u32,
    ) -> Result<Vec<Payment>, PaymentError> {
        let limit = limit.clamp(1, 200);
        self.payments
            .list_payments_for_customer(customer_id, limit)
            .await
            .map_err(repo_err)
    }

    /// Find a single payment by id.
    pub async fn find_payment(&self, id: Uuid) -> Result<Option<Payment>, PaymentError> {
        self.payments.find_payment(id).await.map_err(repo_err)
    }

    /// Admin: list payouts, filterable by technician + status.
    pub async fn list_payouts(
        &self,
        technician_id: Option<Uuid>,
        status: Option<PayoutStatus>,
        limit: u32,
    ) -> Result<Vec<Payout>, PaymentError> {
        let limit = limit.clamp(1, 500);
        self.payments
            .list_payouts(technician_id, status, limit)
            .await
            .map_err(repo_err)
    }

    /// Admin: mark a payout as paid (used by the bank webhook
    /// in M12+; exposed now so the e2e test can drive the
    /// status transitions).
    pub async fn mark_payout_paid(&self, id: Uuid) -> Result<Payout, PaymentError> {
        self.payments
            .update_payout_status(id, PayoutStatus::Paid)
            .await
            .map_err(repo_err)?;
        // Re-read for the return value. (In a real DB the
        // adapter would return the row directly; for the
        // JSON-DB sim the second hop is fine.)
        let all = self
            .payments
            .list_payouts(None, None, 500)
            .await
            .map_err(repo_err)?;
        all.into_iter()
            .find(|p| p.id == id)
            .ok_or(PaymentError::NotFound(id))
    }
}

fn repo_err(e: PaymentRepoError) -> PaymentError {
    PaymentError::Backend(e.to_string())
}

#[doc(hidden)]
pub fn _touch_order_trait(_o: &Order) {}

#[doc(hidden)]
const _REPO_ERROR_TOUCH: fn(RepoError) = |_| {};
