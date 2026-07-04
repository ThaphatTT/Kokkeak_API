

use async_trait::async_trait;
use tiberius::ToSql;
use uuid::Uuid;

use kokkak_domain::payment::{Commission, Payment, PaymentStatus, Payout, PayoutStatus};
use kokkak_domain::traits::payment::{PaymentRepoError, PaymentRepository};

use crate::db::mssql::{exec_sp, MssqlPool};

#[derive(Clone)]
pub struct MssqlPaymentRepository {
    pool: MssqlPool,
}

impl MssqlPaymentRepository {

    pub fn new(pool: MssqlPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl PaymentRepository for MssqlPaymentRepository {
    async fn insert_payment(&self, payment: &Payment) -> Result<(), PaymentRepoError> {
        let amount = payment.amount;

        let method = if payment.gateway_ref.is_empty() {
            "local".to_string()
        } else {
            "gateway".to_string()
        };
        let _ = exec_sp(
            &self.pool,
            "EXEC dbo.API_PAYMENT_CREATE \
                @p_order_guid = @P1, @p_customer_guid = @P2, \
                @p_amount = @P3, @p_method_code = @P4",
            &[
                &payment.order_id as &dyn ToSql,
                &payment.customer_id as &dyn ToSql,
                &amount as &dyn ToSql,
                &method as &dyn ToSql,
            ],
        )
        .await
        .map_err(|e| PaymentRepoError::Backend(e.to_string()))?;
        Ok(())
    }

    async fn find_payment(&self, id: Uuid) -> Result<Option<Payment>, PaymentRepoError> {

        let _ = id;
        Ok(None)
    }

    async fn find_payment_by_order(
        &self,
        _order_id: Uuid,
    ) -> Result<Option<Payment>, PaymentRepoError> {

        Ok(None)
    }

    async fn list_payments_for_customer(
        &self,
        _customer_id: Uuid,
        _limit: u32,
    ) -> Result<Vec<Payment>, PaymentRepoError> {
        Err(PaymentRepoError::Backend(
            "MssqlPaymentRepository::list_payments_for_customer — SP lands in M15+".into(),
        ))
    }

    async fn update_payment_status(
        &self,
        id: Uuid,
        status: PaymentStatus,
        gateway_ref: Option<&str>,
    ) -> Result<(), PaymentRepoError> {
        let _ = id;
        let _ = status;
        let _ = gateway_ref;
        Err(PaymentRepoError::Backend(
            "MssqlPaymentRepository::update_payment_status — API_PAYMENT_CONFIRM SP wired but trait signature differs".into(),
        ))
    }

    async fn insert_commission(&self, _commission: &Commission) -> Result<(), PaymentRepoError> {
        Err(PaymentRepoError::Backend(
            "MssqlPaymentRepository::insert_commission — lands in M15+".into(),
        ))
    }

    async fn find_commission_by_order(
        &self,
        _order_id: Uuid,
    ) -> Result<Option<Commission>, PaymentRepoError> {
        Ok(None)
    }

    async fn insert_payout(&self, _payout: &Payout) -> Result<(), PaymentRepoError> {
        Err(PaymentRepoError::Backend(
            "MssqlPaymentRepository::insert_payout — lands in M15+".into(),
        ))
    }

    async fn list_payouts(
        &self,
        _technician_id: Option<Uuid>,
        _status: Option<PayoutStatus>,
        _limit: u32,
    ) -> Result<Vec<Payout>, PaymentRepoError> {
        Err(PaymentRepoError::Backend(
            "MssqlPaymentRepository::list_payouts — lands in M15+".into(),
        ))
    }

    async fn update_payout_status(
        &self,
        _id: Uuid,
        _status: PayoutStatus,
    ) -> Result<(), PaymentRepoError> {
        Err(PaymentRepoError::Backend(
            "MssqlPaymentRepository::update_payout_status — lands in M15+".into(),
        ))
    }
}
