//! User repository port (พอร์ต User repository — M2 + M14 + M22).
//!
//! Returns `None` when the entity is missing; returns `Err(...)` when
//! the operation could not be performed (DB down, constraint
//! violation, etc.). `RepoError` is the **only** error type a
//! repository raises — adapters translate driver-specific errors
//! into the variants below.
//!
//! M14 renamed the login lookup from `find_by_email` → `find_by_username`
//! because NEW_DB `[user]` has no email column; login is now
//! `[user_username].user_username_username` (a free-form login id —
//! email, phone, or alphanumeric handle).
//!
//! M22 added [`UserRepository::get_user_detail_full`] — read-side
//! counterpart to [`UserRepository::admin_insert_full`].

use async_trait::async_trait;
use thiserror::Error;
use uuid::Uuid;

use crate::admin_user::{
    AdminInsertUserError, AdminInsertUserRequest, AdminInsertUserResult, AdminUpdateUserError,
    AdminUpdateUserRequest, AdminUpdateUserResult, AdminUserDetail, AdminUserListPagingInput,
    AdminUserListPagingPage,
};
use crate::user::{User, UserListRow};

/// Repository-level error (one of the few `domain` types that maps
/// 1:1 to an HTTP status).
#[derive(Debug, Error)]
pub enum RepoError {
    /// 404 — entity not found.
    #[error("not found: {0}")]
    NotFound(String),

    /// 409 — unique-key violation (e.g. duplicate username).
    #[error("conflict: {0}")]
    Conflict(String),

    /// 500 — backend (DB / network) failure.
    #[error("backend error: {0}")]
    Backend(String),
}

/// User repository contract (สัญญา User repository).
#[async_trait]
pub trait UserRepository: Send + Sync {
    /// Find a user by primary key (`[user].user_guid`).
    async fn find_by_id(&self, id: Uuid) -> Result<Option<User>, RepoError>;

    /// Find a user by lowercased login username
    /// (`[user_username].user_username_username`). This is the
    /// canonical login lookup.
    async fn find_by_username(&self, username: &str) -> Result<Option<User>, RepoError>;

    /// Persist a brand-new user. Returns `Conflict` when the
    /// username is already taken.
    ///
    /// Implementations are responsible for inserting into all four
    /// NEW_DB tables atomically (`[user]` + `[user_username]` +
    /// `[user_user_role]` + the role-lookup against `[user_role]`).
    /// Adapters that cannot guarantee atomicity (e.g. JSON-DB sim)
    /// must document that deviation.
    async fn insert(&self, user: &User) -> Result<(), RepoError>;

    /// Replace an existing user. Returns `NotFound` if the id does
    /// not exist. Currently updates `[user]` + `[user_username]` only;
    /// role changes go through a dedicated admin endpoint (planned
    /// M15+).
    async fn update(&self, user: &User) -> Result<(), RepoError>;

    /// M16: fetch the admin user-list view (one row per user) with
    /// permission summary CSVs.
    ///
    /// Backed by `dbo.SP_PERMISSION_USER_LIST`. Returns ALL active
    /// users; the application layer applies cursor pagination
    /// (`after` + `limit`) on top of the result.
    ///
    /// ponytail: the SP returns the full set; pagination lives in
    /// Rust today. Ceiling: extend the SP with `@p_after_username`
    /// + `OFFSET`/`FETCH` when the admin list grows past ~10K
    /// users — at that point the O(n) Rust-side slice + `O(n)`
    /// transport becomes the bottleneck.
    ///
    /// M19: `caller_guid` is the authenticated admin's GUID. The
    /// SP enforces an admin / super_admin check before returning
    /// rows; a non-admin caller receives zero rows per the
    /// fail-closed read contract.
    async fn list_with_permissions(&self, caller_guid: Uuid)
        -> Result<Vec<UserListRow>, RepoError>;

    // M17 cleanup: the per-user detail row type + its SP
    // (`SP_PERMISSION_USER_FIND_BY_USERNAME`) moved to
    // [`kokkak_domain::PermissionUserRepository`] (see
    // `crates/domain/src/traits/permission.rs`). The permission
    // flow no longer lives on the login/auth port — it owns its
    // own port, its own SPs, and its own DTOs.

    /// M20-b: lookup the `user_username_guid` for a given
    /// `user_guid`. Returns `None` when the user has no active
    /// `[user_username]` row (suspended / deleted accounts do
    /// not surface here because the SP filters `status <> 3`).
    ///
    /// Backed by a simple `SELECT user_username_guid ... WHERE
    /// user_username_user_guid = @P1 AND user_username_status = 1`.
    /// Used by `admin_insert_full` to resolve the JWT's
    /// `user_guid` → the SP's required `user_username_guid`.
    async fn find_username_guid_by_user_guid(
        &self,
        user_guid: uuid::Uuid,
    ) -> Result<Option<String>, RepoError>;

    /// M20-b: rich admin user creation — wraps
    /// `dbo.SP_USER_INSERT_FULL`. The actor is identified by
    /// `user_username_guid` (resolved upstream via
    /// [`UserRepository::find_username_guid_by_user_guid`]). The
    /// SP re-checks ADMIN server-side as defense-in-depth — the
    /// Rust handler still gates on `admin_flag`.
    ///
    /// The password MUST arrive at the SP pre-hashed (argon2id
    /// PHC string); the service layer is responsible for
    /// hashing. Plaintext is never sent over the wire to SQL
    /// Server (AGENTS.md § 12.1).
    ///
    /// On SP failure, returns the structured
    /// [`AdminInsertUserError`] (the SP's `code` + `message`
    /// verbatim) so the handler can map to the right HTTP
    /// status + `error.code` for the admin UI.
    async fn admin_insert_full(
        &self,
        req: &AdminInsertUserRequest,
    ) -> Result<AdminInsertUserResult, AdminInsertUserError>;

    /// Admin user listing with page-based pagination.
    ///
    /// Backed by `dbo.SP_USER_LIST_PAGING` — one row per user with
    /// status label / phone / role names / current position name.
    /// Filters on `keyword` (free-form across name + phone + email)
    /// and `user_status` (raw int, `None` = no filter).
    ///
    /// `actor` is forwarded for audit logging consistency (the SP
    /// itself doesn't enforce admin gating — that lives at the
    /// handler layer via [`Permission::PageUsersView`]).
    ///
    /// ponytail: the SP uses `OFFSET` / `FETCH NEXT` (offset pagination)
    /// because that's the legacy contract. Ceiling: project rule §11.4
    /// prefers keyset / cursor at deep pages — when the user table
    /// grows past ~10K rows, extend the SP with `@p_after_user_guid`
    /// + `WHERE user_guid > @P...` and switch the wire contract to
    /// cursor. For the current admin user volume (low-thousands) the
    /// SP's `OFFSET` is fine.
    ///
    /// Default impl returns `Backend("not implemented")` so the
    /// existing test mocks don't break — only the MSSQL adapter needs
    /// to override it. When the next mock is added, copy the
    /// default-impl line.
    async fn list_users_paging(
        &self,
        input: &AdminUserListPagingInput,
        actor: Uuid,
    ) -> Result<AdminUserListPagingPage, RepoError> {
        let _ = (input, actor);
        Err(RepoError::Backend(
            "list_users_paging: not implemented by this repository adapter".into(),
        ))
    }

    /// M22: full detail lookup for a single user — wraps
    /// `dbo.SP_USER_DETAIL_FULL_GET`.
    ///
    /// Returns `Ok(None)` when the SP emits zero rows (i.e. the
    /// `user_guid` doesn't resolve to a non-deleted `[user]` row).
    /// Returns `Ok(Some(detail))` on a successful read; the
    /// `AdminUserDetail` sub-blocks are individually `Option<_>`
    /// so a user without (e.g.) a bank account still serialises
    /// cleanly.
    ///
    /// `actor` is forwarded for audit log consistency; the SP
    /// itself does NOT enforce admin gating — that lives at the
    /// handler layer via [`Permission::PageUsersView`].
    ///
    /// Default impl returns `Backend("not implemented")` so the
    /// existing test mocks don't break — only the MSSQL adapter needs
    /// to override it.
    async fn get_user_detail_full(
        &self,
        user_guid: Uuid,
        actor: Uuid,
    ) -> Result<Option<AdminUserDetail>, RepoError> {
        let _ = (user_guid, actor);
        Err(RepoError::Backend(
            "get_user_detail_full: not implemented by this repository adapter".into(),
        ))
    }

    /// M22-b: rich admin user update — wraps `dbo.SP_USER_UPDATE_FULL`.
    ///
    /// Write-side counterpart to
    /// [`UserRepository::admin_insert_full`]. Updates the
    /// matching `[user]` row + the linked `[user_username]` row
    /// in one transaction. The actor is identified by
    /// `user_username_guid` (resolved upstream via
    /// [`UserRepository::find_username_guid_by_user_guid`]) and
    /// the SP re-checks `USERS_UPDATE` server-side as
    /// defense-in-depth — the Rust handler gates on
    /// [`Permission::UsersUpdate`].
    ///
    /// The SP does NOT touch the password column — password
    /// reset lives on a separate flow.
    ///
    /// On SP failure, returns the structured
    /// [`AdminUpdateUserError`] (the SP's `code` + `message`
    /// verbatim) so the handler can map to the right HTTP
    /// status + `error.code` for the admin UI.
    ///
    /// Default impl returns `Backend("not implemented")` so the
    /// existing test mocks don't break — only the MSSQL adapter
    /// needs to override it.
    async fn admin_update_full(
        &self,
        req: &AdminUpdateUserRequest,
    ) -> Result<AdminUpdateUserResult, AdminUpdateUserError> {
        let _ = req;
        Err(AdminUpdateUserError::new(
            "internal",
            "admin_update_full: not implemented by this repository adapter",
        ))
    }
}
