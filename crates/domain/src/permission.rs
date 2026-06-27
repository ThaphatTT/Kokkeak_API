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

use chrono::{DateTime, Utc};
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

/// One row of the result returned by `dbo.SP_USER_ROLE_PERMISSION_UPDATE`.
///
/// The SP is called once per `(user_role_guid, user_permission_guid,
/// user_role_permission_status)` triple and emits **either** one row
/// (success / domain-level rejection) **or** zero rows (silent no-op
/// when the caller asked to revoke a pair that wasn't granted in the
/// first place — `status = 0` + no existing junction row). The
/// repository layer fills in a synthetic [`PermissionUpdateRow`] with
/// `code = "NO_CHANGE"` when the SP returns zero rows so the wire
/// payload stays uniform (one entry per input item).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct PermissionUpdateRow {
    /// `success` bit the SP returns — `true` for `UPDATED` /
    /// `INSERTED`, `false` for the per-item error codes below.
    pub success: bool,
    /// Stable machine code — one of:
    ///
    /// - `"UPDATED"` — junction row existed, status flipped.
    /// - `"CREATED"` — junction row was missing, a new one was
    ///   inserted (the SP literal is `'CREATED'`, not `'INSERTED'`).
    /// - `"NO_CHANGE"` — synthetic; `status = 0` and the junction
    ///   row didn't exist (the SP returned zero rows; the repo
    ///   fills this in).
    /// - `"ROLE_NOT_FOUND"` — `user_role_guid` doesn't resolve.
    /// - `"PERMISSION_NOT_FOUND"` — `user_permission_guid` doesn't resolve.
    /// - `"INVALID_STATUS"` — defensive; the API layer pre-validates
    ///   `status ∈ {0, 1}` so this should never come back from the SP.
    pub code: String,
    /// Human-readable English message from the SP (or the synthetic
    /// `"no change"` for the no-op case). The wire payload keeps
    /// this in English — admin-only debug surface, not localized
    /// through i18n (which is reserved for user-facing responses).
    pub message: String,
    /// `user_role_permission_guid` — populated on `UPDATED` /
    /// `INSERTED`, otherwise `None`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_role_permission_guid: Option<String>,
    /// Echo of the input `user_role_guid` for caller convenience.
    pub user_role_guid: String,
    /// Echo of the input `user_permission_guid` for caller convenience.
    pub user_permission_guid: String,
    /// Echo of the input `status` for caller convenience.
    /// `0` for `NO_CHANGE` / revoked pairs, `1` for newly granted,
    /// `0` / `1` for `UPDATED` (whatever the caller asked for).
    pub user_role_permission_status: i32,
}

impl PermissionUpdateRow {
    /// One of the SP's success codes (junction row was mutated).
    pub const CODE_UPDATED: &'static str = "UPDATED";
    /// One of the SP's success codes (junction row was created).
    ///
    /// The SP literal is `'CREATED'` — see
    /// `migrations/20260620000007_sp_user_role.sql` →
    /// `SP_USER_ROLE_PERMISSION_UPDATE` CREATE branch. Any code that
    /// checks this constant against an SP-emitted value MUST use this
    /// exact string.
    pub const CODE_CREATED: &'static str = "CREATED";
    /// Legacy alias kept for callers (e.g. older mocks) that still
    /// speak the pre-fix contract. The SP itself only emits
    /// [`CODE_CREATED`].
    pub const CODE_INSERTED: &'static str = "CREATED";
    /// Synthetic code the Rust layer emits when the SP returned
    /// zero rows (see [`PermissionUpdateRow`] doc).
    pub const CODE_NO_CHANGE: &'static str = "NO_CHANGE";
    /// SP rejected the input — `user_role_guid` doesn't exist.
    pub const CODE_ROLE_NOT_FOUND: &'static str = "ROLE_NOT_FOUND";
    /// SP rejected the input — `user_permission_guid` doesn't exist.
    pub const CODE_PERMISSION_NOT_FOUND: &'static str = "PERMISSION_NOT_FOUND";
    /// Defensive — the API layer validates `status` upfront; the SP
    /// would reject anything else.
    pub const CODE_INVALID_STATUS: &'static str = "INVALID_STATUS";

    /// Build the synthetic no-op row the repo emits when the SP
    /// returned zero rows (status = 0 + no existing junction row).
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

    /// `true` for the success codes (`UPDATED`, `CREATED`, `NO_CHANGE`).
    pub fn is_success(&self) -> bool {
        self.success
            && (self.code == Self::CODE_UPDATED
                || self.code == Self::CODE_CREATED
                || self.code == Self::CODE_NO_CHANGE)
    }
}

// ============================================================================
// Permission-page user DTOs (M17 — decoupled from `domain::user`).
// ============================================================================
//
// These types are the wire shape of `GET /api/v1/permission/users` and
// `GET /api/v1/permission/users/:guid/permissions` (plus the admin
// counterpart `GET /api/v1/admin/users/:guid/permissions`). They live in
// this module — **not** in `domain::user` — because:
// 1. The permission page is its own flow (different route prefix, different
//    future evolution path; see `application::permission` module docs).
// 2. The user table no longer leaks its permission-shape contract: `User`
//    is a login/identity aggregate; permission views are derived data
//    served by their own SPs.
//
// Field names match `SP_PERMISSION_USER_LIST_V2` /
// `SP_PERMISSION_USER_DETAIL_FIND_BY_GUID` column names 1:1 — the
// infra mapper is a thin tiberius row → struct.

/// One row of the **permission-page user listing**.
///
/// Backed by `dbo.SP_PERMISSION_USER_LIST_V2`. One row per user.
/// The mapper (`row_to_permission_user_list_row`) reads the SP columns
/// 1:1 — `role_codes` / `role_names` stay as `String` (single role per
/// row today, no CSV split at this layer; if a future SP change makes
/// them CSV, mirror the `UserListRow` pattern and add `split_csv` here).
/// Booleans (`has_permission` / `has_override`) stay as `i32` to match
/// the wire shape the rest of this permission module exposes (see
/// `PermissionUserDetailRow`) — the front-end pattern-matches on `0/1`,
/// no `bool` conversion at this layer.
///
/// ponytail: deliberately thin (no enum mapping, no CSV split). The
/// detail endpoint carries the per-permission rows; this is the cheap
/// "show me the user list" payload only. Ceiling: when the list grows
/// past ~10K users, extend the SP with `@p_after_username` +
/// `OFFSET / FETCH NEXT` and drop the in-Rust cursor pagination.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct PermissionUserListRow {
    /// `user.user_guid` (36-char UUID).
    pub user_guid: String,
    /// `first_name + ' ' + last_name` from `[user]`, COALESCEd to "".
    pub full_name: String,
    /// `user_username.user_username_username` (login handle).
    pub email: String,
    /// Active role code (e.g. `"SUPER_ADMIN"`). Replaces the legacy
    /// `user_role_name` singular string — when a user holds multiple
    /// roles the SP will emit CSV (1:1 with `role_names` by index).
    pub role_codes: String,
    /// Active role display name (e.g. `"Super Admin"`), 1:1 by index
    /// with `role_codes`.
    pub role_names: String,
    /// `1` when the user has at least one effective permission, `0`
    /// otherwise. Cheap "has any perms?" badge — the full code list
    /// lives behind the detail endpoint.
    pub has_permission: bool,
    /// `1` when the user has any explicit override (allow or deny)
    /// recorded in `user_permission_override`, `0` otherwise.
    pub has_override: bool,
    /// `user.user_status` raw int. Front-end maps to a status badge;
    /// no enum mapping at this layer (matches `PermissionUserDetailRow`
    /// convention).
    pub user_status: i32,
    /// `user_username.user_username_status` raw int. Internal to the
    /// auth flow — surfaced for a future "username inactive" indicator.
    pub user_username_status: i32,
    /// `user.user_create_at` (datetime2 UTC, AGENTS.md § 7.4).
    pub user_create_at: DateTime<Utc>,
    /// `user.user_update_at` (datetime2 UTC). The SP layer uses
    /// `ISNULL(user_update_at, user_create_at)` so this never comes
    /// back NULL in practice.
    pub user_update_at: DateTime<Utc>,
}

/// One row of `dbo.SP_PERMISSION_USER_DETAIL_FIND_BY_GUID` — the flat
/// per-permission-for-user view of a single user.
///
/// One user with N effective permissions → N rows. The SP also
/// returns one row per `(user_permission)` pair that exists in the
/// catalog even when the user doesn't hold it (with
/// `effective_status = 0`), so the admin UI can render the full
/// permission matrix without a second lookup.
///
/// `has_override` and `effective_status` are `i32` (0/1) — not
/// `bool` — because the wire payload is consumed by an
/// auto-generated admin frontend where booleans round-trip poorly
/// through optional-chaining libraries.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct PermissionUserDetailRow {
    /// `user.user_guid` echoed back from the SP.
    pub user_guid: String,
    /// `first_name + ' ' + last_name`, COALESCEd to "".
    pub full_name: String,
    /// `user_username.user_username_username` (login handle).
    pub email: String,
    /// Active role name (single string). When multiple roles are
    /// active, the SP picks the canonical one by `user_role_code`.
    pub user_role_name: String,

    /// `user_permission.user_permission_guid` (36-char UUID).
    pub user_permission_guid: String,
    /// `user_permission.user_permission_code` (SCREAMING_SNAKE_CASE,
    /// e.g. `PAGE_DASHBOARD_VIEW`).
    pub user_permission_code: String,
    /// `user_permission.user_permission_name` (English display label).
    pub user_permission_name: String,
    /// `1` when the user has an explicit override (allow or deny)
    /// on this `(user, permission)` pair, `0` otherwise.
    pub has_override: bool,
    /// Override effect: `"allow"`, `"deny"`, or `""` (no override).
    pub override_effect: String,
    /// Final computed status after role grants + override
    /// resolution. `0` when an explicit deny wins, `1` otherwise.
    pub effective_status: bool,
}

/// The grouped wire payload returned by
/// `GET /api/v1/permission/users/:guid/permissions` (and the admin
/// counterpart).
///
/// User identity is hoisted to the outer object; per-permission
/// rows are nested under `permissions`. The flat detail row is the
/// SP output; this struct is the **application-layer grouping** the
/// admin / permission-page UI consumes.
///
/// ## Shape contract
///
/// ```json
/// {
///   "user_guid": "...",
///   "full_name": "...",
///   "email": "...",
///   "user_role_name": "Super Admin",
///   "permissions": [
///     { "user_permission_code": "...", "has_override": 0, "effective_status": 1 },
///     ...
///   ]
/// }
/// ```
///
/// An empty `permissions: []` (no errors) is a legitimate response
/// when the user exists but holds no effective permissions yet —
/// the UI renders an empty-state placeholder.
///
/// ponytail: the inner entry carries the **three** fields the front-end
/// pattern-matches on (`code`, `has_override`, `effective_status`).
/// The richer fields (`user_permission_name`, `override_effect`) live
/// on the flat [`PermissionUserDetailRow`] so the detail dump stays
/// complete. Ceiling: if the front-end starts needing `user_permission_name`
/// in the grouped view too, hoist it onto the inner entry — single
/// line of code, single struct field.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct PermissionUserGroup {
    /// `user.user_guid` (36-char UUID).
    pub user_guid: String,
    /// `first_name + ' ' + last_name`, COALESCEd to "".
    pub full_name: String,
    /// `user_username.user_username_username` (login handle).
    pub email: String,
    /// Active role name (single string).
    pub user_role_name: String,
    /// Per-permission rows. Empty when the user holds no effective
    /// permissions (or when all rows were revoked).
    pub permissions: Vec<PermissionUserGroupEntry>,
}

/// One entry inside [`PermissionUserGroup::permissions`].
///
/// Carries the three fields the front-end pattern-matches on. The
/// richer [`PermissionUserDetailRow`] is exposed separately by the
/// flat `find_permission_user_detail` method for clients that want
/// the full row set in one round-trip.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct PermissionUserGroupEntry {
    /// `user_permission.user_permission_guid` (36-char UUID).
    pub user_permission_guid: String,
    /// `user_permission.user_permission_code` (SCREAMING_SNAKE_CASE).
    pub user_permission_code: String,
    /// `1` when the user has an explicit override, `0` otherwise.
    pub has_override: bool,
    /// Final computed status. `0` when an explicit deny wins,
    /// `1` otherwise.
    pub effective_status: bool,
}

// ============================================================================
// Permission-override update DTOs (M18 — batch upsert).
// ============================================================================
//
// Backed by `dbo.SP_PERMISSION_USER_OVERRIDE_UPDATE`. One request
// item maps to one SP call; the adapter loops the list and each
// SP call is its own transaction (the SP does its own
// BEGIN / COMMIT).
//
// Field names are deliberately **short** at the wire layer
// (`user_guid` / `permission_guid` / `effect`) — the adapter maps
// them to the SP's full `user_permission_override_*` parameter
// names so the domain type doesn't carry the prefix noise.

/// One item in the request body for
/// `POST /api/v1/permission/overrides` (batch upsert).
///
/// Each item is an upsert: if a row already exists for
/// `(user_guid, permission_guid)`, the SP flips its
/// `effect` / `reason` / `assigned_by` / `status`. If no row
/// exists, the SP inserts a new one. The adapter calls the SP
/// once per item — each call is its own transaction, so a single
/// failed item does not abort the rest of the batch.
///
/// ## Field semantics (mirrors the SP contract)
///
/// | Field           | Required | Default     | Notes                              |
/// |-----------------|----------|-------------|------------------------------------|
/// | `user_guid`     | yes      | —           | Must resolve to a non-deleted user |
/// | `permission_guid` | yes    | —           | Must resolve to a non-deleted permission |
/// | `effect`        | yes      | —           | `"allow"` or `"deny"` (case-insensitive; SP lowercases) |
/// | `reason`        | no       | `NULL`      | Free-form text                     |
/// | `assigned_by`   | no       | `update_by` | Auditor's identity; defaults to the actor when omitted |
/// | `status`        | no       | `1`         | `0` = soft-deleted, `1` = active   |
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct PermissionOverrideUpdateItem {
    /// Target user (36-char UUID, matches `[user].user_guid`).
    pub user_guid: String,
    /// Target permission (36-char UUID, matches
    /// `[user_permission].user_permission_guid`).
    pub permission_guid: String,
    /// Effect to apply: `"allow"` or `"deny"`. Case-insensitive
    /// (the SP lowercases before validating). Any other value
    /// → SP returns `INVALID_EFFECT`.
    pub effect: String,
    /// Optional free-form reason (stored in
    /// `user_permission_override_reason`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Optional auditor identity (stored in
    /// `user_permission_override_assigned_by`). Defaults to the
    /// actor's GUID when omitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assigned_by: Option<String>,
    /// Optional row status: `0` = soft-deleted, `1` = active.
    /// Defaults to `1` when omitted. Any other value
    /// → SP returns `INVALID_STATUS`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<i32>,
}

/// One row of the per-item result returned by the adapter.
///
/// Mirrors `SP_PERMISSION_USER_OVERRIDE_UPDATE`'s single-row
/// output 1:1. The adapter calls the SP once per input item and
/// packages each response row into one of these — order is
/// preserved (results[i] corresponds to items[i]).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct PermissionOverrideUpdateResult {
    /// `1` for the success codes (`UPDATED` / `CREATED`), `0` for
    /// per-item validation rejections and unexpected errors.
    pub success: bool,
    /// Stable machine code. One of:
    /// - `"UPDATED"` — junction row existed, status flipped.
    /// - `"CREATED"` — junction row was missing, a new one was
    ///   inserted.
    /// - `"INVALID_EFFECT"` — `effect` was not `allow` / `deny`.
    /// - `"INVALID_STATUS"` — `status` was not `0` / `1`.
    /// - `"USER_NOT_FOUND"` — `user_guid` doesn't resolve (or is
    ///   soft-deleted).
    /// - `"PERMISSION_NOT_FOUND"` — `permission_guid` doesn't
    ///   resolve (or is soft-deleted).
    /// - `"ERROR"` — the SP's CATCH block fired (unexpected DB
    ///   error). `message` carries the SQL Server error text.
    pub code: String,
    /// Human-readable English message from the SP. Admin-only
    /// debug surface — not localized through i18n.
    pub message: String,
    /// `user_permission_override_guid` — populated on `UPDATED` /
    /// `CREATED`, otherwise `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_permission_override_guid: Option<String>,
    /// Echo of the input `user_guid` for caller convenience.
    pub user_permission_override_user_guid: String,
    /// Echo of the input `permission_guid` for caller convenience.
    pub user_permission_override_permission_guid: String,
    /// Echo of the input `effect` (post-lowercase, so callers see
    /// exactly what landed in the row).
    pub user_permission_override_effect: String,
    /// Echo of the input `status` (post-coalesce, so callers see
    /// the value that landed in the row).
    pub user_permission_override_status: i32,
}

impl PermissionOverrideUpdateResult {
    /// SP's success code: junction row existed, status flipped.
    pub const CODE_UPDATED: &'static str = "UPDATED";
    /// SP's success code: junction row was missing, inserted.
    pub const CODE_CREATED: &'static str = "CREATED";
    /// SP rejected the input — `effect` was not `allow` / `deny`.
    pub const CODE_INVALID_EFFECT: &'static str = "INVALID_EFFECT";
    /// SP rejected the input — `status` was not `0` / `1`.
    pub const CODE_INVALID_STATUS: &'static str = "INVALID_STATUS";
    /// SP rejected the input — `user_guid` doesn't resolve (or is
    /// soft-deleted).
    pub const CODE_USER_NOT_FOUND: &'static str = "USER_NOT_FOUND";
    /// SP rejected the input — `permission_guid` doesn't resolve
    /// (or is soft-deleted).
    pub const CODE_PERMISSION_NOT_FOUND: &'static str = "PERMISSION_NOT_FOUND";
    /// SP's CATCH block fired (unexpected DB error). The adapter
    /// never surfaces this as a row when tiberius propagates the
    /// underlying `THROW` — it would instead reach
    /// `RepoError::Backend`. The constant lives here for parity
    /// with the SP's documented code set.
    pub const CODE_ERROR: &'static str = "ERROR";

    /// `true` for the success codes (`UPDATED` / `CREATED`).
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
