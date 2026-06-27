//! Permission-page repository port (พอร์ต Permission-page repository — M17).
//!
//! Strictly isolated from [`crate::traits::user::UserRepository`].
//!
//! ## Why a separate trait?
//!
//! The permission page and the admin user-management screen both
//! surface "who has what permission?" — but they are **different
//! flows** with different evolution paths:
//!
//! - **Different route prefixes** (`/api/v1/permission/...` vs
//!   `/api/v1/admin/...`).
//! - **Different application services** (`PermissionUserService` vs
//!   `UserService`).
//! - **Different SPs**: the permission page needs
//!   `SP_PERMISSION_USER_LIST_V2` (single `user_role_name` string,
//!   no CSVs) and `SP_PERMISSION_USER_DETAIL_FIND_BY_GUID`
//!   (takes a GUID directly — no GUID→username translation in Rust).
//!
//! The earlier design reused [`crate::traits::user::UserRepository`]
//! for these calls (`list_with_permissions` +
//! `find_user_permissions_by_username`) plus a `find_by_id` GUID→username
//! translation. That coupled the permission flow to the login/auth flow
//! and to the generic admin user list. Per the M17 task brief,
//! **the permission flow must own its ports**.
//!
//! ## What this trait owns
//!
//! - [`PermissionUserRepository::list_permission_users`] — backs
//!   `GET /api/v1/permission/users` (returns the rich
//!   [`PermissionUserListRow`] shape: `role_codes` / `role_names` /
//!   `has_permission` / `has_override` / status + timestamps).
//! - [`PermissionUserRepository::find_permission_user_detail`] — backs
//!   `GET /api/v1/permission/users/:guid/permissions` and
//!   `GET /api/v1/admin/users/:guid/permissions`.
//! - [`PermissionUserRepository::update_permission_overrides`] — backs
//!   `POST /api/v1/permission/overrides` (batch upsert against
//!   `user_permission_override`).
//!
//! Both read methods take **only** the inputs the SP needs. The trait
//! does not expose GUID→username translation or any other
//! "borrow from another flow" helper — every implementation must
//! satisfy this contract end-to-end with its own SQL.
//!
//! The write method (`update_permission_overrides`) takes a list
//! of [`PermissionOverrideUpdateItem`] — the adapter calls the
//! SP once per item; each call is its own transaction. Per-item
//! failures surface as [`PermissionOverrideUpdateResult`] rows
//! with `success = false`, never as [`RepoError`]. A real DB
//! failure (connection dropped, network blip) **does** surface
//! as [`RepoError::Backend`] and aborts the rest of the batch.
//!
//! ## Dependency rule (AGENTS.md § 6)
//!
//! This module belongs to `domain`. It MUST NOT import
//! `axum`, `tiberius`, `mongodb`, or any other driver crate.

use async_trait::async_trait;
use uuid::Uuid;

use crate::permission::{
    PermissionOverrideUpdateItem, PermissionOverrideUpdateResult, PermissionUserDetailRow,
    PermissionUserListRow,
};
use crate::traits::user::RepoError;

/// Permission-page repository contract (สัญญา Permission-page repository).
///
/// Every persistence concern specific to the permission flow lives
/// behind this trait. Application code depends on the trait; the
/// concrete SQL Server adapter
/// (`kokkak_infra::db::mssql_permission_user::MssqlPermissionUserRepository`)
/// is wired in `api::main`.
///
/// The trait returns the **permission-page DTOs** ([`PermissionUserListRow`],
/// [`PermissionUserDetailRow`]) — not the generic `User` aggregate — so
/// the permission flow can evolve its own wire shape without touching
/// the login / auth flow.
#[async_trait]
pub trait PermissionUserRepository: Send + Sync {
    /// List users for the permission page (cursor-paginated).
    ///
    /// Backed by `dbo.SP_PERMISSION_USER_LIST_V2`. Returns ALL
    /// active users (no parameters); the application layer applies
    /// cursor pagination on top of the result.
    ///
    /// ponytail: the SP returns the full set; pagination lives in
    /// Rust today. Ceiling: extend the SP with `@p_after_username` +
    /// `OFFSET / FETCH NEXT` when the user table grows past ~10K
    /// rows.
    async fn list_permission_users(&self) -> Result<Vec<PermissionUserListRow>, RepoError>;

    /// Per-user detailed permission rows (one per `(user, permission)`
    /// pair) for the **permission page** AND the admin permission-detail
    /// endpoint.
    ///
    /// Backed by `dbo.SP_PERMISSION_USER_DETAIL_FIND_BY_GUID`. The
    /// SP's `@p_user_guid` parameter accepts the user's primary
    /// key directly — no GUID→username translation needed in the
    /// application layer (which was the coupling that M17 removed).
    ///
    /// Returns an empty `Vec` (not `NotFound`) when the user exists
    /// but has no effective permissions — that's a legitimate
    /// empty state the admin / permission UI renders directly.
    /// Returns [`RepoError::NotFound`] when the GUID doesn't resolve
    /// to a user.
    async fn find_permission_user_detail(
        &self,
        user_guid: Uuid,
    ) -> Result<Vec<PermissionUserDetailRow>, RepoError>;

    /// Batch upsert permission overrides — `POST /api/v1/permission/overrides`.
    ///
    /// Backed by `dbo.SP_PERMISSION_USER_OVERRIDE_UPDATE` called
    /// once per item. The SP is its own transaction per call, so
    /// a single per-item failure (validation rejection, missing
    /// user / permission) does **not** abort the rest of the
    /// batch — it lands as a
    /// [`PermissionOverrideUpdateResult`] with `success = false`
    /// at the matching index. A real DB failure (connection
    /// dropped, network blip) **does** surface as
    /// [`RepoError::Backend`] and aborts the loop.
    ///
    /// `update_by` is the auditor's identity — the SP defaults
    /// `user_permission_override_create_by` / `_update_by` to
    /// `'system'` when both are NULL, so the caller is free to
    /// pass an empty string if no actor is known (e.g. a system
    /// job). Empty / whitespace strings are coerced to `NULL` on
    /// the SQL side.
    ///
    /// ponytail: one SP call per item is the simplest possible
    /// implementation. The SP has no TVP variant today, and a
    /// bulk variant (`SP_PERMISSION_USER_OVERRIDE_UPDATE_BATCH`
    /// taking a single JSON / TVP) is the upgrade path when N
    /// grows past the hundreds (each call is a round-trip +
    /// transaction; the latency adds up).
    async fn update_permission_overrides(
        &self,
        items: &[PermissionOverrideUpdateItem],
        update_by: &str,
    ) -> Result<Vec<PermissionOverrideUpdateResult>, RepoError>;
}
