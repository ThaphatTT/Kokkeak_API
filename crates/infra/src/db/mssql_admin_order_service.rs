use async_trait::async_trait;
use tiberius::ToSql;

use kokkak_domain::admin_order_service::{
    AdminCreateOrderResult, AdminOrderDeleteResult, AdminOrderDetailRow, AdminOrderListInput,
    AdminOrderPage, AdminOrderRow, AdminOrderUpdateInput, AdminOrderUpdateResult,
};
use kokkak_domain::traits::admin_order_service::AdminOrderServiceRepository;
use kokkak_domain::traits::user::RepoError;

use crate::db::mssql::{exec_sp, exec_sp_multi, read_guid_str, read_i32, read_str, MssqlPool};

const SP_LIST: &str = "dbo.SP_ORDER_SERVICE_LIST_GET";
const SP_DETAIL: &str = "dbo.SP_ORDER_SERVICE_DETAIL_GET";
const SP_UPDATE: &str = "dbo.SP_ORDER_SERVICE_UPDATE";
const SP_DELETE: &str = "dbo.SP_ORDER_SERVICE_DELETE";

#[derive(Clone)]
pub struct MssqlAdminOrderServiceRepository {
    pool: MssqlPool,
}

impl MssqlAdminOrderServiceRepository {
    pub fn new(pool: MssqlPool) -> Self {
        Self { pool }
    }

    pub fn disabled() -> Self {
        Self {
            pool: crate::db::mssql::build_disabled_pool(),
        }
    }
}

#[async_trait]
impl AdminOrderServiceRepository for MssqlAdminOrderServiceRepository {
    async fn create_full(
        &self,
        actor_user_guid: &str,
        idempotency_key: &str,
        correlation_id: Option<&str>,
        payload_json: &str,
    ) -> Result<AdminCreateOrderResult, RepoError> {
        let rows = exec_sp(
            &self.pool,
            "EXEC dbo.SP_ADMIN_ORDER_SERVICE_CREATE_FULL \
                @p_payload_json = @P1, \
                @p_actor_user_guid = @P2, \
                @p_idempotency_key = @P3, \
                @p_correlation_id = @P4",
            &[
                &payload_json as &dyn ToSql,
                &actor_user_guid as &dyn ToSql,
                &idempotency_key as &dyn ToSql,
                &correlation_id as &dyn ToSql,
            ],
        )
        .await?;

        let result_json = rows
            .first()
            .and_then(|r| r.get::<&str, _>("result_json"))
            .ok_or_else(|| {
                RepoError::Backend("SP_ADMIN_ORDER_SERVICE_CREATE_FULL: missing result_json".into())
            })?;

        serde_json::from_str(result_json)
            .map_err(|e| RepoError::Backend(format!("parse result_json: {e}")))
    }

    async fn list(&self, input: &AdminOrderListInput) -> Result<AdminOrderPage, RepoError> {
        let keyword: Option<&str> = input.keyword.as_deref();
        let status: Option<i32> = input.workflow_status;
        let page: i32 = input.page.max(1) as i32;
        let page_size: i32 = input.page_size.clamp(1, 100) as i32;

        let result_sets = exec_sp_multi(
            &self.pool,
            &format!(
                "EXEC {sp} \
                    @p_keyword = @P1, \
                    @p_workflow_status = @P2, \
                    @p_page_number = @P3, \
                    @p_page_size = @P4",
                sp = SP_LIST
            ),
            &[
                &keyword as &dyn ToSql,
                &status as &dyn ToSql,
                &page as &dyn ToSql,
                &page_size as &dyn ToSql,
            ],
        )
        .await?;

        let mut items = Vec::new();
        let mut total_count: i64 = 0;
        let mut total_page: u32 = 0;
        let mut out_page: u32 = input.page.max(1);
        let mut out_page_size: u32 = input.page_size.clamp(1, 100);

        if let Some(first_set) = result_sets.first() {
            items = first_set
                .iter()
                .map(|row| AdminOrderRow {
                    order_service_header_guid: read_guid_str(row, "order_service_header_guid"),
                    order_no: read_str(row, "order_no").unwrap_or("").to_string(),
                    owner_user_guid: read_guid_str(row, "owner_user_guid"),
                    owner_name: read_str(row, "owner_name").unwrap_or("").to_string(),
                    sourcing_mode: read_i32(row, "sourcing_mode").unwrap_or(1),
                    workflow_status: read_i32(row, "workflow_status").unwrap_or(0),
                    workflow_status_text: read_str(row, "workflow_status_text")
                        .unwrap_or("")
                        .to_string(),
                    currency: read_str(row, "currency").unwrap_or("LAK").to_string(),
                    body_count: read_i32(row, "body_count").unwrap_or(0),
                    total_amount: read_str(row, "total_amount").unwrap_or("0").to_string(),
                    create_at: read_str(row, "create_at").map(|s| s.to_string()),
                    create_by: read_str(row, "create_by").unwrap_or("").to_string(),
                })
                .collect();
        }
        if let Some(second_set) = result_sets.get(1) {
            if let Some(row) = second_set.first() {
                total_count = row.get::<i64, _>("total_records").unwrap_or(0);
                total_page = row.get::<i32, _>("total_pages").unwrap_or(0) as u32;
                out_page = row.get::<i32, _>("page_number").unwrap_or(page) as u32;
                out_page_size = row.get::<i32, _>("page_size").unwrap_or(page_size) as u32;
            }
        }

        Ok(AdminOrderPage {
            items,
            total_count,
            page: out_page,
            page_size: out_page_size,
            total_page,
        })
    }

    async fn detail(&self, order_guid: &str) -> Result<Option<AdminOrderDetailRow>, RepoError> {
        let rows = exec_sp(
            &self.pool,
            &format!(
                "EXEC {sp} \
                    @p_order_service_header_guid = @P1",
                sp = SP_DETAIL
            ),
            &[&order_guid as &dyn ToSql],
        )
        .await?;

        Ok(rows.first().map(|row| AdminOrderDetailRow {
            order_service_header_guid: read_guid_str(row, "order_service_header_guid"),
            order_no: read_str(row, "order_no").unwrap_or("").to_string(),
            owner_user_guid: read_guid_str(row, "owner_user_guid"),
            owner_name: read_str(row, "owner_name").unwrap_or("").to_string(),
            sourcing_mode: read_i32(row, "sourcing_mode").unwrap_or(1),
            approval_policy: read_i32(row, "approval_policy").unwrap_or(1),
            workflow_status: read_i32(row, "workflow_status").unwrap_or(0),
            workflow_status_text: read_str(row, "workflow_status_text")
                .unwrap_or("")
                .to_string(),
            currency: read_str(row, "currency").unwrap_or("LAK").to_string(),
            preferred_payment_method: read_str(row, "preferred_payment_method")
                .unwrap_or("")
                .to_string(),
            note: read_str(row, "note").unwrap_or("").to_string(),
            body_count: read_i32(row, "body_count").unwrap_or(0),
            participant_count: read_i32(row, "participant_count").unwrap_or(0),
            address_count: read_i32(row, "address_count").unwrap_or(0),
            invitation_count: read_i32(row, "invitation_count").unwrap_or(0),
            total_amount: read_str(row, "total_amount").unwrap_or("0").to_string(),
            create_at: read_str(row, "create_at").map(|s| s.to_string()),
            create_by: read_str(row, "create_by").unwrap_or("").to_string(),
            update_at: read_str(row, "update_at").map(|s| s.to_string()),
            update_by: read_str(row, "update_by").unwrap_or("").to_string(),
        }))
    }

    async fn update(
        &self,
        input: &AdminOrderUpdateInput,
    ) -> Result<AdminOrderUpdateResult, RepoError> {
        let guid: &str = &input.order_service_header_guid;
        let status: Option<i32> = input.workflow_status;
        let note: Option<&str> = input.note.as_deref();
        let actor: &str = &input.update_by;

        let rows = exec_sp(
            &self.pool,
            &format!(
                "EXEC {sp} \
                    @p_order_service_header_guid = @P1, \
                    @p_workflow_status = @P2, \
                    @p_note = @P3, \
                    @p_update_by = @P4",
                sp = SP_UPDATE
            ),
            &[
                &guid as &dyn ToSql,
                &status as &dyn ToSql,
                &note as &dyn ToSql,
                &actor as &dyn ToSql,
            ],
        )
        .await?;

        let row = rows
            .first()
            .ok_or_else(|| RepoError::Backend(format!("{SP_UPDATE}: empty result set")))?;

        Ok(AdminOrderUpdateResult {
            success: row.get::<bool, _>("success").unwrap_or(false),
            code: read_str(row, "code").unwrap_or("").to_string(),
            message: read_str(row, "message").unwrap_or("").to_string(),
            order_service_header_guid: read_guid_str(row, "order_service_header_guid"),
        })
    }

    async fn delete(
        &self,
        order_guid: &str,
        actor_user_guid: &str,
    ) -> Result<AdminOrderDeleteResult, RepoError> {
        let rows = exec_sp(
            &self.pool,
            &format!(
                "EXEC {sp} \
                    @p_order_service_header_guid = @P1, \
                    @p_actor_user_guid = @P2",
                sp = SP_DELETE
            ),
            &[&order_guid as &dyn ToSql, &actor_user_guid as &dyn ToSql],
        )
        .await?;

        let row = rows
            .first()
            .ok_or_else(|| RepoError::Backend(format!("{SP_DELETE}: empty result set")))?;

        Ok(AdminOrderDeleteResult {
            success: row.get::<bool, _>("success").unwrap_or(false),
            code: read_str(row, "code").unwrap_or("").to_string(),
            message: read_str(row, "message").unwrap_or("").to_string(),
            order_service_header_guid: read_guid_str(row, "order_service_header_guid"),
        })
    }
}
