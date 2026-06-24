//! User-role / permission use cases (M15-prep).
//!
//! Thin orchestration layer over [`UserRoleRepository`]:
//! validates that `guid` is supplied when the caller asked for
//! `SELECT_ID`, then forwards to the repository. We intentionally
//! do not post-process the result — the SP is the source of
//! truth for which permissions a role holds, and the admin UI
//! renders whatever the SP returns.
//!
//! ## Granted vs ungranted rows (M15)
//!
//! The SP emits **one row per (role × permission) pair**, including
//! pairs that have **NOT** been granted yet. The grant status is
//! encoded in two fields:
//!
//! | `user_role_permission_guid` | `user_role_permission_status` | Meaning         |
//! |-----------------------------|-------------------------------|-----------------|
//! | filled                      | `1`                           | GRANTED         |
//! | empty (`""`)                | `0`                           | UNGRANTED       |
//!
//! The wire payload must surface **both** granted and ungranted
//! permissions so the admin UI can render checkboxes for the
//! whole permission catalog in one round-trip. We never drop
//! a row that has a real `user_permission_guid` — the only rows
//! we filter are degenerate ones where `user_permission_guid`
//! is also empty (which the current SP never produces, but the
//! defensive guard keeps us safe if a future schema change
//! relaxes the COALESCE contract).
//!
//! Validation errors here map to `RepoError::Backend` (with the
//! pre-localized message baked in). The handler translates that
//! to 400 / 422 using the standard error envelope.

use std::sync::Arc;

use kokkak_domain::traits::user::RepoError;
use kokkak_domain::{
    UserRolePermission, UserRolePermissionRow, UserRoleRepository, UserRoleWithPermissions,
};

/// Application service bundle for the role × permission endpoint.
pub struct UserRoleService {
    repo: Arc<dyn UserRoleRepository>,
}

impl UserRoleService {
    /// Construct the service with a `UserRoleRepository` port.
    pub fn new(repo: Arc<dyn UserRoleRepository>) -> Self {
        Self { repo }
    }

    /// List the role × permission matrix, **grouped by role**
    /// for the admin UI wire format.
    ///
    /// `mode` is a pass-through literal that the SP uses to
    /// scope which role set to return (e.g. `SELECT_ADMIN`,
    /// `SELECT_EMPLOYEE`). The service does not validate the
    /// value — unknown modes return zero rows from the SP,
    /// which we propagate as an empty list (graceful, not 404).
    ///
    /// The flat matrix the SP returns is reshaped here into a
    /// `Vec<UserRoleWithPermissions>` — one entry per active
    /// role, with the role fields hoisted to the top and the
    /// permissions nested under `permissions: Vec<…>`.
    ///
    /// Each role's `permissions` array contains **every**
    /// permission in the catalog (granted + ungranted). Granted
    /// permissions surface with a filled `user_role_permission_guid`
    /// and `status = 1`; ungranted ones surface with an empty
    /// `user_role_permission_guid` and `status = 0`. The admin
    /// UI pattern-matches on those two fields to render
    /// checked / unchecked boxes — the Rust layer must NOT
    /// drop the ungranted rows (that was the bug fixed in M15).
    ///
    /// ponytail: the SP is the source of truth for the matrix
    /// (filters, JOIN order, status gates, supported mode set).
    /// The service only reshapes — no business logic. Ceiling:
    /// when the admin UI needs a second grouping (by permission
    /// → list roles that hold it), add a second method instead
    /// of overloading this one.
    pub async fn list_permissions(
        &self,
        mode: &str,
    ) -> Result<Vec<UserRoleWithPermissions>, RepoError> {
        let flat = self.repo.list_permissions(mode).await?;
        Ok(group_by_role(flat))
    }
}

/// Group the flat SP result into `Vec<UserRoleWithPermissions>`.
///
/// The SP emits rows sorted by `(user_role_code, user_permission_code)`
/// so all rows for the same role are contiguous. We rely on that
/// order to do a single-pass grouping (O(n) — no HashMap needed).
///
/// ## What we keep vs drop
///
/// We keep every row whose `user_permission_guid` is non-empty —
/// that covers both **granted** rows (`user_role_permission_guid`
/// filled, status = 1) and **ungranted** rows (the role × permission
/// pair exists in the catalog but no `user_role_permission`
/// junction row, so `user_role_permission_guid` is empty and
/// status = 0). Both flavors must surface in the wire payload
/// so the admin UI can render a check-matrix.
///
/// We drop only the **degenerate** row whose `user_permission_guid`
/// is also empty — that's the SP's "role-only sentinel" (the role
/// exists but no permission rows came back, which the current SP
/// doesn't actually produce but the defensive guard covers). The
/// role group itself is still emitted with `permissions: []`.
///
/// ponytail: the role-group branch and the append branch share
/// the same predicate + struct-literal logic, so we extract it
/// once. The ceiling is a 5th permission field, at which point
/// a macro-driven mapper earns its keep.
fn group_by_role(rows: Vec<UserRolePermissionRow>) -> Vec<UserRoleWithPermissions> {
    let mut out: Vec<UserRoleWithPermissions> = Vec::new();
    for row in rows {
        match out.last_mut() {
            Some(g) if g.user_role_guid == row.user_role_guid => {
                if let Some(p) = row_to_permission(&row) {
                    g.permissions.push(p);
                }
            }
            _ => {
                let permissions = row_to_permission(&row).into_iter().collect();
                out.push(UserRoleWithPermissions {
                    user_role_guid: row.user_role_guid,
                    user_role_code: row.user_role_code,
                    permissions,
                });
            }
        }
    }
    out
}

/// Map one flat SP row to a wire-shaped permission, dropping only
/// the degenerate "no permission guid at all" case.
fn row_to_permission(row: &UserRolePermissionRow) -> Option<UserRolePermission> {
    if row.user_permission_guid.is_empty() {
        None
    } else {
        Some(UserRolePermission {
            user_role_permission_guid: row.user_role_permission_guid.clone(),
            user_role_permission_status: row.user_role_permission_status,
            user_permission_guid: row.user_permission_guid.clone(),
            user_permission_code: row.user_permission_code.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// In-memory mock of [`UserRoleRepository`] for unit tests.
    /// Stores a flat list of rows the service groups; the
    /// grouping logic is what the tests assert.
    #[derive(Default)]
    struct MockUserRoleRepository {
        rows: Mutex<Vec<UserRolePermissionRow>>,
        last_mode: Mutex<Option<String>>,
    }

    #[async_trait::async_trait]
    impl UserRoleRepository for MockUserRoleRepository {
        async fn list_permissions(
            &self,
            mode: &str,
        ) -> Result<Vec<UserRolePermissionRow>, RepoError> {
            *self.last_mode.lock().unwrap() = Some(mode.to_string());
            Ok(self.rows.lock().unwrap().clone())
        }
    }

    /// Build a flat row that simulates the SP output (role + permission).
    fn flat_row(role_guid: &str, role_code: &str, perm_code: &str) -> UserRolePermissionRow {
        UserRolePermissionRow {
            user_role_guid: role_guid.into(),
            user_role_code: role_code.into(),
            user_role_permission_guid: format!("rp-{}-{}", role_code, perm_code),
            user_role_permission_status: 1,
            user_permission_guid: format!("p-{}", perm_code),
            user_permission_code: perm_code.into(),
        }
    }

    /// Build a flat row for a (role, permission) pair that has NOT
    /// been granted yet. Mirrors the SP output when the role exists
    /// but `user_role_permission` has no row for the pair — the
    /// junction columns COALESCE to `""` / `0`, while the
    /// permission columns stay populated.
    fn ungranted_row(role_guid: &str, role_code: &str, perm_code: &str) -> UserRolePermissionRow {
        UserRolePermissionRow {
            user_role_guid: role_guid.into(),
            user_role_code: role_code.into(),
            user_role_permission_guid: String::new(),
            user_role_permission_status: 0,
            user_permission_guid: format!("p-{}", perm_code),
            user_permission_code: perm_code.into(),
        }
    }

    /// Defensive sentinel: both the junction columns AND the
    /// permission columns are empty. The current SP never produces
    /// this (CROSS JOIN guarantees a permission guid), but if a
    /// future SP change ever does, the role group must still
    /// appear with `permissions: []` so the admin UI doesn't
    /// silently drop the role.
    fn empty_role_sentinel(role_guid: &str, role_code: &str) -> UserRolePermissionRow {
        UserRolePermissionRow {
            user_role_guid: role_guid.into(),
            user_role_code: role_code.into(),
            user_role_permission_guid: String::new(),
            user_role_permission_status: 0,
            user_permission_guid: String::new(),
            user_permission_code: String::new(),
        }
    }

    #[tokio::test]
    async fn groups_rows_by_role() {
        // The SP emits rows in (role, permission) order. The
        // service must group them so the wire payload has one
        // entry per role with a nested permissions array.
        let rows = vec![
            flat_row(
                "30000000-0000-0000-0000-000000000003",
                "FINANCE_MANAGER",
                "FINANCE_ESCROW_RELEASE",
            ),
            flat_row(
                "30000000-0000-0000-0000-000000000003",
                "FINANCE_MANAGER",
                "FINANCE_EXPORT",
            ),
            flat_row(
                "30000000-0000-0000-0000-000000000003",
                "FINANCE_MANAGER",
                "INVOICES_CREATE",
            ),
            flat_row(
                "30000000-0000-0000-0000-000000000003",
                "FINANCE_MANAGER",
                "INVOICES_EXPORT",
            ),
        ];
        let repo = Arc::new(MockUserRoleRepository {
            rows: Mutex::new(rows),
            last_mode: Mutex::new(None),
        });
        let svc = UserRoleService::new(repo.clone());
        let groups = svc.list_permissions("SELECT_ADMIN").await.unwrap();
        assert_eq!(
            groups.len(),
            1,
            "4 rows for 1 role must collapse to 1 group"
        );
        assert_eq!(groups[0].user_role_code, "FINANCE_MANAGER");
        assert_eq!(groups[0].permissions.len(), 4);
        // Order is preserved from the SP's ORDER BY.
        let codes: Vec<&str> = groups[0]
            .permissions
            .iter()
            .map(|p| p.user_permission_code.as_str())
            .collect();
        assert_eq!(
            codes,
            vec![
                "FINANCE_ESCROW_RELEASE",
                "FINANCE_EXPORT",
                "INVOICES_CREATE",
                "INVOICES_EXPORT"
            ]
        );
        // Inner objects must NOT echo the role fields.
        for p in &groups[0].permissions {
            assert!(!p.user_role_permission_guid.is_empty());
            assert_eq!(p.user_role_permission_status, 1);
        }
        // Mode must be passed through to the repo verbatim —
        // no uppercase / lowercase / trim normalisation.
        let last = repo.last_mode.lock().unwrap().clone().unwrap();
        assert_eq!(last, "SELECT_ADMIN");
    }

    #[tokio::test]
    async fn includes_ungranted_permissions_alongside_granted() {
        // The bug fixed in M15: a role with a mix of granted and
        // ungranted permissions used to lose the ungranted ones
        // because the filter checked `user_role_permission_guid`.
        // After the fix, both flavors surface — the admin UI
        // pattern-matches on the empty junction guid to render
        // unchecked boxes.
        let role = "30000000-0000-0000-0000-000000000003";
        let code = "EMPLOYEE";
        let rows = vec![
            ungranted_row(role, code, "COMPANIES_UPDATE"),
            flat_row(role, code, "FINANCE_ESCROW_RELEASE"),
            ungranted_row(role, code, "USERS_APPROVE"),
        ];
        let repo = Arc::new(MockUserRoleRepository {
            rows: Mutex::new(rows),
            last_mode: Mutex::new(None),
        });
        let svc = UserRoleService::new(repo);
        let groups = svc.list_permissions("SELECT_EMPLOYEE").await.unwrap();

        assert_eq!(groups.len(), 1, "EMPLOYEE must appear once");
        assert_eq!(groups[0].user_role_code, "EMPLOYEE");
        assert_eq!(
            groups[0].permissions.len(),
            3,
            "ALL 3 permissions (1 granted + 2 ungranted) must surface"
        );

        // Order is preserved from the SP's ORDER BY.
        let codes: Vec<&str> = groups[0]
            .permissions
            .iter()
            .map(|p| p.user_permission_code.as_str())
            .collect();
        assert_eq!(
            codes,
            vec![
                "COMPANIES_UPDATE",
                "FINANCE_ESCROW_RELEASE",
                "USERS_APPROVE",
            ]
        );

        // Granted row: junction guid filled, status = 1.
        let granted = &groups[0].permissions[1];
        assert_eq!(granted.user_permission_code, "FINANCE_ESCROW_RELEASE");
        assert_eq!(
            granted.user_role_permission_guid,
            "rp-EMPLOYEE-FINANCE_ESCROW_RELEASE"
        );
        assert_eq!(granted.user_role_permission_status, 1);

        // Ungranted rows: junction guid empty, status = 0, but
        // permission guid filled.
        for ungranted in [&groups[0].permissions[0], &groups[0].permissions[2]] {
            assert!(
                ungranted.user_role_permission_guid.is_empty(),
                "ungranted row must have empty junction guid (code = {})",
                ungranted.user_permission_code
            );
            assert_eq!(ungranted.user_role_permission_status, 0);
            assert!(
                !ungranted.user_permission_guid.is_empty(),
                "ungranted row must still carry the permission guid"
            );
        }
    }

    #[tokio::test]
    async fn keeps_empty_sentinel_as_empty_array() {
        // A role with zero permission assignments must still
        // appear so the admin UI can render an "empty state"
        // without a second lookup. The sentinel row (empty
        // user_role_permission_guid) must NOT pollute the
        // permissions array.
        let rows = vec![
            flat_row(
                "30000000-0000-0000-0000-000000000003",
                "FINANCE_MANAGER",
                "FINANCE_ESCROW_RELEASE",
            ),
            empty_role_sentinel("30000000-0000-0000-0000-000000000009", "FRESH_ROLE"),
        ];
        let repo = Arc::new(MockUserRoleRepository {
            rows: Mutex::new(rows),
            last_mode: Mutex::new(None),
        });
        let svc = UserRoleService::new(repo);
        let groups = svc.list_permissions("SELECT_ADMIN").await.unwrap();
        assert_eq!(
            groups.len(),
            2,
            "both roles must appear even if one has no perms"
        );
        assert_eq!(groups[0].user_role_code, "FINANCE_MANAGER");
        assert_eq!(groups[0].permissions.len(), 1);
        assert_eq!(groups[1].user_role_code, "FRESH_ROLE");
        assert_eq!(
            groups[1].permissions.len(),
            0,
            "sentinel must not be promoted to a permission entry"
        );
    }

    #[tokio::test]
    async fn mode_is_passed_through_verbatim() {
        // The mode is a literal the SP interprets — the service
        // must NOT trim, lowercase, or otherwise mangle it.
        // Use a hypothetical extended mode the service doesn't
        // know about to confirm pass-through.
        let repo = Arc::new(MockUserRoleRepository::default());
        let svc = UserRoleService::new(repo.clone());
        let _ = svc.list_permissions("SELECT_FUTURE_MODE_X").await.unwrap();
        let last = repo.last_mode.lock().unwrap().clone().unwrap();
        assert_eq!(last, "SELECT_FUTURE_MODE_X");
    }

    #[tokio::test]
    async fn empty_repo_returns_empty_vec() {
        let repo = Arc::new(MockUserRoleRepository::default());
        let svc = UserRoleService::new(repo);
        let groups = svc.list_permissions("SELECT_ADMIN").await.unwrap();
        assert!(groups.is_empty());
    }
}
