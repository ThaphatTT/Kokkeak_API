

use async_trait::async_trait;
use tiberius::Row;
use tiberius::ToSql;
use uuid::Uuid;

use kokkak_domain::traits::user::RepoError;
use kokkak_domain::{PermissionUpdateRow, UserRolePermissionRow, UserRoleRepository};

use crate::db::mssql::{exec_sp, read_guid_str, read_i32, read_str, MssqlPool};

#[derive(Clone)]
pub struct MssqlUserRoleRepository {
    pool: MssqlPool,
}

impl MssqlUserRoleRepository {

    pub fn new(pool: MssqlPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl UserRoleRepository for MssqlUserRoleRepository {
    async fn list_permissions(
        &self,
        mode: &str,
        caller_guid: Uuid,
    ) -> Result<Vec<UserRolePermissionRow>, RepoError> {

        let caller_str = caller_guid.to_string();
        let rows = exec_sp(
            &self.pool,
            "EXEC dbo.SP_USER_GROUP_ROLE @p_mode = @P1, @p_by = @P2",
            &[&mode as &dyn ToSql, &caller_str as &dyn ToSql],
        )
        .await?;

        Ok(rows.iter().map(row_to_user_role_permission_row).collect())
    }

    async fn update_role_permission(
        &self,
        role_guid: &str,
        permission_guid: &str,
        status: i32,
        update_by: Option<&str>,
    ) -> Result<PermissionUpdateRow, RepoError> {

        let rows = exec_sp(
            &self.pool,
            "EXEC dbo.SP_USER_ROLE_PERMISSION_UPDATE \
                @p_user_role_guid = @P1, \
                @p_user_permission_guid = @P2, \
                @p_user_role_permission_status = @P3, \
                @p_update_by = @P4",
            &[
                &role_guid as &dyn ToSql,
                &permission_guid as &dyn ToSql,
                &status as &dyn ToSql,
                &update_by as &dyn ToSql,
            ],
        )
        .await?;

        match rows.first() {
            Some(row) => Ok(row_to_permission_update_row(row)),
            None => Ok(PermissionUpdateRow::no_change(
                role_guid.to_string(),
                permission_guid.to_string(),
            )),
        }
    }
}

fn row_to_permission_update_row(row: &Row) -> PermissionUpdateRow {
    let success_bit: bool = row.get::<bool, _>("success").unwrap_or(false);
    let code = read_str(row, "code").unwrap_or("").to_string();
    let message = read_str(row, "message").unwrap_or("").to_string();

    let rp_guid_raw = read_guid_str(row, "user_role_permission_guid");
    let role_guid = read_guid_str(row, "user_role_guid");
    let perm_guid = read_guid_str(row, "user_permission_guid");
    let status = read_i32(row, "user_role_permission_status").unwrap_or(0);

    PermissionUpdateRow {
        success: success_bit,
        code,
        message,

        user_role_permission_guid: if rp_guid_raw.is_empty() {
            None
        } else {
            Some(rp_guid_raw)
        },
        user_role_guid: role_guid,
        user_permission_guid: perm_guid,
        user_role_permission_status: status,
    }
}

fn row_to_user_role_permission_row(row: &Row) -> UserRolePermissionRow {
    UserRolePermissionRow {

        user_role_guid: read_guid_str(row, "user_role_guid"),
        user_role_code: read_str(row, "user_role_code").unwrap_or("").to_string(),
        user_role_permission_guid: read_guid_str(row, "user_role_permission_guid"),
        user_role_permission_status: read_i32(row, "user_role_permission_status").unwrap_or(0),
        user_permission_guid: read_guid_str(row, "user_permission_guid"),
        user_permission_code: read_str(row, "user_permission_code")
            .unwrap_or("")
            .to_string(),
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn row_defaults_match_coalesce_contract() {

        let row = UserRolePermissionRow {
            user_role_guid: String::new(),
            user_role_code: String::new(),
            user_role_permission_guid: String::new(),
            user_role_permission_status: 0,
            user_permission_guid: String::new(),
            user_permission_code: String::new(),
        };
        let json = serde_json::to_value(&row).unwrap();
        assert_eq!(json["user_role_guid"], "");
        assert_eq!(json["user_role_permission_status"], 0);
        assert_eq!(json["user_permission_guid"], "");
        assert_eq!(json["user_permission_code"], "");
    }
}
