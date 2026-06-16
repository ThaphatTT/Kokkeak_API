//! SQL Server-backed `OrderRepository` (M5).
//!
//! Schema (`KOKKAK_ORDER` database):
//! ```sql
//! CREATE TABLE [order] (
//!     id            UNIQUEIDENTIFIER NOT NULL PRIMARY KEY,
//!     service_code  NVARCHAR(128)    NOT NULL,
//!     customer_id   UNIQUEIDENTIFIER NOT NULL,
//!     technician_id UNIQUEIDENTIFIER NULL,
//!     description   NVARCHAR(MAX)    NOT NULL,
//!     address       NVARCHAR(MAX)    NOT NULL,
//!     total         DECIMAL(19,4)    NOT NULL,
//!     status        NVARCHAR(32)     NOT NULL,
//!     created_at    DATETIME2(7)     NOT NULL,
//!     updated_at    DATETIME2(7)     NOT NULL
//! );
//! CREATE INDEX ix_order_customer   ON [order] (customer_id, created_at DESC);
//! CREATE INDEX ix_order_technician ON [order] (technician_id, created_at DESC);
//! ```

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use futures::TryStreamExt;
use rust_decimal::Decimal;
use tiberius::ToSql;
use uuid::Uuid;

use kokkak_domain::{Cursor, Order, OrderRepository, OrderStatus, RepoError};

use crate::db::mssql::MssqlPool;

#[derive(Clone)]
pub struct MssqlOrderRepository {
    pool: MssqlPool,
}

impl MssqlOrderRepository {
    pub fn new(pool: MssqlPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl OrderRepository for MssqlOrderRepository {
    async fn find_by_id(&self, id: Uuid) -> Result<Option<Order>, RepoError> {
        let mut conn = self
            .pool
            .get()
            .await
            .map_err(|e| RepoError::Backend(format!("acquire: {e}")))?;
        let rows = conn
            .query(
                "SELECT id, service_code, customer_id, technician_id, description, address, total, status, created_at, updated_at \
                 FROM [order] WHERE id = @P1",
                &[&id as &dyn ToSql],
            )
            .await
            .map_err(|e| RepoError::Backend(e.to_string()))?;
        let collected: Vec<tiberius::Row> = {
            let mut s = rows.into_row_stream();
            let mut out = Vec::new();
            while let Some(row) = s
                .try_next()
                .await
                .map_err(|e| RepoError::Backend(e.to_string()))?
            {
                out.push(row);
            }
            out
        };
        if let Some(row) = collected.into_iter().next() {
            return Ok(Some(row_to_order(&row)?));
        }
        Ok(None)
    }

    async fn list_for_customer(
        &self,
        customer_id: Uuid,
        after: Option<Cursor>,
        limit: u32,
    ) -> Result<Vec<Order>, RepoError> {
        let after_time: Option<DateTime<Utc>> = match after {
            Some(c) => Some(decode_cursor(&c)?),
            None => None,
        };
        let limit = limit.clamp(1, 200) as i64;
        let mut conn = self
            .pool
            .get()
            .await
            .map_err(|e| RepoError::Backend(format!("acquire: {e}")))?;
        let rows = if let Some(t) = after_time {
            conn.query(
                "SELECT id, service_code, customer_id, technician_id, description, address, total, status, created_at, updated_at \
                 FROM [order] WHERE customer_id = @P1 AND created_at < @P2 \
                 ORDER BY created_at DESC OFFSET 0 ROWS FETCH NEXT @P3 ROWS ONLY",
                &[&customer_id as &dyn ToSql, &t as &dyn ToSql, &limit as &dyn ToSql],
            )
            .await
        } else {
            conn.query(
                "SELECT id, service_code, customer_id, technician_id, description, address, total, status, created_at, updated_at \
                 FROM [order] WHERE customer_id = @P1 \
                 ORDER BY created_at DESC OFFSET 0 ROWS FETCH NEXT @P2 ROWS ONLY",
                &[&customer_id as &dyn ToSql, &limit as &dyn ToSql],
            )
            .await
        }
        .map_err(|e| RepoError::Backend(e.to_string()))?;
        collect_orders(rows).await
    }

    async fn list_for_technician(
        &self,
        technician_id: Uuid,
        after: Option<Cursor>,
        limit: u32,
    ) -> Result<Vec<Order>, RepoError> {
        let after_time: Option<DateTime<Utc>> = match after {
            Some(c) => Some(decode_cursor(&c)?),
            None => None,
        };
        let limit = limit.clamp(1, 200) as i64;
        let mut conn = self
            .pool
            .get()
            .await
            .map_err(|e| RepoError::Backend(format!("acquire: {e}")))?;
        let rows = if let Some(t) = after_time {
            conn.query(
                "SELECT id, service_code, customer_id, technician_id, description, address, total, status, created_at, updated_at \
                 FROM [order] WHERE technician_id = @P1 AND created_at < @P2 \
                 ORDER BY created_at DESC OFFSET 0 ROWS FETCH NEXT @P3 ROWS ONLY",
                &[&technician_id as &dyn ToSql, &t as &dyn ToSql, &limit as &dyn ToSql],
            )
            .await
        } else {
            conn.query(
                "SELECT id, service_code, customer_id, technician_id, description, address, total, status, created_at, updated_at \
                 FROM [order] WHERE technician_id = @P1 \
                 ORDER BY created_at DESC OFFSET 0 ROWS FETCH NEXT @P2 ROWS ONLY",
                &[&technician_id as &dyn ToSql, &limit as &dyn ToSql],
            )
            .await
        }
        .map_err(|e| RepoError::Backend(e.to_string()))?;
        collect_orders(rows).await
    }

    async fn insert(&self, order: &Order) -> Result<(), RepoError> {
        let mut conn = self
            .pool
            .get()
            .await
            .map_err(|e| RepoError::Backend(format!("acquire: {e}")))?;
        let status = order.status.as_str();
        conn.execute(
            "INSERT INTO [order](id, service_code, customer_id, technician_id, description, address, total, status, created_at, updated_at) \
             VALUES (@P1, @P2, @P3, @P4, @P5, @P6, @P7, @P8, @P9, @P10)",
            &[
                &order.id as &dyn ToSql,
                &order.service_code as &dyn ToSql,
                &order.customer_id as &dyn ToSql,
                &order.technician_id as &dyn ToSql,
                &order.description as &dyn ToSql,
                &order.address as &dyn ToSql,
                &order.total as &dyn ToSql,
                &status as &dyn ToSql,
                &order.created_at as &dyn ToSql,
                &order.updated_at as &dyn ToSql,
            ],
        )
        .await
        .map_err(|e| {
            let s = e.to_string();
            if s.contains("2627") {
                RepoError::Conflict("id exists".into())
            } else {
                RepoError::Backend(s)
            }
        })?;
        Ok(())
    }
}

async fn collect_orders<'a>(rows: tiberius::QueryStream<'a>) -> Result<Vec<Order>, RepoError> {
    let mut out = Vec::new();
    let mut stream = rows.into_row_stream();
    while let Some(row) = stream
        .try_next()
        .await
        .map_err(|e| RepoError::Backend(e.to_string()))?
    {
        out.push(row_to_order(&row)?);
    }
    Ok(out)
}

fn row_to_order(row: &tiberius::Row) -> Result<Order, RepoError> {
    let id: Uuid = row
        .get::<Uuid, _>(0)
        .ok_or_else(|| RepoError::Backend("missing id".into()))?;
    let service_code: &str = row
        .get::<&str, _>(1)
        .ok_or_else(|| RepoError::Backend("missing service_code".into()))?;
    let customer_id: Uuid = row
        .get::<Uuid, _>(2)
        .ok_or_else(|| RepoError::Backend("missing customer_id".into()))?;
    let technician_id: Option<Uuid> = row.get::<Uuid, _>(3);
    let description: &str = row
        .get::<&str, _>(4)
        .ok_or_else(|| RepoError::Backend("missing description".into()))?;
    let address: &str = row
        .get::<&str, _>(5)
        .ok_or_else(|| RepoError::Backend("missing address".into()))?;
    let total: Decimal = row
        .get::<Decimal, _>(6)
        .ok_or_else(|| RepoError::Backend("missing total".into()))?;
    let status: &str = row
        .get::<&str, _>(7)
        .ok_or_else(|| RepoError::Backend("missing status".into()))?;
    let created_at = row
        .get::<chrono::DateTime<chrono::Utc>, _>(8)
        .ok_or_else(|| RepoError::Backend("missing created_at".into()))?;
    let updated_at = row
        .get::<chrono::DateTime<chrono::Utc>, _>(9)
        .ok_or_else(|| RepoError::Backend("missing updated_at".into()))?;
    let status = match status {
        "pending" => OrderStatus::Pending,
        "active" => OrderStatus::Active,
        "completed" => OrderStatus::Completed,
        "closed" => OrderStatus::Closed,
        "cancelled" => OrderStatus::Cancelled,
        other => return Err(RepoError::Backend(format!("unknown status: {other}"))),
    };
    Ok(Order {
        id,
        service_code: service_code.to_string(),
        customer_id,
        technician_id,
        description: description.to_string(),
        address: address.to_string(),
        total,
        status,
        created_at,
        updated_at,
    })
}

#[derive(serde::Serialize, serde::Deserialize)]
struct CursorPayload {
    after: DateTime<Utc>,
}

fn decode_cursor(c: &Cursor) -> Result<DateTime<Utc>, RepoError> {
    let p: CursorPayload = c
        .decode()
        .map_err(|e| RepoError::Backend(format!("invalid cursor: {e}")))?;
    Ok(p.after)
}

pub fn encode_cursor(after: DateTime<Utc>) -> Result<Cursor, RepoError> {
    Cursor::encode(&CursorPayload { after })
        .map_err(|e| RepoError::Backend(format!("cursor encode: {e}")))
}
