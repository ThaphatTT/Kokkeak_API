//! SQL Server-backed `PaymentRepository` (M10).
//!
//! Three aggregates in the `KOKKAK_PAYMENT` database:
//! ```sql
//! CREATE TABLE payment (
//!     id           UNIQUEIDENTIFIER NOT NULL PRIMARY KEY,
//!     order_id     UNIQUEIDENTIFIER NOT NULL UNIQUE,
//!     customer_id  UNIQUEIDENTIFIER NOT NULL,
//!     amount       DECIMAL(18,2)    NOT NULL,
//!     gateway_ref  NVARCHAR(255)    NOT NULL DEFAULT '',
//!     status       NVARCHAR(32)     NOT NULL,
//!     currency     NVARCHAR(8)      NOT NULL DEFAULT 'LAK',
//!     created_at   DATETIME2(7)     NOT NULL,
//!     updated_at   DATETIME2(7)     NOT NULL
//! );
//! CREATE TABLE commission (
//!     id            UNIQUEIDENTIFIER NOT NULL PRIMARY KEY,
//!     order_id      UNIQUEIDENTIFIER NOT NULL UNIQUE,
//!     technician_id UNIQUEIDENTIFIER NOT NULL,
//!     gross         DECIMAL(18,2)    NOT NULL,
//!     amount        DECIMAL(18,2)    NOT NULL,
//!     rate          DECIMAL(9,6)     NOT NULL,
//!     net_to_tech   DECIMAL(18,2)    NOT NULL,
//!     computed_at   DATETIME2(7)     NOT NULL
//! );
//! CREATE TABLE payout (
//!     id             UNIQUEIDENTIFIER NOT NULL PRIMARY KEY,
//!     technician_id  UNIQUEIDENTIFIER NOT NULL,
//!     order_id       UNIQUEIDENTIFIER NOT NULL UNIQUE,
//!     amount         DECIMAL(18,2)    NOT NULL,
//!     status         NVARCHAR(32)     NOT NULL,
//!     created_at     DATETIME2(7)     NOT NULL,
//!     updated_at     DATETIME2(7)     NOT NULL
//! );
//! ```
//!
//! `insert_commission` is idempotent on `order_id` (UNIQUE) —
//! the use case's re-confirm is a no-op.

use async_trait::async_trait;
use futures::TryStreamExt;
use rust_decimal::Decimal;
use tiberius::ToSql;
use uuid::Uuid;

use kokkak_domain::{
    Commission, Payment, PaymentRepoError, PaymentRepository, PaymentStatus, Payout, PayoutStatus,
};

use crate::db::mssql::MssqlPool;

#[derive(Clone)]
pub struct MssqlPaymentRepository {
    pool: MssqlPool,
}

impl MssqlPaymentRepository {
    pub fn new(pool: MssqlPool) -> Self {
        Self { pool }
    }
}

fn err(e: impl std::fmt::Display) -> PaymentRepoError {
    PaymentRepoError::Backend(e.to_string())
}

fn row_to_payment(row: &tiberius::Row) -> Result<Payment, PaymentRepoError> {
    let id: Uuid = row.get::<Uuid, _>(0).ok_or_else(|| err("missing id"))?;
    let order_id: Uuid = row
        .get::<Uuid, _>(1)
        .ok_or_else(|| err("missing order_id"))?;
    let customer_id: Uuid = row
        .get::<Uuid, _>(2)
        .ok_or_else(|| err("missing customer_id"))?;
    let amount: Decimal = row.get::<Decimal, _>(3).unwrap_or_default();
    let gateway_ref: &str = row.get::<&str, _>(4).unwrap_or("");
    let status: &str = row.get::<&str, _>(5).unwrap_or("pending");
    let currency: &str = row.get::<&str, _>(6).unwrap_or("LAK");
    let created_at = row
        .get::<chrono::DateTime<chrono::Utc>, _>(7)
        .ok_or_else(|| err("missing created_at"))?;
    let updated_at = row
        .get::<chrono::DateTime<chrono::Utc>, _>(8)
        .ok_or_else(|| err("missing updated_at"))?;
    let status = match status {
        "pending" => PaymentStatus::Pending,
        "authorized" => PaymentStatus::Authorized,
        "captured" => PaymentStatus::Captured,
        "failed" => PaymentStatus::Failed,
        "refunded" => PaymentStatus::Refunded,
        _ => return Err(err(format!("unknown status: {status}"))),
    };
    Ok(Payment {
        id,
        order_id,
        customer_id,
        amount,
        gateway_ref: gateway_ref.to_string(),
        status,
        currency: currency.to_string(),
        created_at,
        updated_at,
    })
}

fn row_to_commission(row: &tiberius::Row) -> Result<Commission, PaymentRepoError> {
    let id: Uuid = row.get::<Uuid, _>(0).ok_or_else(|| err("missing id"))?;
    let oid: Uuid = row
        .get::<Uuid, _>(1)
        .ok_or_else(|| err("missing order_id"))?;
    let tech: Uuid = row
        .get::<Uuid, _>(2)
        .ok_or_else(|| err("missing technician_id"))?;
    let gross: Decimal = row.get::<Decimal, _>(3).unwrap_or_default();
    let amount: Decimal = row.get::<Decimal, _>(4).unwrap_or_default();
    let rate: Decimal = row.get::<Decimal, _>(5).unwrap_or_default();
    let net: Decimal = row.get::<Decimal, _>(6).unwrap_or_default();
    let at = row
        .get::<chrono::DateTime<chrono::Utc>, _>(7)
        .ok_or_else(|| err("missing computed_at"))?;
    Ok(Commission {
        id,
        order_id: oid,
        technician_id: tech,
        gross,
        amount,
        rate,
        net_to_tech: net,
        computed_at: at,
    })
}

fn row_to_payout(row: &tiberius::Row) -> Result<Payout, PaymentRepoError> {
    let id: Uuid = row.get::<Uuid, _>(0).ok_or_else(|| err("missing id"))?;
    let tech: Uuid = row.get::<Uuid, _>(1).ok_or_else(|| err("missing tech"))?;
    let oid: Uuid = row.get::<Uuid, _>(2).ok_or_else(|| err("missing order"))?;
    let amount: Decimal = row.get::<Decimal, _>(3).unwrap_or_default();
    let status: &str = row.get::<&str, _>(4).unwrap_or("pending");
    let created_at = row
        .get::<chrono::DateTime<chrono::Utc>, _>(5)
        .ok_or_else(|| err("missing created_at"))?;
    let updated_at = row
        .get::<chrono::DateTime<chrono::Utc>, _>(6)
        .ok_or_else(|| err("missing updated_at"))?;
    let s = match status {
        "pending" => PayoutStatus::Pending,
        "queued" => PayoutStatus::Queued,
        "paid" => PayoutStatus::Paid,
        "failed" => PayoutStatus::Failed,
        _ => return Err(err(format!("unknown status: {status}"))),
    };
    Ok(Payout {
        id,
        technician_id: tech,
        order_id: oid,
        amount,
        status: s,
        created_at,
        updated_at,
    })
}

async fn collect_rows_payment(
    stream: tiberius::QueryStream<'_>,
) -> Result<Vec<tiberius::Row>, PaymentRepoError> {
    let mut s = stream.into_row_stream();
    let mut out = Vec::new();
    while let Some(row) = s.try_next().await.map_err(err)? {
        out.push(row);
    }
    Ok(out)
}

#[async_trait]
impl PaymentRepository for MssqlPaymentRepository {
    async fn insert_payment(&self, payment: &Payment) -> Result<(), PaymentRepoError> {
        let mut conn = self.pool.get().await.map_err(err)?;
        let status = payment.status.as_str();
        match conn
            .execute(
                "INSERT INTO payment(id, order_id, customer_id, amount, gateway_ref, status, currency, created_at, updated_at) \
                 VALUES (@P1, @P2, @P3, @P4, @P5, @P6, @P7, @P8, @P9)",
                &[
                    &payment.id as &dyn ToSql,
                    &payment.order_id as &dyn ToSql,
                    &payment.customer_id as &dyn ToSql,
                    &payment.amount as &dyn ToSql,
                    &payment.gateway_ref as &dyn ToSql,
                    &status as &dyn ToSql,
                    &payment.currency as &dyn ToSql,
                    &payment.created_at as &dyn ToSql,
                    &payment.updated_at as &dyn ToSql,
                ],
            )
            .await
        {
            Ok(_) => Ok(()),
            Err(e) => {
                let s = e.to_string();
                if s.contains("2627") || s.contains("UNIQUE") || s.contains("duplicate") {
                    Err(PaymentRepoError::Backend(format!(
                        "order {} already has a payment",
                        payment.order_id
                    )))
                } else {
                    Err(err(s))
                }
            }
        }
    }

    async fn find_payment(&self, id: Uuid) -> Result<Option<Payment>, PaymentRepoError> {
        let mut conn = self.pool.get().await.map_err(err)?;
        let rows = conn
            .query(
                "SELECT id, order_id, customer_id, amount, gateway_ref, status, currency, created_at, updated_at \
                 FROM payment WHERE id = @P1",
                &[&id as &dyn ToSql],
            )
            .await
            .map_err(err)?;
        let collected = collect_rows_payment(rows).await?;
        if let Some(row) = collected.into_iter().next() {
            return Ok(Some(row_to_payment(&row)?));
        }
        Ok(None)
    }

    async fn find_payment_by_order(
        &self,
        order_id: Uuid,
    ) -> Result<Option<Payment>, PaymentRepoError> {
        let mut conn = self.pool.get().await.map_err(err)?;
        let rows = conn
            .query(
                "SELECT id, order_id, customer_id, amount, gateway_ref, status, currency, created_at, updated_at \
                 FROM payment WHERE order_id = @P1",
                &[&order_id as &dyn ToSql],
            )
            .await
            .map_err(err)?;
        let collected = collect_rows_payment(rows).await?;
        if let Some(row) = collected.into_iter().next() {
            return Ok(Some(row_to_payment(&row)?));
        }
        Ok(None)
    }

    async fn list_payments_for_customer(
        &self,
        customer_id: Uuid,
        limit: u32,
    ) -> Result<Vec<Payment>, PaymentRepoError> {
        let limit_i64 = limit.clamp(1, 200) as i64;
        let mut conn = self.pool.get().await.map_err(err)?;
        let rows = conn
            .query(
                "SELECT TOP (@P1) id, order_id, customer_id, amount, gateway_ref, status, currency, created_at, updated_at \
                 FROM payment WHERE customer_id = @P2 ORDER BY created_at DESC",
                &[&limit_i64 as &dyn ToSql, &customer_id as &dyn ToSql],
            )
            .await
            .map_err(err)?;
        let collected = collect_rows_payment(rows).await?;
        collected.iter().map(row_to_payment).collect()
    }

    async fn update_payment_status(
        &self,
        id: Uuid,
        status: PaymentStatus,
        gateway_ref: Option<&str>,
    ) -> Result<(), PaymentRepoError> {
        let mut conn = self.pool.get().await.map_err(err)?;
        let status_s = status.as_str();
        let now = chrono::Utc::now();
        if let Some(gw) = gateway_ref {
            conn.execute(
                "UPDATE payment SET status = @P1, gateway_ref = @P2, updated_at = @P3 WHERE id = @P4",
                &[
                    &status_s as &dyn ToSql,
                    &gw as &dyn ToSql,
                    &now as &dyn ToSql,
                    &id as &dyn ToSql,
                ],
            )
            .await
            .map_err(err)?;
        } else {
            conn.execute(
                "UPDATE payment SET status = @P1, updated_at = @P2 WHERE id = @P3",
                &[
                    &status_s as &dyn ToSql,
                    &now as &dyn ToSql,
                    &id as &dyn ToSql,
                ],
            )
            .await
            .map_err(err)?;
        }
        Ok(())
    }

    async fn insert_commission(&self, commission: &Commission) -> Result<(), PaymentRepoError> {
        let mut conn = self.pool.get().await.map_err(err)?;
        // UPSERT (idempotent on order_id's UNIQUE constraint):
        // try INSERT, on duplicate do UPDATE.
        match conn
            .execute(
                "INSERT INTO commission(id, order_id, technician_id, gross, amount, rate, net_to_tech, computed_at) \
                 VALUES (@P1, @P2, @P3, @P4, @P5, @P6, @P7, @P8)",
                &[
                    &commission.id as &dyn ToSql,
                    &commission.order_id as &dyn ToSql,
                    &commission.technician_id as &dyn ToSql,
                    &commission.gross as &dyn ToSql,
                    &commission.amount as &dyn ToSql,
                    &commission.rate as &dyn ToSql,
                    &commission.net_to_tech as &dyn ToSql,
                    &commission.computed_at as &dyn ToSql,
                ],
            )
            .await
        {
            Ok(_) => Ok(()),
            Err(e) => {
                let s = e.to_string();
                if s.contains("2627") || s.contains("UNIQUE") || s.contains("duplicate") {
                    conn.execute(
                        "UPDATE commission SET technician_id = @P1, gross = @P2, amount = @P3, rate = @P4, net_to_tech = @P5, computed_at = @P6 WHERE order_id = @P7",
                        &[
                            &commission.technician_id as &dyn ToSql,
                            &commission.gross as &dyn ToSql,
                            &commission.amount as &dyn ToSql,
                            &commission.rate as &dyn ToSql,
                            &commission.net_to_tech as &dyn ToSql,
                            &commission.computed_at as &dyn ToSql,
                            &commission.order_id as &dyn ToSql,
                        ],
                    )
                    .await
                    .map_err(err)?;
                    Ok(())
                } else {
                    Err(err(s))
                }
            }
        }
    }

    async fn find_commission_by_order(
        &self,
        order_id: Uuid,
    ) -> Result<Option<Commission>, PaymentRepoError> {
        let mut conn = self.pool.get().await.map_err(err)?;
        let rows = conn
            .query(
                "SELECT id, order_id, technician_id, gross, amount, rate, net_to_tech, computed_at \
                 FROM commission WHERE order_id = @P1",
                &[&order_id as &dyn ToSql],
            )
            .await
            .map_err(err)?;
        let collected = collect_rows_payment(rows).await?;
        if let Some(row) = collected.into_iter().next() {
            return Ok(Some(row_to_commission(&row)?));
        }
        Ok(None)
    }

    async fn insert_payout(&self, payout: &Payout) -> Result<(), PaymentRepoError> {
        let mut conn = self.pool.get().await.map_err(err)?;
        let status = payout.status.as_str();
        conn.execute(
            "INSERT INTO payout(id, technician_id, order_id, amount, status, created_at, updated_at) \
             VALUES (@P1, @P2, @P3, @P4, @P5, @P6, @P7)",
            &[
                &payout.id as &dyn ToSql,
                &payout.technician_id as &dyn ToSql,
                &payout.order_id as &dyn ToSql,
                &payout.amount as &dyn ToSql,
                &status as &dyn ToSql,
                &payout.created_at as &dyn ToSql,
                &payout.updated_at as &dyn ToSql,
            ],
        )
        .await
        .map_err(err)?;
        Ok(())
    }

    async fn list_payouts(
        &self,
        technician_id: Option<Uuid>,
        status: Option<PayoutStatus>,
        limit: u32,
    ) -> Result<Vec<Payout>, PaymentRepoError> {
        let limit_i64 = limit.clamp(1, 500) as i64;
        let status_s = status.map(|s| s.as_str().to_string());
        let mut conn = self.pool.get().await.map_err(err)?;
        let rows = match (technician_id, status_s.as_deref()) {
            (Some(t), Some(s)) => {
                conn.query(
                    "SELECT TOP (@P1) id, technician_id, order_id, amount, status, created_at, updated_at \
                     FROM payout WHERE technician_id = @P2 AND status = @P3 ORDER BY created_at DESC",
                    &[&limit_i64 as &dyn ToSql, &t as &dyn ToSql, &s as &dyn ToSql],
                )
                .await
                .map_err(err)?
            }
            (Some(t), None) => {
                conn.query(
                    "SELECT TOP (@P1) id, technician_id, order_id, amount, status, created_at, updated_at \
                     FROM payout WHERE technician_id = @P2 ORDER BY created_at DESC",
                    &[&limit_i64 as &dyn ToSql, &t as &dyn ToSql],
                )
                .await
                .map_err(err)?
            }
            (None, Some(s)) => {
                conn.query(
                    "SELECT TOP (@P1) id, technician_id, order_id, amount, status, created_at, updated_at \
                     FROM payout WHERE status = @P2 ORDER BY created_at DESC",
                    &[&limit_i64 as &dyn ToSql, &s as &dyn ToSql],
                )
                .await
                .map_err(err)?
            }
            (None, None) => {
                conn.query(
                    "SELECT TOP (@P1) id, technician_id, order_id, amount, status, created_at, updated_at \
                     FROM payout ORDER BY created_at DESC",
                    &[&limit_i64 as &dyn ToSql],
                )
                .await
                .map_err(err)?
            }
        };
        let collected = collect_rows_payment(rows).await?;
        collected.iter().map(row_to_payout).collect()
    }

    async fn update_payout_status(
        &self,
        id: Uuid,
        status: PayoutStatus,
    ) -> Result<(), PaymentRepoError> {
        let mut conn = self.pool.get().await.map_err(err)?;
        let status_s = status.as_str();
        let now = chrono::Utc::now();
        conn.execute(
            "UPDATE payout SET status = @P1, updated_at = @P2 WHERE id = @P3",
            &[
                &status_s as &dyn ToSql,
                &now as &dyn ToSql,
                &id as &dyn ToSql,
            ],
        )
        .await
        .map_err(err)?;
        Ok(())
    }
}
