

use std::sync::Arc;

use kokkak_domain::permission::{
    PermissionOverrideUpdateItem, PermissionOverrideUpdateResult, PermissionUserDetailRow,
    PermissionUserGroup, PermissionUserListRow,
};
use kokkak_domain::traits::permission::PermissionUserRepository;
use kokkak_domain::traits::user::RepoError;
use uuid::Uuid;

pub struct PermissionUserListPage {

    pub items: Vec<PermissionUserListRow>,

    pub next_cursor: Option<String>,
}

pub struct PermissionUserService {

    permission_users: Arc<dyn PermissionUserRepository>,
}

impl PermissionUserService {

    pub fn new(permission_users: Arc<dyn PermissionUserRepository>) -> Self {
        Self { permission_users }
    }

    pub async fn list_permission_users(
        &self,
        after: Option<String>,
        limit: u32,
        actor: Uuid,
    ) -> Result<PermissionUserListPage, RepoError> {
        let rows = self.permission_users.list_permission_users(actor).await?;
        Ok(apply_cursor_pagination(rows, after.as_deref(), limit))
    }

    pub async fn get_permission_user_group(
        &self,
        user_guid: Uuid,
        actor: Uuid,
    ) -> Result<PermissionUserGroup, RepoError> {
        let rows = self
            .permission_users
            .find_permission_user_detail(user_guid, actor)
            .await?;
        Ok(group_permission_user_rows(&rows))
    }

    pub async fn get_permission_user_detail(
        &self,
        user_guid: Uuid,
        actor: Uuid,
    ) -> Result<Vec<PermissionUserDetailRow>, RepoError> {
        self.permission_users
            .find_permission_user_detail(user_guid, actor)
            .await
    }

    pub async fn update_permission_overrides(
        &self,
        items: &[PermissionOverrideUpdateItem],
        actor: Uuid,
    ) -> Result<Vec<PermissionOverrideUpdateResult>, RepoError> {
        self.permission_users
            .update_permission_overrides(items, &actor.to_string())
            .await
    }
}

fn group_permission_user_rows(rows: &[PermissionUserDetailRow]) -> PermissionUserGroup {

    let user_guid = rows
        .first()
        .map(|r| r.user_guid.clone())
        .unwrap_or_default();
    let full_name = rows
        .first()
        .map(|r| r.full_name.clone())
        .unwrap_or_default();
    let email = rows.first().map(|r| r.email.clone()).unwrap_or_default();
    let user_role_name = rows
        .first()
        .map(|r| r.user_role_name.clone())
        .unwrap_or_default();

    let permissions = rows
        .iter()
        .map(|r| kokkak_domain::permission::PermissionUserGroupEntry {
            user_permission_code: r.user_permission_code.clone(),
            user_permission_guid: r.user_permission_guid.clone(),
            has_override: r.has_override,
            effective_status: r.effective_status,
        })
        .collect();

    PermissionUserGroup {
        user_guid,
        full_name,
        email,
        user_role_name,
        permissions,
    }
}

fn apply_cursor_pagination(
    rows: Vec<PermissionUserListRow>,
    after: Option<&str>,
    limit: u32,
) -> PermissionUserListPage {
    let start = match after {
        Some(cursor) => rows
            .iter()
            .position(|r| r.email.as_str() > cursor)
            .unwrap_or(rows.len()),
        None => 0,
    };
    let end = (start + limit as usize).min(rows.len());
    let items = rows[start..end].to_vec();
    let next_cursor = if end < rows.len() {
        items.last().map(|r| r.email.clone())
    } else {
        None
    };
    PermissionUserListPage { items, next_cursor }
}

#[cfg(test)]
mod tests {

    use super::*;
    use kokkak_domain::permission::{
        PermissionOverrideUpdateItem, PermissionOverrideUpdateResult, PermissionUserDetailRow,
        PermissionUserGroupEntry,
    };

    fn list_row(email: &str) -> PermissionUserListRow {
        PermissionUserListRow {
            user_guid: format!("00000000-0000-0000-0000-{email:0>12}"),
            full_name: email.to_string(),
            email: email.to_string(),
            role_codes: "SUPER_ADMIN".to_string(),
            role_names: "Super Admin".to_string(),
            has_permission: true,
            has_override: false,
            user_status: 1,
            user_username_status: 1,
            user_create_at: chrono::DateTime::<chrono::Utc>::default(),
            user_update_at: chrono::DateTime::<chrono::Utc>::default(),
        }
    }

    fn detail_row(
        user_guid: &str,
        code: &str,
        has_override: bool,
        effective_status: bool,
    ) -> PermissionUserDetailRow {
        PermissionUserDetailRow {
            user_guid: user_guid.to_string(),
            full_name: "Test User".to_string(),
            email: "test@x".to_string(),
            user_role_name: "Super Admin".to_string(),
            user_permission_guid: format!("p-{code}"),
            user_permission_code: code.to_string(),
            user_permission_name: format!("display for {code}"),
            has_override,
            override_effect: String::new(),
            effective_status,
        }
    }

    struct MockPermissionUserRepo {
        list: Vec<PermissionUserListRow>,
        detail: Vec<PermissionUserDetailRow>,

        override_calls: std::sync::Mutex<Vec<Vec<PermissionOverrideUpdateItem>>>,
        scripted: Option<Vec<PermissionOverrideUpdateResult>>,
    }

    impl Default for MockPermissionUserRepo {
        fn default() -> Self {
            Self {
                list: Vec::new(),
                detail: Vec::new(),
                override_calls: std::sync::Mutex::new(Vec::new()),
                scripted: None,
            }
        }
    }

    #[async_trait::async_trait]
    impl PermissionUserRepository for MockPermissionUserRepo {
        async fn list_permission_users(
            &self,
            _caller_guid: Uuid,
        ) -> Result<Vec<PermissionUserListRow>, RepoError> {

            Ok(self.list.clone())
        }

        async fn find_permission_user_detail(
            &self,
            _user_guid: Uuid,
            _caller_guid: Uuid,
        ) -> Result<Vec<PermissionUserDetailRow>, RepoError> {
            Ok(self.detail.clone())
        }

        async fn update_permission_overrides(
            &self,
            items: &[PermissionOverrideUpdateItem],
            update_by: &str,
        ) -> Result<Vec<PermissionOverrideUpdateResult>, RepoError> {

            self.override_calls.lock().unwrap().push(
                items
                    .iter()
                    .map(|i| i.clone_with_update_by(update_by))
                    .collect(),
            );

            if let Some(scripted) = &self.scripted {

                let mut out = Vec::with_capacity(items.len());
                for i in 0..items.len() {
                    let row = scripted
                        .get(i)
                        .or_else(|| scripted.last())
                        .cloned()
                        .unwrap();
                    out.push(row);
                }
                Ok(out)
            } else {

                Ok(items
                    .iter()
                    .map(|i| PermissionOverrideUpdateResult {
                        success: true,
                        code: PermissionOverrideUpdateResult::CODE_UPDATED.to_string(),
                        message: "ok".to_string(),
                        user_permission_override_guid: Some(format!(
                            "{}-{}",
                            i.user_guid, i.permission_guid
                        )),
                        user_permission_override_user_guid: i.user_guid.clone(),
                        user_permission_override_permission_guid: i.permission_guid.clone(),
                        user_permission_override_effect: i.effect.to_lowercase(),
                        user_permission_override_status: i.status.unwrap_or(1),
                    })
                    .collect())
            }
        }
    }

    fn make_svc(
        list: Vec<PermissionUserListRow>,
        detail: Vec<PermissionUserDetailRow>,
    ) -> PermissionUserService {
        PermissionUserService::new(Arc::new(MockPermissionUserRepo {
            list,
            detail,
            ..Default::default()
        }))
    }

    fn override_item(user: &str, permission: &str, effect: &str) -> PermissionOverrideUpdateItem {
        PermissionOverrideUpdateItem {
            user_guid: user.to_string(),
            permission_guid: permission.to_string(),
            effect: effect.to_string(),
            reason: None,
            assigned_by: None,
            status: None,
        }
    }

    trait CloneWithUpdateBy {
        fn clone_with_update_by(&self, update_by: &str) -> Self;
    }
    impl CloneWithUpdateBy for PermissionOverrideUpdateItem {
        fn clone_with_update_by(&self, _update_by: &str) -> Self {

            Self {
                user_guid: self.user_guid.clone(),
                permission_guid: self.permission_guid.clone(),
                effect: self.effect.clone(),
                reason: self.reason.clone(),
                assigned_by: Some(_update_by.to_string()),
                status: self.status,
            }
        }
    }

    #[tokio::test]
    async fn list_permission_users_returns_empty_page_when_repo_empty() {
        let svc = make_svc(vec![], vec![]);
        let actor = Uuid::new_v4();
        let page = svc.list_permission_users(None, 20, actor).await.unwrap();
        assert!(page.items.is_empty());
        assert!(page.next_cursor.is_none());
    }

    #[tokio::test]
    async fn list_permission_users_applies_cursor_pagination() {
        let rows: Vec<PermissionUserListRow> = ["a@x", "b@x", "c@x", "d@x", "e@x"]
            .iter()
            .map(|e| list_row(e))
            .collect();
        let svc = make_svc(rows, vec![]);
        let actor = Uuid::new_v4();

        let page1 = svc.list_permission_users(None, 2, actor).await.unwrap();
        assert_eq!(page1.items.len(), 2);
        assert_eq!(page1.items[0].email, "a@x");
        assert_eq!(page1.items[1].email, "b@x");
        assert_eq!(page1.next_cursor.as_deref(), Some("b@x"));

        let page2 = svc
            .list_permission_users(page1.next_cursor, 2, actor)
            .await
            .unwrap();
        assert_eq!(page2.items.len(), 2);
        assert_eq!(page2.items[0].email, "c@x");
        assert_eq!(page2.items[1].email, "d@x");
        assert_eq!(page2.next_cursor.as_deref(), Some("d@x"));

        let page3 = svc
            .list_permission_users(page2.next_cursor, 2, actor)
            .await
            .unwrap();
        assert_eq!(page3.items.len(), 1);
        assert_eq!(page3.items[0].email, "e@x");
        assert!(page3.next_cursor.is_none());
    }

    #[tokio::test]
    async fn get_permission_user_group_hoists_identity_to_outer() {
        let user_guid = "11111111-1111-1111-1111-111111111111";
        let rows = vec![
            detail_row(user_guid, "BANNER_CREATE", false, true),
            detail_row(user_guid, "BANNER_DELETE", false, true),
            detail_row(user_guid, "BANNER_UPDATE", false, true),
        ];
        let svc = make_svc(vec![], rows);

        let group = svc
            .get_permission_user_group(Uuid::nil(), Uuid::new_v4())
            .await
            .unwrap();
        assert_eq!(group.user_guid, user_guid);
        assert_eq!(group.full_name, "Test User");
        assert_eq!(group.email, "test@x");
        assert_eq!(group.user_role_name, "Super Admin");
        assert_eq!(group.permissions.len(), 3);
        assert_eq!(group.permissions[0].user_permission_code, "BANNER_CREATE");
        assert!(!group.permissions[0].has_override);
        assert!(group.permissions[0].effective_status);
    }

    #[tokio::test]
    async fn get_permission_user_group_returns_empty_permissions_when_no_rows() {
        let svc = make_svc(vec![], vec![]);
        let group = svc
            .get_permission_user_group(Uuid::nil(), Uuid::new_v4())
            .await
            .unwrap();

        assert!(group.permissions.is_empty());
        assert!(group.user_guid.is_empty());
    }

    #[tokio::test]
    async fn get_permission_user_detail_returns_flat_rows() {
        let user_guid = "11111111-1111-1111-1111-111111111111";
        let rows = vec![
            detail_row(user_guid, "PAGE_DASHBOARD_VIEW", false, true),
            detail_row(user_guid, "INVOICES_EXPORT", false, true),
        ];
        let svc = make_svc(vec![], rows);
        let flat = svc
            .get_permission_user_detail(Uuid::nil(), Uuid::new_v4())
            .await
            .unwrap();
        assert_eq!(flat.len(), 2);
        assert_eq!(flat[0].user_permission_code, "PAGE_DASHBOARD_VIEW");
        assert!(flat[0].effective_status);
    }

    #[test]
    fn group_inner_carries_only_three_fields() {

        let row = detail_row("guid", "X", false, true);
        let entry = PermissionUserGroupEntry {
            user_permission_guid: row.user_permission_guid.clone(),
            user_permission_code: row.user_permission_code.clone(),
            has_override: row.has_override,
            effective_status: row.effective_status,
        };
        let value = serde_json::to_value(&entry).unwrap();
        let obj = value.as_object().unwrap();
        assert_eq!(obj.len(), 4);
        assert!(obj.contains_key("user_permission_guid"));
        assert!(obj.contains_key("user_permission_code"));
        assert!(obj.contains_key("has_override"));
        assert!(obj.contains_key("effective_status"));
        assert!(!obj.contains_key("user_permission_name"));
        assert!(!obj.contains_key("override_effect"));
    }

    #[tokio::test]
    async fn update_permission_overrides_forwards_actor_and_echoes_input() {

        let actor = Uuid::new_v4();
        let mock = MockPermissionUserRepo {
            list: vec![],
            detail: vec![],
            override_calls: std::sync::Mutex::new(Vec::new()),
            scripted: Some(vec![
                PermissionOverrideUpdateResult {
                    success: true,
                    code: PermissionOverrideUpdateResult::CODE_UPDATED.to_string(),
                    message: "ok".to_string(),
                    user_permission_override_guid: Some("ovr-1".to_string()),
                    user_permission_override_user_guid: "u-1".to_string(),
                    user_permission_override_permission_guid: "p-1".to_string(),
                    user_permission_override_effect: "allow".to_string(),
                    user_permission_override_status: 1,
                },
                PermissionOverrideUpdateResult {
                    success: true,
                    code: PermissionOverrideUpdateResult::CODE_CREATED.to_string(),
                    message: "ok".to_string(),
                    user_permission_override_guid: Some("ovr-2".to_string()),
                    user_permission_override_user_guid: "u-1".to_string(),
                    user_permission_override_permission_guid: "p-2".to_string(),
                    user_permission_override_effect: "deny".to_string(),
                    user_permission_override_status: 1,
                },
                PermissionOverrideUpdateResult {
                    success: true,
                    code: PermissionOverrideUpdateResult::CODE_UPDATED.to_string(),
                    message: "ok".to_string(),
                    user_permission_override_guid: Some("ovr-3".to_string()),
                    user_permission_override_user_guid: "u-2".to_string(),
                    user_permission_override_permission_guid: "p-1".to_string(),
                    user_permission_override_effect: "allow".to_string(),
                    user_permission_override_status: 0,
                },
            ]),
        };
        let svc = PermissionUserService::new(Arc::new(mock));
        let items = vec![
            override_item("u-1", "p-1", "allow"),
            override_item("u-1", "p-2", "deny"),
            override_item("u-2", "p-1", "ALLOW"),
        ];
        let results = svc
            .update_permission_overrides(&items, actor)
            .await
            .unwrap();

        assert_eq!(results.len(), 3);
        assert_eq!(results[0].code, "UPDATED");
        assert_eq!(
            results[0].user_permission_override_guid.as_deref(),
            Some("ovr-1")
        );
        assert_eq!(results[1].code, "CREATED");
        assert_eq!(results[2].user_permission_override_status, 0);
    }

    #[tokio::test]
    async fn update_permission_overrides_preserves_order_and_count() {

        let actor = Uuid::new_v4();
        let mock = MockPermissionUserRepo {
            list: vec![],
            detail: vec![],
            override_calls: std::sync::Mutex::new(Vec::new()),
            scripted: None,
        };
        let svc = PermissionUserService::new(Arc::new(mock));
        let items = vec![
            override_item("u-1", "p-1", "allow"),
            override_item("u-1", "p-2", "deny"),
        ];
        let results = svc
            .update_permission_overrides(&items, actor)
            .await
            .unwrap();
        assert_eq!(results.len(), items.len());
        assert_eq!(results[0].user_permission_override_user_guid, "u-1");
        assert_eq!(results[0].user_permission_override_permission_guid, "p-1");
        assert_eq!(results[1].user_permission_override_effect, "deny");
    }

    #[tokio::test]
    async fn update_permission_overrides_propagates_per_item_rejection() {

        let mock = MockPermissionUserRepo {
            list: vec![],
            detail: vec![],
            override_calls: std::sync::Mutex::new(Vec::new()),
            scripted: Some(vec![
                PermissionOverrideUpdateResult {
                    success: true,
                    code: PermissionOverrideUpdateResult::CODE_UPDATED.to_string(),
                    message: "ok".to_string(),
                    user_permission_override_guid: Some("ovr-1".to_string()),
                    user_permission_override_user_guid: "u-1".to_string(),
                    user_permission_override_permission_guid: "p-1".to_string(),
                    user_permission_override_effect: "allow".to_string(),
                    user_permission_override_status: 1,
                },
                PermissionOverrideUpdateResult {
                    success: false,
                    code: PermissionOverrideUpdateResult::CODE_USER_NOT_FOUND.to_string(),
                    message: "user_permission_override_user_guid not found".to_string(),
                    user_permission_override_guid: None,
                    user_permission_override_user_guid: "missing".to_string(),
                    user_permission_override_permission_guid: "p-1".to_string(),
                    user_permission_override_effect: "allow".to_string(),
                    user_permission_override_status: 1,
                },
                PermissionOverrideUpdateResult {
                    success: true,
                    code: PermissionOverrideUpdateResult::CODE_CREATED.to_string(),
                    message: "ok".to_string(),
                    user_permission_override_guid: Some("ovr-3".to_string()),
                    user_permission_override_user_guid: "u-2".to_string(),
                    user_permission_override_permission_guid: "p-1".to_string(),
                    user_permission_override_effect: "deny".to_string(),
                    user_permission_override_status: 1,
                },
            ]),
        };
        let svc = PermissionUserService::new(Arc::new(mock));
        let items = vec![
            override_item("u-1", "p-1", "allow"),
            override_item("missing", "p-1", "allow"),
            override_item("u-2", "p-1", "deny"),
        ];
        let results = svc
            .update_permission_overrides(&items, Uuid::new_v4())
            .await
            .unwrap();
        assert_eq!(results.len(), 3);
        assert!(results[0].is_success());
        assert!(!results[1].is_success());
        assert_eq!(results[1].code, "USER_NOT_FOUND");
        assert!(results[2].is_success());
    }

    #[tokio::test]
    async fn update_permission_overrides_empty_list_returns_empty_results() {

        let mock = MockPermissionUserRepo {
            list: vec![],
            detail: vec![],
            override_calls: std::sync::Mutex::new(Vec::new()),
            scripted: None,
        };
        let svc = PermissionUserService::new(Arc::new(mock));
        let results = svc
            .update_permission_overrides(&[], Uuid::new_v4())
            .await
            .unwrap();
        assert!(results.is_empty());
    }
}
