//! `PaymentRepository` port (พอร์ตการชำระเงิน — M9).
//!
//! Persistence for the three payment aggregates: `Payment`,
//! `Commission`, `Payout`. Adapters are responsible for keeping
//! related rows consistent — e.g. the JSON-DB sim persists them
//! to a single file and rebuilds the in-memory index on load.

use async_trait::async_trait;
use uuid::Uuid;

use crate::payment::{Commission, Payment, PaymentStatus, Payout, PayoutStatus};

#[derive(Debug, thiserror::Error)]
pub enum PaymentRepoError {
    /// The payment / payout does not exist.
    #[error("not found: {0}")]
    NotFound(String),

    /// Persistence / driver failure.
    #[error("payment backend error: {0}")]
    Backend(String),
}

#[async_trait]
pub trait PaymentRepository: Send + Sync {
    // -- payments --
    /// Insert a new payment (idempotent on payment id; rejects
    /// when a payment for the same `order_id` already exists).
    async fn insert_payment(&self, payment: &Payment) -> Result<(), PaymentRepoError>;
    /// Look up a payment by id.
    async fn find_payment(&self, id: Uuid) -> Result<Option<Payment>, PaymentRepoError>;
    /// Look up a payment by the order it settles.
    async fn find_payment_by_order(
        &self,
        order_id: Uuid,
    ) -> Result<Option<Payment>, PaymentRepoError>;
    /// List the customer's payments, newest first.
    async fn list_payments_for_customer(
        &self,
        customer_id: Uuid,
        limit: u32,
    ) -> Result<Vec<Payment>, PaymentRepoError>;
    /// Update a payment's lifecycle status. `gateway_ref` is
    /// stored alongside when supplied.
    async fn update_payment_status(
        &self,
        id: Uuid,
        status: PaymentStatus,
        gateway_ref: Option<&str>,
    ) -> Result<(), PaymentRepoError>;

    // -- commission --
    /// Insert / overwrite a commission record (one per order).
    async fn insert_commission(&self, commission: &Commission) -> Result<(), PaymentRepoError>;
    /// Look up the commission record for an order.
    async fn find_commission_by_order(
        &self,
        order_id: Uuid,
    ) -> Result<Option<Commission>, PaymentRepoError>;

    // -- payout --
    /// Insert a new payout (the technician's net portion of an
    /// order's commission).
    async fn insert_payout(&self, payout: &Payout) -> Result<(), PaymentRepoError>;
    /// Admin / dashboard projection; filterable by technician
    /// and status, newest first.
    async fn list_payouts(
        &self,
        technician_id: Option<Uuid>,
        status: Option<PayoutStatus>,
        limit: u32,
    ) -> Result<Vec<Payout>, PaymentRepoError>;
    /// Update a payout's lifecycle status (used by the bank
    /// webhook in M12+).
    async fn update_payout_status(
        &self,
        id: Uuid,
        status: PayoutStatus,
    ) -> Result<(), PaymentRepoError>;
}
