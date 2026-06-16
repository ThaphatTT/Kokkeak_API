//! SQL Server-backed `ServiceRepository` (M5).
//!
//! Schema (`KOKKAK_CATALOG` database):
//! ```sql
//! CREATE TABLE service_category (
//!     id            UNIQUEIDENTIFIER NOT NULL PRIMARY KEY,
//!     code          NVARCHAR(128)    NOT NULL UNIQUE,
//!     default_price DECIMAL(19,4)    NULL,
//!     warranty_days INT              NOT NULL DEFAULT 30,
//!     active        BIT              NOT NULL DEFAULT 1,
//!     sort_order    INT              NOT NULL DEFAULT 0,
//!     created_at    DATETIME2(7)     NOT NULL,
//!     updated_at    DATETIME2(7)     NOT NULL
//! );
//! ```

use async_trait::async_trait;
use futures::TryStreamExt;
use rust_decimal::Decimal;
use tiberius::ToSql;
use uuid::Uuid;

use kokkak_domain::{Cursor, RepoError, ServiceCategory, ServiceRepository};

use crate::db::mssql::MssqlPool;

#[derive(Clone)]
pub struct MssqlServiceRepository {
    pool: MssqlPool,
}

impl MssqlServiceRepository {
    pub fn new(pool: MssqlPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl ServiceRepository for MssqlServiceRepository {
    async fn find_by_id(&self, id: Uuid) -> Result<Option<ServiceCategory>, RepoError> {
        let mut conn = self
            .pool
            .get()
            .await
            .map_err(|e| RepoError::Backend(format!("acquire: {e}")))?;
        let rows = conn
            .query(
                "SELECT id, code, default_price, warranty_days, active, sort_order, created_at, updated_at \
                 FROM service_category WHERE id = @P1",
                &[&id as &dyn ToSql],
            )
            .await
            .map_err(|e| RepoError::Backend(e.to_string()))?;
        let mut stream = rows.into_row_stream();
        while let Some(row) = stream
            .try_next()
            .await
            .map_err(|e| RepoError::Backend(e.to_string()))?
        {
            return Ok(Some(row_to_service(&row)?));
        }
        Ok(None)
    }

    async fn find_by_code(&self, code: &str) -> Result<Option<ServiceCategory>, RepoError> {
        let mut conn = self
            .pool
            .get()
            .await
            .map_err(|e| RepoError::Backend(format!("acquire: {e}")))?;
        let rows = conn
            .query(
                "SELECT id, code, default_price, warranty_days, active, sort_order, created_at, updated_at \
                 FROM service_category WHERE code = @P1",
                &[&code as &dyn ToSql],
            )
            .await
            .map_err(|e| RepoError::Backend(e.to_string()))?;
        let mut stream = rows.into_row_stream();
        while let Some(row) = stream
            .try_next()
            .await
            .map_err(|e| RepoError::Backend(e.to_string()))?
        {
            return Ok(Some(row_to_service(&row)?));
        }
        Ok(None)
    }

    async fn list_active(
        &self,
        after: Option<Cursor>,
        limit: u32,
    ) -> Result<Vec<ServiceCategory>, RepoError> {
        let after_sort: Option<i32> = match after {
            Some(c) => Some(decode_cursor(&c)?),
            None => None,
        };
        let limit = limit.clamp(1, 200) as i64;
        let mut conn = self
            .pool
            .get()
            .await
            .map_err(|e| RepoError::Backend(format!("acquire: {e}")))?;
        // Use keyset pagination on `sort_order`.
        let rows = if let Some(off) = after_sort {
            conn.query(
                "SELECT id, code, default_price, warranty_days, active, sort_order, created_at, updated_at \
                 FROM service_category WHERE active = 1 AND sort_order > @P1 \
                 ORDER BY sort_order ASC OFFSET 0 ROWS FETCH NEXT @P2 ROWS ONLY",
                &[&off as &dyn ToSql, &limit as &dyn ToSql],
            )
            .await
        } else {
            conn.query(
                "SELECT id, code, default_price, warranty_days, active, sort_order, created_at, updated_at \
                 FROM service_category WHERE active = 1 \
                 ORDER BY sort_order ASC OFFSET 0 ROWS FETCH NEXT @P1 ROWS ONLY",
                &[&limit as &dyn ToSql],
            )
            .await
        }
        .map_err(|e| RepoError::Backend(e.to_string()))?;
        let mut out = Vec::new();
        let mut stream = rows.into_row_stream();
        while let Some(row) = stream
            .try_next()
            .await
            .map_err(|e| RepoError::Backend(e.to_string()))?
        {
            out.push(row_to_service(&row)?);
        }
        Ok(out)
    }

    async fn insert(&self, service: &ServiceCategory) -> Result<(), RepoError> {
        let mut conn = self
            .pool
            .get()
            .await
            .map_err(|e| RepoError::Backend(format!("acquire: {e}")))?;
        conn.execute(
            "INSERT INTO service_category(id, code, default_price, warranty_days, active, sort_order, created_at, updated_at) \
             VALUES (@P1, @P2, @P3, @P4, @P5, @P6, @P7, @P8)",
            &[
                &service.id as &dyn ToSql,
                &service.code as &dyn ToSql,
                &service.default_price as &dyn ToSql,
                &service.warranty_days as &dyn ToSql,
                &service.active as &dyn ToSql,
                &service.sort_order as &dyn ToSql,
                &service.created_at as &dyn ToSql,
                &service.updated_at as &dyn ToSql,
            ],
        )
        .await
        .map_err(|e| {
            let s = e.to_string();
            if s.contains("UNIQUE") || s.contains("2627") {
                RepoError::Conflict(format!("code {} is already taken", service.code))
            } else {
                RepoError::Backend(s)
            }
        })?;
        Ok(())
    }
}

fn row_to_service(row: &tiberius::Row) -> Result<ServiceCategory, RepoError> {
    let id: Uuid = row
        .get::<Uuid, _>(0)
        .ok_or_else(|| RepoError::Backend("missing id".into()))?;
    let code: &str = row
        .get::<&str, _>(1)
        .ok_or_else(|| RepoError::Backend("missing code".into()))?;
    let default_price: Option<Decimal> = row.get::<Decimal, _>(2);
    let warranty_days: i32 = row
        .get::<i32, _>(3)
        .ok_or_else(|| RepoError::Backend("missing warranty_days".into()))?;
    let active: bool = row
        .get::<bool, _>(4)
        .ok_or_else(|| RepoError::Backend("missing active".into()))?;
    let sort_order: i32 = row
        .get::<i32, _>(5)
        .ok_or_else(|| RepoError::Backend("missing sort_order".into()))?;
    let created_at = row
        .get::<chrono::DateTime<chrono::Utc>, _>(6)
        .ok_or_else(|| RepoError::Backend("missing created_at".into()))?;
    let updated_at = row
        .get::<chrono::DateTime<chrono::Utc>, _>(7)
        .ok_or_else(|| RepoError::Backend("missing updated_at".into()))?;
    Ok(ServiceCategory {
        id,
        code: code.to_string(),
        default_price,
        warranty_days,
        active,
        sort_order,
        created_at,
        updated_at,
    })
}

#[derive(serde::Serialize, serde::Deserialize)]
struct CursorPayload {
    after_sort: i32,
}

fn decode_cursor(c: &Cursor) -> Result<i32, RepoError> {
    let p: CursorPayload = c
        .decode()
        .map_err(|e| RepoError::Backend(format!("invalid cursor: {e}")))?;
    Ok(p.after_sort)
}

pub fn encode_cursor(after_sort: i32) -> Result<Cursor, RepoError> {
    Cursor::encode(&CursorPayload { after_sort })
        .map_err(|e| RepoError::Backend(format!("cursor encode: {e}")))
}
