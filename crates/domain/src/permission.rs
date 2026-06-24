//! Role ↔ Permission entities (M15-prep).
//!
//! Three layers of DTO, each with a single responsibility:
//!
//! 1. [`UserRolePermissionRow`] — the flat row the SP returns
//!    (one per role × permission pair; COALESCE'd on the SQL side
//!    so the Rust side never sees NULL). Used by the infra mapper
//!    to hydrate from tiberius rows.
//! 2. [`UserRolePermission`] — the inner object in the wire
//!    payload (4 fields, no role echo).
//! 3. [`UserRoleWithPermissions`] — the outer group (role + a
//!    `Vec<UserRolePermission>`). The handler returns a
//!    `Vec` of these.
//!
//! The grouping step (flat row → nested group) lives in the
//! application service (`UserRoleService::list_permissions`),
//! relying on the SP's `ORDER BY ur.user_role_code, up.user_permission_code`
//! to keep rows for the same role contiguous.
//!
//! ## Grant status encoding (M15)
//!
//! Each flat row encodes one of three states:
//!
//! | `user_role_permission_guid` | `user_role_permission_status` | `user_permission_guid` | Meaning |
//! |-----------------------------|-------------------------------|------------------------|---------|
//! | filled (GUID)               | `1`                           | filled (GUID)          | GRANTED |
//! | empty (`""`)                | `0`                           | filled (GUID)          | UNGRANTED — the permission exists but has NOT been assigned to this role |
//! | empty (`""`)                | `0`                           | empty (`""`)           | Defensive sentinel — the SP shouldn't produce this, but the application layer filters it out without dropping the role group |
//!
//! The wire payload surfaces both GRANTED and UNGRANTED rows so
//! the admin UI can render a full check-matrix in one round-trip.

use serde::{Deserialize, Serialize};

/// One row of the role × permission matrix.
///
/// The struct is intentionally flat (no nested objects) so the
/// tiberius `Row::get::<&str, _>("col_name")` lookups in
/// `mssql_user_role.rs` are 1:1 with the SP's column names. The
/// empty-string / zero defaults come from `COALESCE` on the SQL
/// side — see the module-level doc for the full grant-status
/// encoding table.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct UserRolePermissionRow {
    /// `user_role.user_role_guid` (UNIQUEIDENTIFIER → string).
    pub user_role_guid: String,
    /// `user_role.user_role_code` (snake_case: `customer`, `admin`, ...).
    pub user_role_code: String,

    /// `user_role_permission.user_role_permission_guid` — filled
    /// when the (role, permission) pair has been GRANTED. Empty
    /// (`""`) when the pair is UNGRANTED — i.e. the role exists
    /// and the permission exists in the catalog, but there's no
    /// row in the `user_role_permission` junction for this pair.
    pub user_role_permission_guid: String,
    /// `user_role_permission.user_role_permission_status` — `1`
    /// for GRANTED pairs, `0` for UNGRANTED pairs (COALESCE'd from
    /// the LEFT JOIN miss). Mirrors the SP output verbatim.
    pub user_role_permission_status: i32,

    /// `user_permission.user_permission_guid` — empty only for the
    /// defensive "role-only sentinel" row (the role exists but no
    /// permission rows came back, which the current SP never
    /// produces). For both GRANTED and UNGRANTED rows, this
    /// field carries the real permission guid.
    pub user_permission_guid: String,
    /// `user_permission.user_permission_code` (SCREAMING_SNAKE_CASE).
    pub user_permission_code: String,
}

/// One permission entry within a role group.
///
/// This is the inner object inside [`UserRoleWithPermissions`]. The
/// four fields are a strict subset of [`UserRolePermissionRow`]
/// (the role fields are hoisted up to the parent group).
///
/// Each entry covers **either** a GRANTED permission (junction
/// guid filled, status = 1) **or** an UNGRANTED one (junction
/// guid empty, status = 0). The admin UI pattern-matches on
/// `user_role_permission_guid.is_empty()` to render checked /
/// unchecked boxes — the wire payload must surface **both**
/// flavors so the check-matrix is complete in one round-trip.
///
/// The role-only sentinel row (`user_permission_guid` also empty)
/// is filtered out at the service layer before it reaches this
/// struct, so the inner payload never carries an empty
/// `user_permission_guid`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct UserRolePermission {
    /// `user_role_permission.user_role_permission_guid` — empty
    /// (`""`) when this (role, permission) pair is UNGRANTED.
    pub user_role_permission_guid: String,
    /// `user_role_permission.user_role_permission_status` — `1`
    /// when GRANTED, `0` when UNGRANTED.
    pub user_role_permission_status: i32,
    /// `user_permission.user_permission_guid`.
    pub user_permission_guid: String,
    /// `user_permission.user_permission_code` (SCREAMING_SNAKE_CASE).
    pub user_permission_code: String,
}

/// One role with its permission list — the wire shape of
/// `GET /api/v1/admin/permissions` (grouped by role).
///
/// The flat matrix that comes out of the SP is grouped here at
/// the service layer (single pass, relying on the SP's
/// `ORDER BY ur.user_role_code, up.user_permission_code` to keep
/// rows for the same role contiguous).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct UserRoleWithPermissions {
    /// `user_role.user_role_guid`.
    pub user_role_guid: String,
    /// `user_role.user_role_code`.
    pub user_role_code: String,
    /// Every permission in the catalog for this role, sorted by
    /// `user_permission_code` (preserved from the SP's ORDER BY).
    /// Includes both GRANTED entries (junction guid filled,
    /// status = 1) and UNGRANTED entries (junction guid empty,
    /// status = 0) so the admin UI can render a complete
    /// check-matrix in one round-trip.
    pub permissions: Vec<UserRolePermission>,
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
        // Field names are snake_case (AGENTS.md § 4) so the mobile
        // SDK generator emits strongly-typed clients without manual
        // mapping.
        assert!(json.contains("\"user_role_guid\""));
        assert!(json.contains("\"user_role_permission_status\":1"));
        assert!(json.contains("\"user_permission_code\":\"PAGE_DASHBOARD_VIEW\""));
    }

    #[test]
    fn nested_group_serializes_with_permissions_array() {
        // The wire shape the admin UI consumes — roles hoisted to
        // the top of each object, permissions nested under
        // `permissions` array. The inner objects carry the
        // 4 permission fields only (no role echo).
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
        // Top-level: role fields + permissions array, no role echo inside.
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
        // Make sure role fields are NOT duplicated inside the array.
        let inner = &value["permissions"][0];
        assert!(inner.get("user_role_guid").is_none());
        assert!(inner.get("user_role_code").is_none());
    }

    #[test]
    fn nested_group_with_empty_permissions_array() {
        // Roles with zero permission assignments must still appear
        // so the admin UI can render the "no permissions yet"
        // empty state without a second lookup.
        let group = UserRoleWithPermissions {
            user_role_guid: "30000000-0000-0000-0000-000000000004".into(),
            user_role_code: "FRESH_ROLE".into(),
            permissions: vec![],
        };
        let value: serde_json::Value = serde_json::to_value(&group).unwrap();
        assert_eq!(value["permissions"].as_array().unwrap().len(), 0);
    }
}
