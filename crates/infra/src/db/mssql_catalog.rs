//! SQL Server-backed `ServiceRepository` (M14.5 — stored procedures).
//!
//! See `migrations/20260620000002_sp_service.sql` for SP definitions.
//! Methods without a matching SP return `RepoError::Backend` (the trait
//! contract is fulfilled; admin endpoints that need them land in M15+).

use async_trait::async_trait;
use tiberius::ToSql;
use uuid::Uuid;

use kokkak_domain::pagination::Cursor;
use kokkak_domain::{RepoError, ServiceCategory, ServiceRepository};

use crate::db::mssql::{exec_sp, read_i32, read_str, read_uuid, MssqlPool};

/// SQL Server-backed `ServiceRepository` (M14.5 — stored procedures).
#[derive(Clone)]
pub struct MssqlServiceRepository {
    pool: MssqlPool,
}

impl MssqlServiceRepository {
    /// Construct the repository with a shared `MssqlPool`.
    pub fn new(pool: MssqlPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl ServiceRepository for MssqlServiceRepository {
    async fn find_by_id(&self, id: Uuid) -> Result<Option<ServiceCategory>, RepoError> {
        let rows = exec_sp(
            &self.pool,
            "EXEC dbo.API_SERVICE_GET @p_service_id = @P1",
            &[&id as &dyn ToSql],
        )
        .await?;
        Ok(rows.first().map(row_to_service))
    }

    async fn find_by_code(&self, code: &str) -> Result<Option<ServiceCategory>, RepoError> {
        // M14.5: no dedicated SP; client-side filter on LIST_ACTIVE.
        let rows = exec_sp(
            &self.pool,
            "EXEC dbo.API_SERVICE_LIST_ACTIVE @p_lang_code = @P1",
            &[&code as &dyn ToSql],
        )
        .await?;
        for row in &rows {
            if let Some(c) = read_str(row, "name_en") {
                if c == code {
                    return Ok(Some(row_to_service(row)));
                }
            }
        }
        Ok(None)
    }

    async fn list_active(
        &self,
        _after: Option<Cursor>,
        _limit: u32,
    ) -> Result<Vec<ServiceCategory>, RepoError> {
        let rows = exec_sp(
            &self.pool,
            "EXEC dbo.API_SERVICE_LIST_ACTIVE @p_lang_code = @P1",
            &[&"th" as &dyn ToSql],
        )
        .await?;
        Ok(rows.iter().map(row_to_service).collect())
    }

    async fn insert(&self, _service: &ServiceCategory) -> Result<(), RepoError> {
        Err(RepoError::Backend(
            "MssqlServiceRepository::insert — API_SERVICE_CREATE SP lands in M15+".into(),
        ))
    }
}

/// Hydrate a `ServiceCategory` from `API_SERVICE_GET` / `API_SERVICE_LIST_ACTIVE`.
///
/// Read by column name (not positional index) so the mapper
/// survives future SP-side column reorders. The authoritative SELECT
/// list lives in `migrations/20260620000002_sp_service.sql`:
///
/// | Column            | Consumed as                |
/// |-------------------|----------------------------|
/// | `id`              | `ServiceCategory.id`       |
/// | `name_th`         | `ServiceCategory.code`     |
/// | `priority`        | `ServiceCategory.sort_order`|
///
/// The remaining columns (`category_main_id`, `name_en`, `status`, ...)
/// are not modelled on `ServiceCategory` yet — they fall through to
/// the default-constructed fields.
fn row_to_service(row: &tiberius::Row) -> ServiceCategory {
    let id = read_uuid(row, "id").unwrap_or_else(Uuid::nil);
    let name_th = read_str(row, "name_th").unwrap_or("").to_string();
    ServiceCategory {
        id,
        code: name_th, // name_th serves as a placeholder code
        default_price: None,
        warranty_days: 30,
        active: true,
        sort_order: read_i32(row, "priority").unwrap_or(0),
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    }
}
