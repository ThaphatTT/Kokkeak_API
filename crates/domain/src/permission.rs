

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct UserRolePermissionRow {

    pub user_role_guid: String,

    pub user_role_code: String,

    pub user_role_permission_guid: String,

    pub user_role_permission_status: i32,

    pub user_permission_guid: String,

    pub user_permission_code: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct UserRolePermission {

    pub user_role_permission_guid: String,

    pub user_role_permission_status: i32,

    pub user_permission_guid: String,

    pub user_permission_code: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct UserRoleWithPermissions {

    pub user_role_guid: String,

    pub user_role_code: String,

    pub permissions: Vec<UserRolePermission>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct PermissionUpdateRow {

    pub success: bool,

    pub code: String,

    pub message: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_role_permission_guid: Option<String>,

    pub user_role_guid: String,

    pub user_permission_guid: String,

    pub user_role_permission_status: i32,
}

impl PermissionUpdateRow {

    pub const CODE_UPDATED: &'static str = "UPDATED";

    pub const CODE_CREATED: &'static str = "CREATED";

    pub const CODE_INSERTED: &'static str = "CREATED";

    pub const CODE_NO_CHANGE: &'static str = "NO_CHANGE";

    pub const CODE_ROLE_NOT_FOUND: &'static str = "ROLE_NOT_FOUND";

    pub const CODE_PERMISSION_NOT_FOUND: &'static str = "PERMISSION_NOT_FOUND";

    pub const CODE_INVALID_STATUS: &'static str = "INVALID_STATUS";

    pub fn no_change(role_guid: String, permission_guid: String) -> Self {
        Self {
            success: true,
            code: Self::CODE_NO_CHANGE.to_string(),
            message: "no change (permission was not granted)".to_string(),
            user_role_permission_guid: None,
            user_role_guid: role_guid,
            user_permission_guid: permission_guid,
            user_role_permission_status: 0,
        }
    }

    pub fn is_success(&self) -> bool {
        self.success
            && (self.code == Self::CODE_UPDATED
                || self.code == Self::CODE_CREATED
                || self.code == Self::CODE_NO_CHANGE)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct PermissionUserListRow {

    pub user_guid: String,

    pub full_name: String,

    pub email: String,

    pub role_codes: String,

    pub role_names: String,

    pub has_permission: bool,

    pub has_override: bool,

    pub user_status: i32,

    pub user_username_status: i32,

    pub user_create_at: DateTime<Utc>,

    pub user_update_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct PermissionUserDetailRow {

    pub user_guid: String,

    pub full_name: String,

    pub email: String,

    pub user_role_name: String,

    pub user_permission_guid: String,

    pub user_permission_code: String,

    pub user_permission_name: String,

    pub has_override: bool,

    pub override_effect: String,

    pub effective_status: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct PermissionUserGroup {

    pub user_guid: String,

    pub full_name: String,

    pub email: String,

    pub user_role_name: String,

    pub permissions: Vec<PermissionUserGroupEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct PermissionUserGroupEntry {

    pub user_permission_guid: String,

    pub user_permission_code: String,

    pub has_override: bool,

    pub effective_status: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct PermissionOverrideUpdateItem {

    pub user_guid: String,

    pub permission_guid: String,

    pub effect: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assigned_by: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct PermissionOverrideUpdateResult {

    pub success: bool,

    pub code: String,

    pub message: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_permission_override_guid: Option<String>,

    pub user_permission_override_user_guid: String,

    pub user_permission_override_permission_guid: String,

    pub user_permission_override_effect: String,

    pub user_permission_override_status: i32,
}

impl PermissionOverrideUpdateResult {

    pub const CODE_UPDATED: &'static str = "UPDATED";

    pub const CODE_CREATED: &'static str = "CREATED";

    pub const CODE_INVALID_EFFECT: &'static str = "INVALID_EFFECT";

    pub const CODE_INVALID_STATUS: &'static str = "INVALID_STATUS";

    pub const CODE_USER_NOT_FOUND: &'static str = "USER_NOT_FOUND";

    pub const CODE_PERMISSION_NOT_FOUND: &'static str = "PERMISSION_NOT_FOUND";

    pub const CODE_ERROR: &'static str = "ERROR";

    pub fn is_success(&self) -> bool {
        self.success && (self.code == Self::CODE_UPDATED || self.code == Self::CODE_CREATED)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn row_serializes_snake_case_for_api_consumers() {
        let row = UserRolePermissionRow {
            user_role_guid: "11111111-1111-1111-1111-000000000003".into(),
            user_role_code: "admin".into(),
            user_role_permission_guid: "rp-guid".into(),
            user_role_permission_status: 1,
            user_permission_guid: "p-guid".into(),
            user_permission_code: "PAGE_DASHBOARD_VIEW".into(),
        };
        let json = serde_json::to_string(&row).unwrap();

        assert!(json.contains("\"user_role_guid\""));
        assert!(json.contains("\"user_role_permission_status\":1"));
        assert!(json.contains("\"user_permission_code\":\"PAGE_DASHBOARD_VIEW\""));
    }

    #[test]
    fn nested_group_serializes_with_permissions_array() {

        let group = UserRoleWithPermissions {
            user_role_guid: "30000000-0000-0000-0000-000000000003".into(),
            user_role_code: "FINANCE_MANAGER".into(),
            permissions: vec![
                UserRolePermission {
                    user_role_permission_guid: "17ED709B-EA96-4949-8C18-4392224EFB0E".into(),
                    user_role_permission_status: 1,
                    user_permission_guid: "1557D692-A45B-4723-A722-3684F86F5F2F".into(),
                    user_permission_code: "INVOICES_EXPORT".into(),
                },
                UserRolePermission {
                    user_role_permission_guid: "2A721AE7-9B47-4866-8C28-24EB826233FC".into(),
                    user_role_permission_status: 1,
                    user_permission_guid: "42303992-5487-4F31-8551-004677961D78".into(),
                    user_permission_code: "FINANCE_ESCROW_RELEASE".into(),
                },
            ],
        };
        let value: serde_json::Value = serde_json::to_value(&group).unwrap();

        assert_eq!(
            value["user_role_guid"],
            "30000000-0000-0000-0000-000000000003"
        );
        assert_eq!(value["user_role_code"], "FINANCE_MANAGER");
        assert_eq!(value["permissions"].as_array().unwrap().len(), 2);
        assert_eq!(
            value["permissions"][0]["user_permission_code"],
            "INVOICES_EXPORT"
        );
        assert_eq!(
            value["permissions"][1]["user_permission_code"],
            "FINANCE_ESCROW_RELEASE"
        );

        let inner = &value["permissions"][0];
        assert!(inner.get("user_role_guid").is_none());
        assert!(inner.get("user_role_code").is_none());
    }

    #[test]
    fn nested_group_with_empty_permissions_array() {

        let group = UserRoleWithPermissions {
            user_role_guid: "30000000-0000-0000-0000-000000000004".into(),
            user_role_code: "FRESH_ROLE".into(),
            permissions: vec![],
        };
        let value: serde_json::Value = serde_json::to_value(&group).unwrap();
        assert_eq!(value["permissions"].as_array().unwrap().len(), 0);
    }
}
