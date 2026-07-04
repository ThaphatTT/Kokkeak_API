

use async_trait::async_trait;
use tiberius::ToSql;
use uuid::Uuid;

use kokkak_domain::pagination::Cursor;
use kokkak_domain::{RepoError, ServiceCategory, ServiceRepository};

use crate::db::mssql::{exec_sp, read_i32, read_str, read_uuid, MssqlPool};

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
        let rows = exec_sp(
            &self.pool,
            "EXEC dbo.API_SERVICE_GET @p_service_id = @P1",
            &[&id as &dyn ToSql],
        )
        .await?;
        Ok(rows.first().map(row_to_service))
    }

    async fn find_by_code(&self, code: &str) -> Result<Option<ServiceCategory>, RepoError> {

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

fn row_to_service(row: &tiberius::Row) -> ServiceCategory {
    let id = read_uuid(row, "id").unwrap_or_else(Uuid::nil);
    let name_th = read_str(row, "name_th").unwrap_or("").to_string();
    ServiceCategory {
        id,
        code: name_th,
        default_price: None,
        warranty_days: 30,
        active: true,
        sort_order: read_i32(row, "priority").unwrap_or(0),
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    }
}
