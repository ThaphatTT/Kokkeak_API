//! SQL Server-backed `OrderRepository` (M14.5 — stored procedures).
//!
//! See `migrations/20260620000003_sp_order.sql` for SP definitions.

use async_trait::async_trait;
use rust_decimal::Decimal;
use tiberius::ToSql;
use uuid::Uuid;

use kokkak_domain::pagination::Cursor;
use kokkak_domain::{Order, OrderRepository, RepoError};

use crate::db::mssql::{exec_sp, read_i32, read_str, read_uuid, MssqlPool};

/// SQL Server-backed `OrderRepository` (M14.5 — stored procedures).
#[derive(Clone)]
pub struct MssqlOrderRepository {
    pool: MssqlPool,
}

impl MssqlOrderRepository {
    /// Construct the repository with a shared `MssqlPool`.
    pub fn new(pool: MssqlPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl OrderRepository for MssqlOrderRepository {
    async fn find_by_id(&self, id: Uuid) -> Result<Option<Order>, RepoError> {
        let rows = exec_sp(
            &self.pool,
            "EXEC dbo.API_ORDER_GET @p_order_guid = @P1",
            &[&id as &dyn ToSql],
        )
        .await?;
        if rows.is_empty() {
            return Ok(None);
        }
        // 3 result sets: header, body, assignment.
        let header = &rows[0];
        let customer_id = read_uuid(header, "customer_id").unwrap_or_else(Uuid::nil);
        let status = order_status_from_i32(read_i32(header, "status").unwrap_or(1));
        let address = read_str(header, "address").unwrap_or("").to_string();
        let total = Decimal::from(read_i32(header, "total_amount").unwrap_or(0));
        let created_at = header
            .get::<chrono::DateTime<chrono::Utc>, _>("created_at")
            .unwrap_or_else(chrono::Utc::now);
        let service_code = rows
            .get(1)
            .and_then(|b| read_str(b, "service_id").map(|s| s.to_string()))
            .unwrap_or_default();
        let description = rows
            .get(1)
            .and_then(|b| read_str(b, "description").map(|s| s.to_string()))
            .unwrap_or_default();
        let technician_id = rows.get(2).and_then(|a| read_uuid(a, "technician_id"));
        Ok(Some(Order {
            id,
            service_code,
            customer_id,
            technician_id,
            description,
            address,
            total,
            status,
            created_at,
            updated_at: created_at,
        }))
    }

    async fn list_for_customer(
        &self,
        customer_id: Uuid,
        _after: Option<Cursor>,
        limit: u32,
    ) -> Result<Vec<Order>, RepoError> {
        let rows = exec_sp(
            &self.pool,
            "EXEC dbo.API_ORDER_LIST_BY_CUSTOMER \
                @p_customer_guid = @P1, @p_limit = @P2, @p_offset = @P3",
            &[
                &customer_id as &dyn ToSql,
                &(limit as i32) as &dyn ToSql,
                &0_i32 as &dyn ToSql,
            ],
        )
        .await?;
        Ok(rows.iter().map(|r| header_row_to_order(r, None)).collect())
    }

    async fn list_for_technician(
        &self,
        technician_id: Uuid,
        _after: Option<Cursor>,
        limit: u32,
    ) -> Result<Vec<Order>, RepoError> {
        let rows = exec_sp(
            &self.pool,
            "EXEC dbo.API_ORDER_LIST_BY_TECHNICIAN \
                @p_technician_guid = @P1, @p_limit = @P2, @p_offset = @P3",
            &[
                &technician_id as &dyn ToSql,
                &(limit as i32) as &dyn ToSql,
                &0_i32 as &dyn ToSql,
            ],
        )
        .await?;
        Ok(rows
            .iter()
            .map(|r| header_row_to_order(r, Some(technician_id)))
            .collect())
    }

    async fn insert(&self, order: &Order) -> Result<(), RepoError> {
        let total = order.total;
        let _ = exec_sp(
            &self.pool,
            "EXEC dbo.API_ORDER_CREATE \
                @p_customer_guid = @P1, @p_service_id = @P2, \
                @p_address = @P3, @p_description = @P4, \
                @p_latitude = @P5, @p_longitude = @P6, \
                @p_total_amount = @P7",
            &[
                &order.customer_id as &dyn ToSql,
                &order.service_code as &dyn ToSql,
                &order.address as &dyn ToSql,
                &order.description as &dyn ToSql,
                &None::<Decimal> as &dyn ToSql,
                &None::<Decimal> as &dyn ToSql,
                &total as &dyn ToSql,
            ],
        )
        .await?;
        Ok(())
    }
}

fn header_row_to_order(row: &tiberius::Row, technician_id: Option<Uuid>) -> Order {
    let id = read_uuid(row, "id").unwrap_or_else(Uuid::nil);
    let customer_id = read_uuid(row, "customer_id").unwrap_or_else(Uuid::nil);
    let status = order_status_from_i32(read_i32(row, "status").unwrap_or(1));
    let total = Decimal::from(read_i32(row, "total_amount").unwrap_or(0));
    let created_at = row
        .get::<chrono::DateTime<chrono::Utc>, _>("created_at")
        .unwrap_or_else(chrono::Utc::now);
    Order {
        id,
        service_code: String::new(),
        customer_id,
        technician_id,
        description: String::new(),
        address: String::new(),
        total,
        status,
        created_at,
        updated_at: created_at,
    }
}

fn order_status_from_i32(v: i32) -> kokkak_domain::OrderStatus {
    use kokkak_domain::OrderStatus::*;
    match v {
        1 => Active,
        2 => Active, // alias
        3 => Completed,
        4 => Closed,
        5 => Cancelled,
        _ => Pending,
    }
}
