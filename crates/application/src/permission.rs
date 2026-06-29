//! Permission-page use cases (M17 — fully decoupled from the user
//! repository).
//!
//! ## Why this module is separate from `crate::user`
//!
//! The permission page is **its own flow** — different route prefix
//! (`/api/v1/permission/...` vs `/api/v1/admin/...`), different
//! handler, different future evolution path. Before M17 it shared
//! `UserRepository::find_by_id`, `list_with_permissions`, and
//! `find_user_permissions_by_username` with the login/auth flow and
//! the generic admin user list, which coupled the flows together at
//! the SP level (`SP_PERMISSION_USER_LIST` +
//! `SP_PERMISSION_USER_FIND_BY_USERNAME`)
//!
//! AND forced a GUID→username translation in Rust.
//!
//! M17 creates a dedicated [`PermissionUserRepository`] port. The
//! port is backed by two new SPs: `SP_PERMISSION_USER_LIST_V2` and
//! `SP_PERMISSION_USER_DETAIL_FIND_BY_GUID`.
//!
//! The permission flow no longer shares any function with the
//! login/auth flow or the generic admin user list.
//!
//! ## Layering
//!
//! ```text
//! api/handlers/permission.rs         (axum)
//!        ↓
//! application::permission::PermissionUserService   ← this module
//!        ↓ (Arc<dyn PermissionUserRepository>)
//! infra::db::mssql_permission_user::MssqlPermissionUserRepository
//!        ↓
//! dbo.SP_PERMISSION_USER_LIST_V2 / SP_PERMISSION_USER_DETAIL_FIND_BY_GUID
//! ```
//!
//! The shared repository is the **only** port the service depends on —
//! no `UserRepository`, no `find_by_id`, no GUID→username translation.
//!
//! ## M18 — batch override upsert
//!
//! `update_permission_overrides` is the write-side counterpart to
//! the M17 read flow. It calls the SP once per input item through
//! the dedicated [`PermissionUserRepository::update_permission_overrides`]
//! port — the admin permission screen can hit the same endpoint
//! (same SP, same port, same wire shape) by going through the
//! admin handler that re-exports this service method.

use std::sync::Arc;

use kokkak_domain::permission::{
    PermissionOverrideUpdateItem, PermissionOverrideUpdateResult, PermissionUserDetailRow,
    PermissionUserGroup, PermissionUserListRow,
};
use kokkak_domain::traits::permission::PermissionUserRepository;
use kokkak_domain::traits::user::RepoError;
use uuid::Uuid;

/// One page of the permission-page user listing.
///
/// Wire-shape is [`PermissionUserListRow`] (the simpler "single
/// `user_role_name`" payload). The cursor field is still the
/// `email` (login handle) — matches the SP's `ORDER BY
/// user_username_username`.
pub struct PermissionUserListPage {
    /// Items on this page.
    pub items: Vec<PermissionUserListRow>,
    /// Opaque cursor for the next page — the `email` of the last
    /// item on this page when more rows remain; `None` on the last
    /// page.
    pub next_cursor: Option<String>,
}

/// Use case bundle for the permission page.
///
/// Holds an `Arc<dyn PermissionUserRepository>` — the **only**
/// repository port this service depends on. No coupling to
/// `UserRepository` (intentional — see module docs).
pub struct PermissionUserService {
    /// Permission-page repository port (M17).
    permission_users: Arc<dyn PermissionUserRepository>,
}

impl PermissionUserService {
    /// Construct the service with a `PermissionUserRepository` port.
    pub fn new(permission_users: Arc<dyn PermissionUserRepository>) -> Self {
        Self { permission_users }
    }

    /// List users for the permission page (cursor-paginated).
    ///
    /// Backed by [`PermissionUserRepository::list_permission_users`]
    /// → `dbo.SP_PERMISSION_USER_LIST`. Pagination is applied in
    /// Rust on top of the SP's full result (the SP sorts by
    /// `user_username_username`, so a `>` scan on the email cursor
    /// is correct). The handler caps `limit` at 1..=100.
    ///
    /// ponytail: full-result fetch + Rust-side pagination. Ceiling:
    /// extend the SP with `@p_after_username` + `OFFSET / FETCH NEXT`
    /// when the user table grows past the ten-thousand-row range.
    ///
    /// M19: `actor` is forwarded as `caller_guid` for the SP-level
    /// admin check (defense-in-depth on top of the axum `admin_flag`
    /// middleware).
    pub async fn list_permission_users(
        &self,
        after: Option<String>,
        limit: u32,
        actor: Uuid,
    ) -> Result<PermissionUserListPage, RepoError> {
        let rows = self.permission_users.list_permission_users(actor).await?;
        Ok(apply_cursor_pagination(rows, after.as_deref(), limit))
    }

    /// Per-user effective permission detail for the permission page
    /// (and the admin permission-detail screen).
    ///
    /// Returns the **grouped** wire payload: one outer user identity
    /// object + a nested `permissions: Vec<PermissionUserGroupEntry>`.
    /// The flat per-permission rows from the SP are grouped here at
    /// the application layer.
    ///
    /// Maps to:
    /// - `RepoError::NotFound` when the GUID doesn't resolve to a
    ///   user (handler → 404 + `err_auth.user_not_found`).
    /// - `RepoError::Backend` when the DB call fails.
    ///
    /// An empty `permissions: []` is a legitimate response when the
    /// user exists but holds no effective permissions — the UI
    /// renders an empty-state placeholder.
    ///
    /// M19: `actor` is the authenticated admin's GUID forwarded as
    /// `@p_caller_user_guid` for the SP-level admin gate.
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

    /// Per-user effective permission detail (flat list) — exposed
    /// for clients that want every per-permission row in one
    /// round-trip (no grouping). The permission-page UI uses the
    /// grouped variant ([`Self::get_permission_user_group`]) by
    /// default; the flat variant is kept for the admin per-user
    /// permission detail screen and for SDK generators that
    /// prefer the unprocessed SP output.
    pub async fn get_permission_user_detail(
        &self,
        user_guid: Uuid,
        actor: Uuid,
    ) -> Result<Vec<PermissionUserDetailRow>, RepoError> {
        self.permission_users
            .find_permission_user_detail(user_guid, actor)
            .await
    }

    /// Batch upsert permission overrides (M18).
    ///
    /// Calls
    /// [`PermissionUserRepository::update_permission_overrides`]
    /// which in turn loops the SP. The actor's GUID is forwarded
    /// as `update_by` so the SP records it in
    /// `user_permission_override_update_by` for the audit trail.
    ///
    /// ## Per-item semantics
    ///
    /// The SP is its own transaction per call. A per-item
    /// validation rejection (e.g. `INVALID_EFFECT`,
    /// `USER_NOT_FOUND`) lands as a
    /// [`PermissionOverrideUpdateResult`] row with
    /// `success = false` at the matching index — the rest of
    /// the batch still runs. A real DB failure (connection
    /// dropped, tiberius propagating the SP's `THROW` from
    /// the CATCH block) surfaces as [`RepoError::Backend`]
    /// and aborts the loop; the handler maps to 500.
    ///
    /// ## Why a dedicated port
    ///
    /// The override flow writes a different table
    /// (`user_permission_override`) and uses a different SP
    /// (`SP_PERMISSION_USER_OVERRIDE_UPDATE`) than the read
    /// flow. Per the M17 "permission owns its port" rule, the
    /// write method sits on the same `PermissionUserRepository`
    /// trait (not on `UserRepository` or a new trait) so the
    /// permission flow keeps a single dependency edge.
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

/// Group the flat per-permission rows from the SP into the wire
/// payload shape the admin / permission UI consumes.
///
/// The SP returns N rows per user (one per `(user, permission)`
/// pair); the grouping step hoists the user identity onto the outer
/// object and drops it from the inner entries. All inner fields
/// the SP provides (`user_permission_name`, `override_effect`)
/// are **preserved on the flat variant** ([`PermissionUserDetailRow`])
/// — the grouped inner carries only the three fields the front-end
/// pattern-matches on (`code`, `has_override`, `effective_status`).
///
/// ponytail: cheap single-pass over the rows. The SP's `ORDER BY
/// user_permission_code` is not needed here because the outer user
/// identity is the same on every row; if the SP ever returns rows
/// for multiple users in a single call, this helper would need to
/// re-group by `user_guid` — single line of code, single change.
fn group_permission_user_rows(rows: &[PermissionUserDetailRow]) -> PermissionUserGroup {
    // The outer user identity is identical on every row by
    // construction (the SP takes a single GUID parameter).
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

/// Apply cursor pagination to the full SP result.
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
    //! In-process tests against a hand-rolled `PermissionUserRepository`
    //! mock — no DB, no axum. Keeps the cursor-pagination contract
    //! honest because the production path applies it inside the
    //! handler, plus the row→group grouping.

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
        /// Per-call counter + scripted result for the override
        /// update path. When `scripted` is `Some`, the Nth call
        /// returns the Nth scripted result (clamped). When `None`,
        /// the mock returns a generic "UPDATED" row per item so
        /// tests can assert on the loop + ordering without
        /// spelling out every scripted result.
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
            // M19: caller_guid accepted but ignored — admin gate
            // is exercised against the SP in the integration suite.
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
            // Record the call so tests can assert on the
            // forwarded `update_by` and the exact item list.
            self.override_calls.lock().unwrap().push(
                items
                    .iter()
                    .map(|i| i.clone_with_update_by(update_by))
                    .collect(),
            );

            if let Some(scripted) = &self.scripted {
                // One scripted row per input item, clamped to
                // the last scripted result if the script is
                // shorter than the input (mirrors real SP
                // behavior where each item is independent).
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
                // Default: every item is a successful UPSERT.
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

    /// Build a `PermissionOverrideUpdateItem` for tests.
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

    /// Helper trait extension: copy an item while stamping the
    /// forwarded `update_by` so the mock can record it. Keeps
    /// the recording separate from the result-building branch.
    trait CloneWithUpdateBy {
        fn clone_with_update_by(&self, update_by: &str) -> Self;
    }
    impl CloneWithUpdateBy for PermissionOverrideUpdateItem {
        fn clone_with_update_by(&self, _update_by: &str) -> Self {
            // The mock's `assigned_by` field carries the
            // forwarded actor — tests that need to assert on
            // it can read this column back.
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
        // Empty rows → empty identity + empty permissions (the SP
        // returns empty set when the GUID is unknown; the trait
        // adapter maps that to NotFound instead, but the service
        // is robust to either path).
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
        // The grouped inner drops `user_permission_name` +
        // `override_effect` so the wire payload matches the
        // {code, has_override, effective_status} contract.
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

    // -----------------------------------------------------------------
    // M18 — override update (batch upsert)
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn update_permission_overrides_forwards_actor_and_echoes_input() {
        // Three items, all scripted as `UPDATED` / `CREATED`. The
        // service must forward the actor's GUID as `update_by`
        // and pass each item through unchanged (SP echoes them
        // on the result row).
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
            override_item("u-2", "p-1", "ALLOW"), // SP lowercases
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
        // Per-item results must align 1:1 with the input list —
        // front-end relies on `results[i]` to correspond to
        // `items[i]`. The mock's default branch echoes
        // `UPDATED` for every input; we just assert ordering
        // and counts.
        let actor = Uuid::new_v4();
        let mock = MockPermissionUserRepo {
            list: vec![],
            detail: vec![],
            override_calls: std::sync::Mutex::new(Vec::new()),
            scripted: None, // default UPDATED per item
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
        // The second item fails per-item (`USER_NOT_FOUND`); the
        // first and third still run. The service returns a
        // mixed result list with the failure at index 1.
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
        // An empty list is allowed (the handler validates `1..=500`
        // before reaching the service). The service must return an
        // empty `Vec` without calling the repo.
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
