

use std::sync::Arc;

use chrono::Utc;
use kokkak_domain::{
    commission, Commission, Order, OrderRepository, Payment, PaymentError, PaymentRepoError,
    PaymentRepository, PaymentStatus, Payout, PayoutStatus, RepoError,
};
use rust_decimal::Decimal;
use uuid::Uuid;

const DEFAULT_COMMISSION_RATE: Decimal = rust_decimal_macros::dec!(0.50);

#[derive(Debug, Clone)]
pub struct CreatePaymentInput {

    pub order_id: Uuid,

    pub customer_id: Uuid,

    pub amount: Option<Decimal>,
}

#[derive(Debug, Clone)]
pub struct ConfirmPaymentInput {

    pub payment_id: Uuid,

    pub gateway_ref: Option<String>,
}

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

    pub async fn create_payment(&self, input: CreatePaymentInput) -> Result<Payment, PaymentError> {
        if let Some(existing) = self
            .payments
            .find_payment_by_order(input.order_id)
            .await
            .map_err(repo_err)?
        {

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

        let (cut, net) = commission::compute(&order, DEFAULT_COMMISSION_RATE);

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

    pub async fn find_payment(&self, id: Uuid) -> Result<Option<Payment>, PaymentError> {
        self.payments.find_payment(id).await.map_err(repo_err)
    }

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

    pub async fn mark_payout_paid(&self, id: Uuid) -> Result<Payout, PaymentError> {
        self.payments
            .update_payout_status(id, PayoutStatus::Paid)
            .await
            .map_err(repo_err)?;

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
