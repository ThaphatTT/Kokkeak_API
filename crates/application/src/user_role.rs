

use std::sync::Arc;

use uuid::Uuid;

use kokkak_domain::traits::user::RepoError;
use kokkak_domain::{
    PermissionUpdateRow, UserRolePermission, UserRolePermissionRow, UserRoleRepository,
    UserRoleWithPermissions,
};

#[derive(Debug, Clone)]
pub struct PermissionUpdateInput {

    pub user_role_guid: String,

    pub user_permission_guid: String,

    pub user_role_permission_status: i32,
}

#[derive(Debug, Clone)]
pub struct UpdatePermissionsInput {

    pub updates: Vec<PermissionUpdateInput>,

    pub update_by: Option<String>,
}

pub struct UserRoleService {
    repo: Arc<dyn UserRoleRepository>,
}

impl UserRoleService {

    pub fn new(repo: Arc<dyn UserRoleRepository>) -> Self {
        Self { repo }
    }

    pub async fn list_permissions(
        &self,
        mode: &str,
        caller_guid: Uuid,
    ) -> Result<Vec<UserRoleWithPermissions>, RepoError> {

        let flat = self.repo.list_permissions(mode, caller_guid).await?;
        Ok(group_by_role(flat))
    }

    pub async fn update_permissions(
        &self,
        input: UpdatePermissionsInput,
    ) -> Result<Vec<PermissionUpdateRow>, RepoError> {
        let mut out = Vec::with_capacity(input.updates.len());
        for item in &input.updates {
            let row = self
                .repo
                .update_role_permission(
                    &item.user_role_guid,
                    &item.user_permission_guid,
                    item.user_role_permission_status,
                    input.update_by.as_deref(),
                )
                .await?;
            out.push(row);
        }
        Ok(out)
    }
}

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

    #[derive(Default)]
    struct MockUserRoleRepository {
        rows: Mutex<Vec<UserRolePermissionRow>>,
        last_mode: Mutex<Option<String>>,

        update_responses: Mutex<Vec<PermissionUpdateRow>>,

        update_calls: Mutex<Vec<UpdateCall>>,
    }

    type UpdateCall = (String, String, i32, Option<String>);

    #[async_trait::async_trait]
    impl UserRoleRepository for MockUserRoleRepository {
        async fn list_permissions(
            &self,
            mode: &str,
            _caller_guid: Uuid,
        ) -> Result<Vec<UserRolePermissionRow>, RepoError> {

            *self.last_mode.lock().unwrap() = Some(mode.to_string());
            Ok(self.rows.lock().unwrap().clone())
        }

        async fn update_role_permission(
            &self,
            role_guid: &str,
            permission_guid: &str,
            status: i32,
            update_by: Option<&str>,
        ) -> Result<PermissionUpdateRow, RepoError> {
            self.update_calls.lock().unwrap().push((
                role_guid.to_string(),
                permission_guid.to_string(),
                status,
                update_by.map(str::to_string),
            ));
            let mut queue = self.update_responses.lock().unwrap();
            if queue.is_empty() {
                return Err(RepoError::Backend(
                    "MockUserRoleRepository ran out of canned update responses".into(),
                ));
            }
            Ok(queue.remove(0))
        }
    }

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
            ..Default::default()
        });
        let svc = UserRoleService::new(repo.clone());
        let actor = Uuid::new_v4();
        let groups = svc.list_permissions("SELECT_ADMIN", actor).await.unwrap();
        assert_eq!(
            groups.len(),
            1,
            "4 rows for 1 role must collapse to 1 group"
        );
        assert_eq!(groups[0].user_role_code, "FINANCE_MANAGER");
        assert_eq!(groups[0].permissions.len(), 4);

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

        for p in &groups[0].permissions {
            assert!(!p.user_role_permission_guid.is_empty());
            assert_eq!(p.user_role_permission_status, 1);
        }

        let last = repo.last_mode.lock().unwrap().clone().unwrap();
        assert_eq!(last, "SELECT_ADMIN");
    }

    #[tokio::test]
    async fn includes_ungranted_permissions_alongside_granted() {

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
            ..Default::default()
        });
        let svc = UserRoleService::new(repo);
        let actor = Uuid::new_v4();
        let groups = svc
            .list_permissions("SELECT_EMPLOYEE", actor)
            .await
            .unwrap();

        assert_eq!(groups.len(), 1, "EMPLOYEE must appear once");
        assert_eq!(groups[0].user_role_code, "EMPLOYEE");
        assert_eq!(
            groups[0].permissions.len(),
            3,
            "ALL 3 permissions (1 granted + 2 ungranted) must surface"
        );

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

        let granted = &groups[0].permissions[1];
        assert_eq!(granted.user_permission_code, "FINANCE_ESCROW_RELEASE");
        assert_eq!(
            granted.user_role_permission_guid,
            "rp-EMPLOYEE-FINANCE_ESCROW_RELEASE"
        );
        assert_eq!(granted.user_role_permission_status, 1);

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
            ..Default::default()
        });
        let svc = UserRoleService::new(repo);
        let actor = Uuid::new_v4();
        let groups = svc.list_permissions("SELECT_ADMIN", actor).await.unwrap();
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

        let repo = Arc::new(MockUserRoleRepository::default());
        let svc = UserRoleService::new(repo.clone());
        let actor = Uuid::new_v4();
        let _ = svc
            .list_permissions("SELECT_FUTURE_MODE_X", actor)
            .await
            .unwrap();
        let last = repo.last_mode.lock().unwrap().clone().unwrap();
        assert_eq!(last, "SELECT_FUTURE_MODE_X");
    }

    #[tokio::test]
    async fn empty_repo_returns_empty_vec() {
        let repo = Arc::new(MockUserRoleRepository::default());
        let svc = UserRoleService::new(repo);
        let actor = Uuid::new_v4();
        let groups = svc.list_permissions("SELECT_ADMIN", actor).await.unwrap();
        assert!(groups.is_empty());
    }

    fn update_row_updated(role_guid: &str, perm_guid: &str) -> PermissionUpdateRow {
        PermissionUpdateRow {
            success: true,
            code: PermissionUpdateRow::CODE_UPDATED.to_string(),
            message: "Role permission updated".to_string(),
            user_role_permission_guid: Some(format!("rp-{role_guid}-{perm_guid}")),
            user_role_guid: role_guid.to_string(),
            user_permission_guid: perm_guid.to_string(),
            user_role_permission_status: 1,
        }
    }

    fn update_row_created(role_guid: &str, perm_guid: &str) -> PermissionUpdateRow {
        PermissionUpdateRow {
            success: true,
            code: PermissionUpdateRow::CODE_CREATED.to_string(),
            message: "Role permission created".to_string(),
            user_role_permission_guid: Some(format!("rp-{role_guid}-{perm_guid}")),
            user_role_guid: role_guid.to_string(),
            user_permission_guid: perm_guid.to_string(),
            user_role_permission_status: 1,
        }
    }

    fn update_row_role_not_found(role_guid: &str, perm_guid: &str) -> PermissionUpdateRow {
        PermissionUpdateRow {
            success: false,
            code: PermissionUpdateRow::CODE_ROLE_NOT_FOUND.to_string(),
            message: "user_role_guid not found".to_string(),
            user_role_permission_guid: None,
            user_role_guid: role_guid.to_string(),
            user_permission_guid: perm_guid.to_string(),
            user_role_permission_status: 0,
        }
    }

    fn update_row_permission_not_found(role_guid: &str, perm_guid: &str) -> PermissionUpdateRow {
        PermissionUpdateRow {
            success: false,
            code: PermissionUpdateRow::CODE_PERMISSION_NOT_FOUND.to_string(),
            message: "user_permission_guid not found".to_string(),
            user_role_permission_guid: None,
            user_role_guid: role_guid.to_string(),
            user_permission_guid: perm_guid.to_string(),
            user_role_permission_status: 0,
        }
    }

    #[tokio::test]
    async fn update_permissions_loops_in_order_and_preserves_responses() {

        let repo = Arc::new(MockUserRoleRepository {
            update_responses: Mutex::new(vec![
                update_row_updated("r1", "p1"),
                update_row_created("r1", "p2"),
                update_row_role_not_found("r-missing", "p3"),
            ]),
            ..Default::default()
        });
        let svc = UserRoleService::new(repo.clone());
        let input = UpdatePermissionsInput {
            updates: vec![
                PermissionUpdateInput {
                    user_role_guid: "r1".into(),
                    user_permission_guid: "p1".into(),
                    user_role_permission_status: 1,
                },
                PermissionUpdateInput {
                    user_role_guid: "r1".into(),
                    user_permission_guid: "p2".into(),
                    user_role_permission_status: 1,
                },
                PermissionUpdateInput {
                    user_role_guid: "r-missing".into(),
                    user_permission_guid: "p3".into(),
                    user_role_permission_status: 0,
                },
            ],
            update_by: Some("admin-guid".into()),
        };
        let out = svc.update_permissions(input).await.unwrap();

        assert_eq!(out.len(), 3);
        assert_eq!(out[0].code, PermissionUpdateRow::CODE_UPDATED);
        assert_eq!(out[1].code, PermissionUpdateRow::CODE_CREATED);
        assert_eq!(out[2].code, PermissionUpdateRow::CODE_ROLE_NOT_FOUND);
        assert!(
            !out[2].success,
            "ROLE_NOT_FOUND must surface as success=false"
        );

        let calls = repo.update_calls.lock().unwrap();
        assert_eq!(calls.len(), 3);
        assert_eq!(
            calls[0],
            ("r1".into(), "p1".into(), 1, Some("admin-guid".into()))
        );
        assert_eq!(
            calls[1],
            ("r1".into(), "p2".into(), 1, Some("admin-guid".into()))
        );
        assert_eq!(
            calls[2],
            (
                "r-missing".into(),
                "p3".into(),
                0,
                Some("admin-guid".into())
            )
        );
    }

    #[tokio::test]
    async fn update_permissions_forwards_none_update_by_as_none() {

        let repo = Arc::new(MockUserRoleRepository {
            update_responses: Mutex::new(vec![update_row_updated("r", "p")]),
            ..Default::default()
        });
        let svc = UserRoleService::new(repo.clone());
        let input = UpdatePermissionsInput {
            updates: vec![PermissionUpdateInput {
                user_role_guid: "r".into(),
                user_permission_guid: "p".into(),
                user_role_permission_status: 1,
            }],
            update_by: None,
        };
        let _ = svc.update_permissions(input).await.unwrap();
        let calls = repo.update_calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].3, None, "None must round-trip as None");
    }

    #[tokio::test]
    async fn update_permissions_empty_input_is_a_no_op() {

        let repo = Arc::new(MockUserRoleRepository::default());
        let svc = UserRoleService::new(repo.clone());
        let input = UpdatePermissionsInput {
            updates: vec![],
            update_by: None,
        };
        let out = svc.update_permissions(input).await.unwrap();
        assert!(out.is_empty());
        assert!(repo.update_calls.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn update_permissions_propagates_repo_backend_error() {

        let repo = Arc::new(MockUserRoleRepository::default());
        let svc = UserRoleService::new(repo);
        let input = UpdatePermissionsInput {
            updates: vec![PermissionUpdateInput {
                user_role_guid: "r".into(),
                user_permission_guid: "p".into(),
                user_role_permission_status: 1,
            }],
            update_by: None,
        };
        let err = svc.update_permissions(input).await.unwrap_err();
        match err {
            RepoError::Backend(msg) => {
                assert!(msg.contains("MockUserRoleRepository"), "got: {msg}");
            }
            other => panic!("expected Backend error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn update_permissions_surfaces_permission_not_found() {

        let repo = Arc::new(MockUserRoleRepository {
            update_responses: Mutex::new(vec![update_row_permission_not_found("r", "p-missing")]),
            ..Default::default()
        });
        let svc = UserRoleService::new(repo);
        let input = UpdatePermissionsInput {
            updates: vec![PermissionUpdateInput {
                user_role_guid: "r".into(),
                user_permission_guid: "p-missing".into(),
                user_role_permission_status: 1,
            }],
            update_by: None,
        };
        let out = svc.update_permissions(input).await.unwrap();
        assert_eq!(out.len(), 1);
        assert!(!out[0].success);
        assert_eq!(out[0].code, PermissionUpdateRow::CODE_PERMISSION_NOT_FOUND);
        assert!(out[0].user_role_permission_guid.is_none());
    }
}
