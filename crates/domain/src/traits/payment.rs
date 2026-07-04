

use async_trait::async_trait;
use uuid::Uuid;

use crate::payment::{Commission, Payment, PaymentStatus, Payout, PayoutStatus};

#[derive(Debug, thiserror::Error)]
pub enum PaymentRepoError {

    #[error("not found: {0}")]
    NotFound(String),

    #[error("payment backend error: {0}")]
    Backend(String),
}

#[async_trait]
pub trait PaymentRepository: Send + Sync {

    async fn insert_payment(&self, payment: &Payment) -> Result<(), PaymentRepoError>;

    async fn find_payment(&self, id: Uuid) -> Result<Option<Payment>, PaymentRepoError>;

    async fn find_payment_by_order(
        &self,
        order_id: Uuid,
    ) -> Result<Option<Payment>, PaymentRepoError>;

    async fn list_payments_for_customer(
        &self,
        customer_id: Uuid,
        limit: u32,
    ) -> Result<Vec<Payment>, PaymentRepoError>;

    async fn update_payment_status(
        &self,
        id: Uuid,
        status: PaymentStatus,
        gateway_ref: Option<&str>,
    ) -> Result<(), PaymentRepoError>;

    async fn insert_commission(&self, commission: &Commission) -> Result<(), PaymentRepoError>;

    async fn find_commission_by_order(
        &self,
        order_id: Uuid,
    ) -> Result<Option<Commission>, PaymentRepoError>;

    async fn insert_payout(&self, payout: &Payout) -> Result<(), PaymentRepoError>;

    async fn list_payouts(
        &self,
        technician_id: Option<Uuid>,
        status: Option<PayoutStatus>,
        limit: u32,
    ) -> Result<Vec<Payout>, PaymentRepoError>;

    async fn update_payout_status(
        &self,
        id: Uuid,
        status: PayoutStatus,
    ) -> Result<(), PaymentRepoError>;
}
