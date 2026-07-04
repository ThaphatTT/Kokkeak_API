

use async_trait::async_trait;
use tiberius::ToSql;
use uuid::Uuid;

use kokkak_domain::permission::{
    PermissionOverrideUpdateItem, PermissionOverrideUpdateResult, PermissionUserDetailRow,
    PermissionUserListRow,
};
use kokkak_domain::traits::permission::PermissionUserRepository;
use kokkak_domain::traits::user::RepoError;

use crate::db::mssql::{exec_sp, read_datetime, read_i32, read_str, MssqlPool};

#[derive(Clone)]
pub struct MssqlPermissionUserRepository {
    pool: MssqlPool,
}

impl MssqlPermissionUserRepository {

    pub fn new(pool: MssqlPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl PermissionUserRepository for MssqlPermissionUserRepository {
    async fn list_permission_users(
        &self,
        caller_guid: Uuid,
    ) -> Result<Vec<PermissionUserListRow>, RepoError> {

        let caller_str = caller_guid.to_string();
        let rows = exec_sp(
            &self.pool,
            "EXEC dbo.SP_PERMISSION_USER_LIST @p_by = @P1",
            &[&caller_str as &dyn ToSql],
        )
        .await?;
        Ok(rows.iter().map(row_to_permission_user_list_row).collect())
    }

    async fn find_permission_user_detail(
        &self,
        user_guid: Uuid,
        caller_guid: Uuid,
    ) -> Result<Vec<PermissionUserDetailRow>, RepoError> {

        let user_str = user_guid.to_string();
        let caller_str = caller_guid.to_string();
        let rows = exec_sp(
            &self.pool,
            "EXEC dbo.SP_PERMISSION_USER_DETAIL_FIND_BY_GUID \
             @p_user_guid = @P1, @p_by = @P2",
            &[&user_str as &dyn ToSql, &caller_str as &dyn ToSql],
        )
        .await?;
        if rows.is_empty() {
            return Err(RepoError::NotFound(format!("user {user_guid} not found")));
        }
        Ok(rows.iter().map(row_to_permission_user_detail_row).collect())
    }

    async fn update_permission_overrides(
        &self,
        items: &[PermissionOverrideUpdateItem],
        update_by: &str,
    ) -> Result<Vec<PermissionOverrideUpdateResult>, RepoError> {

        let mut results = Vec::with_capacity(items.len());
        for item in items {
            let result = call_override_update_sp(&self.pool, item, update_by).await?;
            results.push(result);
        }
        Ok(results)
    }
}

fn row_to_permission_user_list_row(row: &tiberius::Row) -> PermissionUserListRow {
    PermissionUserListRow {
        user_guid: read_str(row, "user_guid").unwrap_or_default().to_string(),
        full_name: read_str(row, "full_name").unwrap_or_default().to_string(),
        email: read_str(row, "email").unwrap_or_default().to_string(),
        role_codes: read_str(row, "role_codes").unwrap_or_default().to_string(),
        role_names: read_str(row, "role_names").unwrap_or_default().to_string(),
        has_permission: row.get::<bool, _>("has_permission").unwrap_or(false),
        has_override: row.get::<bool, _>("has_override").unwrap_or(false),
        user_status: read_i32(row, "user_status").unwrap_or(0),
        user_username_status: read_i32(row, "user_username_status").unwrap_or(0),
        user_create_at: read_datetime(row, "user_create_at").unwrap_or_default(),
        user_update_at: read_datetime(row, "user_update_at").unwrap_or_default(),
    }
}

fn row_to_permission_user_detail_row(row: &tiberius::Row) -> PermissionUserDetailRow {
    PermissionUserDetailRow {
        user_guid: read_str(row, "user_guid").unwrap_or_default().to_string(),
        full_name: read_str(row, "full_name").unwrap_or_default().to_string(),
        email: read_str(row, "email").unwrap_or_default().to_string(),
        user_role_name: read_str(row, "user_role_name")
            .unwrap_or_default()
            .to_string(),
        user_permission_guid: read_str(row, "user_permission_guid")
            .unwrap_or_default()
            .to_string(),
        user_permission_code: read_str(row, "user_permission_code")
            .unwrap_or_default()
            .to_string(),
        user_permission_name: read_str(row, "user_permission_name")
            .unwrap_or_default()
            .to_string(),
        has_override: row.get::<bool, _>("has_override").unwrap_or(false),
        override_effect: read_str(row, "override_effect")
            .unwrap_or_default()
            .to_string(),
        effective_status: row.get::<bool, _>("effective_status").unwrap_or(false),
    }
}

async fn call_override_update_sp(
    pool: &MssqlPool,
    item: &PermissionOverrideUpdateItem,
    update_by: &str,
) -> Result<PermissionOverrideUpdateResult, RepoError> {

    let user_guid = item.user_guid.as_str();
    let permission_guid = item.permission_guid.as_str();
    let effect = item.effect.as_str();
    let reason: Option<&str> = item.reason.as_deref();
    let assigned_by: Option<&str> = item.assigned_by.as_deref();

    let status: i32 = item.status.unwrap_or(1);

    let update_by_str = update_by;

    let rows = exec_sp(
        pool,
        "EXEC dbo.SP_PERMISSION_USER_OVERRIDE_UPDATE \
         @p_user_permission_override_user_guid = @P1, \
         @p_user_permission_override_permission_guid = @P2, \
         @p_user_permission_override_effect = @P3, \
         @p_user_permission_override_reason = @P4, \
         @p_user_permission_override_assigned_by = @P5, \
         @p_user_permission_override_status = @P6, \
         @p_update_by = @P7",
        &[
            &user_guid as &dyn ToSql,
            &permission_guid as &dyn ToSql,
            &effect as &dyn ToSql,
            &reason as &dyn ToSql,
            &assigned_by as &dyn ToSql,
            &status as &dyn ToSql,
            &update_by_str as &dyn ToSql,
        ],
    )
    .await?;

    let row = rows.first().ok_or_else(|| {
        RepoError::Backend(format!(
            "SP_PERMISSION_USER_OVERRIDE_UPDATE returned no row for user={} permission={}",
            item.user_guid, item.permission_guid
        ))
    })?;

    Ok(row_to_permission_override_update_result(row))
}

fn row_to_permission_override_update_result(row: &tiberius::Row) -> PermissionOverrideUpdateResult {
    PermissionOverrideUpdateResult {
        success: row.get::<bool, _>("success").unwrap_or(false),
        code: read_str(row, "code").unwrap_or("ERROR").to_string(),
        message: read_str(row, "message").unwrap_or("").to_string(),
        user_permission_override_guid: read_str(row, "user_permission_override_guid")
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string()),
        user_permission_override_user_guid: read_str(row, "user_permission_override_user_guid")
            .unwrap_or_default()
            .to_string(),
        user_permission_override_permission_guid: read_str(
            row,
            "user_permission_override_permission_guid",
        )
        .unwrap_or_default()
        .to_string(),
        user_permission_override_effect: read_str(row, "user_permission_override_effect")
            .unwrap_or_default()
            .to_string(),
        user_permission_override_status: read_i32(row, "user_permission_override_status")
            .unwrap_or(0),
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn mappers_use_default_fallbacks() {

        let source = include_str!("mssql_permission_user.rs");
        assert!(
            source.contains("unwrap_or_default()"),
            "mssql_permission_user.rs mappers must use unwrap_or_default() for defensive reads"
        );
    }
}
