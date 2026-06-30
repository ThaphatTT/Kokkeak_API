//! User use cases (M2 + M14 + M16).
//!
//! - `get_me`: fetch the public view of the current user.
//! - `get_user`: fetch the full `User` aggregate (used by chat /
//!   payment use cases that need the role list / status).
//! - `list_users`: admin-only listing (M16 — backed by
//!   `dbo.SP_PERMISSION_USER_LIST`; cursor pagination applied
//!   in Rust on top of the SP result).
//!
//! **M17 cleanup**: `list_user_permissions` and the
//! `SP_PERMISSION_USER_FIND_BY_USERNAME` plumbing moved to
//! `kokkak_application::permission::PermissionUserService` (backed
//! by the new [`kokkak_domain::PermissionUserRepository`] port).
//! The permission flow no longer lives on the user/auth port.

use std::sync::Arc;

use kokkak_domain::traits::user::RepoError;
use kokkak_domain::{AuthError, PublicUser, UserListRow, UserRepository};
use uuid::Uuid;

/// One page of the admin user listing (M16).
///
/// Mirrors [`crate::order::OrderListPage`] 1:1 so the wire shape
/// is consistent across admin list endpoints.
pub struct UserListPage {
    /// Items on this page ([`UserListRow`], 1:1 with the SP).
    pub items: Vec<UserListRow>,
    /// Opaque cursor for the next page (`None` when this is the
    /// last page). The cursor value is the `email` of the last
    /// item — see [`apply_cursor_pagination`].
    pub next_cursor: Option<String>,
}

/// User use case bundle (M2 + M14 + M16).
pub struct UserService {
    users: Arc<dyn UserRepository>,
}

impl UserService {
    /// Construct the service with a `UserRepository` port.
    pub fn new(users: Arc<dyn UserRepository>) -> Self {
        Self { users }
    }

    /// Fetch the public view of the given user (used by the `GET /users/me` route).
    pub async fn get_me(&self, user_id: Uuid) -> Result<PublicUser, AuthError> {
        let user = self
            .users
            .find_by_id(user_id)
            .await
            .map_err(|e| AuthError::Backend(e.to_string()))?
            .ok_or(AuthError::InvalidCredentials)?;
        Ok(PublicUser::from(&user))
    }

    /// Fetch the full `User` (used by chat + payment use cases
    /// that need the role list / status).
    pub async fn get_user(&self, user_id: Uuid) -> Result<kokkak_domain::User, AuthError> {
        self.users
            .find_by_id(user_id)
            .await
            .map_err(|e| AuthError::Backend(e.to_string()))?
            .ok_or(AuthError::InvalidCredentials)
    }

    /// M16: list users for the admin console.
    ///
    /// Backed by [`UserRepository::list_with_permissions`]
    /// (`dbo.SP_PERMISSION_USER_LIST`). The SP returns the full
    /// set of active users; the application layer applies
    /// cursor pagination (`after` + `limit`) on top:
    ///
    /// - `after` is the `email` (login handle) of the last item
    ///   on the previous page — the SP sorts by `user_username_username`
    ///   so a "first row whose email > after" scan is correct.
    /// - `limit` caps the slice size (handler clamps to 1..=100).
    ///
    /// `next_cursor` is the email of the last item on this page
    /// when more rows remain; `None` otherwise.
    ///
    /// ponytail: the SP returns the full set; pagination lives
    /// in Rust today. Ceiling: extend the SP with `@p_after_username`
    /// plus `OFFSET` / `FETCH NEXT` once the user table grows past
    /// the ten-thousand-row range. At that point the O(n) Rust-side
    /// scan plus transport becomes the bottleneck.
    ///
    /// M19: `actor` is the authenticated admin's GUID forwarded as
    /// `@p_user_guid` to `dbo.SP_PERMISSION_USER_LIST` for the
    /// SP-level admin gate (defense-in-depth).
    pub async fn list_users(
        &self,
        after: Option<String>,
        limit: u32,
        actor: Uuid,
    ) -> Result<UserListPage, RepoError> {
        let rows = self.users.list_with_permissions(actor).await?;
        Ok(apply_cursor_pagination(rows, after.as_deref(), limit))
    }
}

/// Apply cursor pagination to the full SP result.
///
/// `after` is the email of the last item on the previous page;
/// `limit` is the max number of rows on this page. The SP sorts
/// by `user_username_username` (alias `email`) so a `>` scan is
/// correct.
///
/// Returns a page with `next_cursor = Some(last_email)` when more
/// rows remain; `None` otherwise.
fn apply_cursor_pagination(
    rows: Vec<UserListRow>,
    after: Option<&str>,
    limit: u32,
) -> UserListPage {
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
    UserListPage { items, next_cursor }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// In-memory mock of UserRepository for unit tests.
    /// Stores users in a HashMap; collision on username returns Conflict.
    #[derive(Default)]
    struct MockUserRepository {
        by_id: std::sync::Mutex<std::collections::HashMap<uuid::Uuid, User>>,
        by_username: std::sync::Mutex<std::collections::HashMap<String, Uuid>>,
        /// Pre-loaded rows for [`UserRepository::list_with_permissions`].
        /// When empty, returns an empty Vec (the default for a
        /// brand-new mock).
        list_rows: std::sync::Mutex<Vec<kokkak_domain::UserListRow>>,
    }

    #[async_trait::async_trait]
    impl UserRepository for MockUserRepository {
        async fn find_by_id(&self, id: Uuid) -> Result<Option<User>, kokkak_domain::RepoError> {
            Ok(self.by_id.lock().unwrap().get(&id).cloned())
        }
        async fn find_by_username(
            &self,
            username: &str,
        ) -> Result<Option<User>, kokkak_domain::RepoError> {
            let key = username.trim().to_lowercase();
            let by_un = self.by_username.lock().unwrap();
            let by_id = self.by_id.lock().unwrap();
            Ok(by_un.get(&key).and_then(|id| by_id.get(id).cloned()))
        }
        async fn insert(&self, user: &User) -> Result<(), kokkak_domain::RepoError> {
            let key = user.username.trim().to_lowercase();
            let mut by_un = self.by_username.lock().unwrap();
            if by_un.contains_key(&key) {
                return Err(kokkak_domain::RepoError::Conflict(format!(
                    "username {} taken",
                    user.username
                )));
            }
            by_un.insert(key, user.id);
            self.by_id.lock().unwrap().insert(user.id, user.clone());
            Ok(())
        }
        async fn update(&self, user: &User) -> Result<(), kokkak_domain::RepoError> {
            let mut by_id = self.by_id.lock().unwrap();
            if !by_id.contains_key(&user.id) {
                return Err(kokkak_domain::RepoError::NotFound(format!(
                    "user {} not found",
                    user.id
                )));
            }
            by_id.insert(user.id, user.clone());
            Ok(())
        }
        async fn list_with_permissions(
            &self,
            _caller_guid: Uuid,
        ) -> Result<Vec<kokkak_domain::UserListRow>, kokkak_domain::RepoError> {
            // M19: caller_guid accepted but ignored by the mock —
            // the SP-side admin gate is exercised by the integration
            // suite; the unit tests here verify pagination only.
            Ok(self.list_rows.lock().unwrap().clone())
        }
        async fn find_username_guid_by_user_guid(
            &self,
            _user_guid: Uuid,
        ) -> Result<Option<String>, kokkak_domain::RepoError> {
            Ok(None)
        }
        async fn admin_insert_full(
            &self,
            _req: &kokkak_domain::AdminInsertUserRequest,
        ) -> Result<kokkak_domain::AdminInsertUserResult, kokkak_domain::AdminInsertUserError>
        {
            Err(kokkak_domain::AdminInsertUserError::new(
                "internal",
                "admin_insert_full not implemented in user mock",
            ))
        }
    }

    use chrono::Utc;
    use kokkak_domain::{Role, User, UserStatus};
    // skip MssqlUserRepository;

    #[tokio::test]
    async fn get_me_returns_public_view() {
        let repo: Arc<dyn UserRepository> = Arc::new(MockUserRepository::default());
        let id = Uuid::new_v4();
        let u = User {
            id,
            first_name: "A".into(),
            last_name: "B".into(),
            username: "ab".into(),
            password_hash: "$argon2".into(),
            roles: vec![Role::Customer],
            permissions: Vec::new(),
            status: UserStatus::Active,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        repo.insert(&u).await.unwrap();
        let svc = UserService::new(repo);
        let me = svc.get_me(id).await.unwrap();
        assert_eq!(me.username, "ab");
        assert_eq!(me.first_name, "A");
        assert_eq!(me.last_name, "B");
    }

    #[tokio::test]
    async fn get_me_unknown_user_fails() {
        let repo: Arc<dyn UserRepository> = Arc::new(MockUserRepository::default());
        let svc = UserService::new(repo);
        let err = svc.get_me(Uuid::new_v4()).await.unwrap_err();
        assert!(matches!(err, AuthError::InvalidCredentials));
    }

    /// Helper: build a sample UserListRow for pagination tests.
    ///
    /// M17 cleanup: `permission_codes` was already removed in M16
    /// round 2 (LIST = summary only). Detail codes now come from the
    /// M17 [`kokkak_domain::PermissionUserDetailRow`] via
    /// [`kokkak_application::permission::PermissionUserService`].
    fn sample_list_row(email: &str) -> kokkak_domain::UserListRow {
        kokkak_domain::UserListRow {
            user_guid: Uuid::new_v4().to_string(),
            full_name: format!("Test {email}"),
            email: email.to_string(),
            role_codes: vec!["customer".into()],
            role_names: vec!["Customer".into()],
            has_permission: true,
            has_override: false,
            user_status: UserStatus::Active,
            user_username_status: 1,
        }
    }

    /// M16: empty list → empty page, no cursor.
    #[tokio::test]
    async fn list_users_returns_empty_page_when_repo_empty() {
        let repo: Arc<dyn UserRepository> = Arc::new(MockUserRepository::default());
        let svc = UserService::new(repo);
        let actor = Uuid::new_v4();
        let page = svc.list_users(None, 25, actor).await.unwrap();
        assert!(page.items.is_empty());
        assert!(page.next_cursor.is_none());
    }

    /// M16: cursor pagination slices by `after` (email > cursor)
    /// and caps by `limit`. The cursor is the email of the LAST
    /// item on the previous page.
    #[tokio::test]
    async fn list_users_applies_cursor_pagination() {
        let mock = MockUserRepository {
            list_rows: std::sync::Mutex::new(vec![
                sample_list_row("alice@example.com"),
                sample_list_row("bob@example.com"),
                sample_list_row("carol@example.com"),
            ]),
            ..Default::default()
        };
        let repo: Arc<dyn UserRepository> = Arc::new(mock);
        let svc = UserService::new(repo);
        let actor = Uuid::new_v4();

        // First page: 2 rows, cursor = bob (last email).
        let page1 = svc.list_users(None, 2, actor).await.unwrap();
        assert_eq!(page1.items.len(), 2);
        assert_eq!(page1.items[0].email, "alice@example.com");
        assert_eq!(page1.items[1].email, "bob@example.com");
        assert_eq!(page1.next_cursor.as_deref(), Some("bob@example.com"));

        // Second page: starts after "bob", returns carol.
        let page2 = svc
            .list_users(page1.next_cursor.clone(), 2, actor)
            .await
            .unwrap();
        assert_eq!(page2.items.len(), 1);
        assert_eq!(page2.items[0].email, "carol@example.com");
        assert!(page2.next_cursor.is_none());
    }

    // M17 cleanup: the three `list_user_permissions_*` tests moved
    // to `crates/application/src/permission.rs` alongside the new
    // `PermissionUserService::get_permission_user_group` test set.
    // The mock `UserRepository` no longer carries `perm_rows`; the
    // permission flow's mock lives next to the service that uses it.
}
