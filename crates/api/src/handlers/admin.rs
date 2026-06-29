//! Admin HTTP handlers (M14.5 register-role split + M15 + M16 + T-06 refactor).
//!
//! - `POST /api/v1/admin/users` — admin-only user creation (M14.5).
//! - `GET /api/v1/admin/users` — admin-only user listing (M16,
//!   backed by `dbo.SP_PERMISSION_USER_LIST`).
//! - `GET /api/v1/admin/users/:guid/permissions` — per-user detailed
//!   permission rows (M16, backed by
//!   `dbo.SP_PERMISSION_USER_FIND_BY_USERNAME`). Handler translates
//!   GUID → username via the existing `UserRepository::find_by_id`
//!   path so the wire contract stays stable.
//! - `GET /api/v1/admin/permissions?mode=<literal>` — role × permission
//!   matrix (M15-prep). The `mode` is a pass-through literal the SP
//!   uses to scope which role set to return (e.g. `SELECT_ADMIN`,
//!   `SELECT_EMPLOYEE`); the service does not validate it.
//! - `POST /api/v1/admin/permissions` — bulk grant / revoke role ×
//!   permission pairs (M15). Wraps
//!   `dbo.SP_USER_ROLE_PERMISSION_UPDATE` and reports per-item
//!   results so the admin UI can show which GUID failed.
//!
//! **T-06**: the bespoke `forbidden` / `validation` envelope
//! helpers were deleted; role + RBAC failures now build an
//! [`ApiError`] and call [`crate::error::IntoLocalizedResponse::into_localized_response`]
//! like every other handler.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use kokkak_application::auth::RegisterInput;
use kokkak_application::user_role::{PermissionUpdateInput, UpdatePermissionsInput};
use kokkak_common::error::AppError;
use kokkak_common::error_codes::ErrorCode;
use kokkak_common::i18n::{current_locale, tr};
use kokkak_common::response::{created, paginated, ApiResponse, PageMeta};
use kokkak_domain::{
    PermissionUpdateRow, PermissionUserGroup, RepoError, Role, UserListRow, UserRoleWithPermissions,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::{Validate, ValidationError};

use crate::error::{ApiError, IntoLocalizedResponse};
use crate::extractors::ValidatedJson;
use crate::handlers::auth::AuthResponse;
use crate::middleware::auth::AuthnUser;
use crate::state::AppState;

#[derive(Debug, Deserialize, Validate, utoipa::ToSchema)]
pub struct CreateUserRequest {
    #[validate(length(min = 3, max = 64, message = "username must be 3-64 characters"))]
    pub username: String,
    #[validate(length(min = 8, max = 128, message = "password must be 8-128 characters"))]
    pub password: String,
    #[validate(length(min = 1, max = 100, message = "first_name must be 1-100 characters"))]
    pub first_name: String,
    #[validate(length(min = 1, max = 100, message = "last_name must be 1-100 characters"))]
    pub last_name: String,
    /// Required for the admin endpoint: must be one of
    /// `customer` / `technician` / `admin` / `super_admin`.
    #[validate(length(min = 1, max = 20, message = "role must be 1-20 characters"))]
    pub role: String,
}

/// `POST /api/v1/admin/users` — admin-only user creation.
///
/// M14.5 split: this is the only place that can create accounts
/// with `Admin` or `SuperAdmin` roles. The public register endpoint
/// is locked down to `customer` / `technician`; the admin page uses
/// this endpoint to provision staff accounts.
///
/// Requires the caller to hold a JWT carrying `Admin` or
/// `SuperAdmin` (mirrors the pattern in `handlers::payment::list_payouts_admin`).
#[utoipa::path(
    post,
    path = "/api/v1/admin/users",
    tag = "admin",
    request_body = CreateUserRequest,
    responses(
        (status = 201, description = "User created (admin-created)", body = kokkak_domain::PublicUser),
        (status = 401, description = "Not authenticated", body = crate::openapi::ApiError),
        (status = 403, description = "Not an admin", body = crate::openapi::ApiError),
        (status = 409, description = "Username already taken", body = crate::openapi::ApiError),
        (status = 422, description = "Validation error", body = crate::openapi::ApiError),
    ),
    security(("bearer_auth" = []))
)]
pub async fn create_user_admin(
    State(state): State<AppState>,
    user: AuthnUser,
    ValidatedJson(req): ValidatedJson<CreateUserRequest>,
) -> Result<Response, Response> {
    // 1. RBAC: only admins / super_admins may create accounts here.
    if !user.has_role(Role::Admin) && !user.has_role(Role::SuperAdmin) {
        // AdminRequired carries the admin_required key — the admin
        // page surfaces this directly to the operator. The message
        // is pre-localized via the file-based catalog (no repo
        // override for this message yet), then handed to AppError's
        // Localized carrier so IntoResponse surfaces it verbatim.
        let localized = tr("err_auth.admin_required", &current_locale(), &[]);
        return Err(
            ApiError::from(AppError::AdminRequired.with_message(localized)).into_response(),
        );
    }

    // 2. Parse the role. Unlike the public register endpoint, all
    //    four roles are accepted here; an unknown string is a 422
    //    role_not_allowed.
    let role = match Role::from_code(&req.role) {
        Some(r) => r,
        None => {
            return Err(ApiError::from(AppError::RoleNotAllowed(req.role))
                .into_localized_response(&state)
                .await);
        }
    };

    // 3. Delegate to the same application service the public
    //    register uses. Re-using `AuthService::register` keeps the
    //    password hashing, username normalisation, and repo
    //    conflict mapping in one place.
    let input = RegisterInput {
        username: req.username,
        password: req.password,
        first_name: req.first_name,
        last_name: req.last_name,
        role,
    };
    let outcome = match state.auth.register(input).await {
        Ok(o) => o,
        Err(e) => return Err(ApiError::from(e).into_localized_response(&state).await),
    };
    Ok((StatusCode::CREATED, created(AuthResponse::from(outcome))).into_response())
}

// ============================================================================
// M16: GET /api/v1/admin/users  (LIST — backed by SP_PERMISSION_USER_LIST)
// ============================================================================
//
// Admin-only user listing. Backed by
// `dbo.SP_PERMISSION_USER_LIST` (one row per user with permission
// summary CSVs). Cursor pagination is applied in the application
// service (`UserService::list_users`) on top of the SP result.
//
// **Scope**: this handler is a thin wrapper over
// [`kokkak_application::user::UserService::list_users`]. The
// permission module (`MssqlUserRoleRepository` /
// `UserRoleRepository` / `UserRoleService`) is **not** touched
// here — the per-user permissions endpoint below also reuses
// the existing `UserRepository::find_by_id` path instead.

/// Query parameters for the admin user listing.
///
/// Mirrors `handlers::order::ListQuery` so the cursor-pagination
/// contract is uniform across admin list endpoints (`limit` +
/// opaque `after` cursor; `next_cursor` + `has_next` in the
/// response meta).
#[derive(Debug, Deserialize, utoipa::ToSchema, utoipa::IntoParams)]
pub struct ListUsersQuery {
    /// Opaque cursor returned by the previous page
    /// (callers MUST treat as a black box).
    pub after: Option<String>,
    /// Max users per page. Defaults to 20, hard cap 100 so a
    /// runaway client can't dump the whole table in one shot.
    pub limit: Option<u32>,
}

/// One row in the admin user listing — 1:1 with
/// `dbo.SP_PERMISSION_USER_LIST`'s SELECT list. CSVs in the SP
/// (`role_codes`, `role_names`) are split into `Vec<String>` at
/// the infra layer so the wire shape never carries CSV strings.
///
/// M16 round 2: the LIST SP (`SP_PERMISSION_USER_LIST`) no
/// longer ships `permission_codes` (only the cheap
/// `has_permission` boolean) — the full code list lives behind
/// the detail endpoint
/// (`GET /api/v1/admin/users/:guid/permissions`).
pub type UserListItem = UserListRow;

/// `GET /api/v1/admin/users?after=&limit=`
///
/// Admin-only listing of users. Backed by
/// `dbo.SP_PERMISSION_USER_LIST` (one row per user). The wire
/// payload (`Vec<UserListRow>`) and the pagination contract
/// (`?after=&limit=`) are pinned; adding more columns to the
/// SP only widens the row struct, never breaks the contract.
#[utoipa::path(
        get,
        path = "/api/v1/admin/users",
        tag = "admin",
        params(ListUsersQuery),
        responses(
            (status = 200, description = "Page of users (placeholder until SP_USER_LIST lands)", body = Vec<UserListItem>),
            (status = 401, description = "Not authenticated", body = crate::openapi::ApiError),
            (status = 403, description = "Not an admin", body = crate::openapi::ApiError),
        ),
        security(("bearer_auth" = []))
    )]
pub async fn list_users_admin(
    State(state): State<AppState>,
    user: AuthnUser,
    Query(q): Query<ListUsersQuery>,
) -> Result<Response, Response> {
    // 1. RBAC: only admins / super_admins may list users.
    if !user.has_role(Role::Admin) && !user.has_role(Role::SuperAdmin) {
        let localized = tr("err_auth.admin_required", &current_locale(), &[]);
        return Err(
            ApiError::from(AppError::AdminRequired.with_message(localized)).into_response(),
        );
    }

    // 2. Cap the limit so a runaway client can't dump the whole
    //    table in one shot. 100 covers the admin UI's "show 50 / 100"
    //    paginators with headroom.
    let limit = q.limit.unwrap_or(20).clamp(1, 100);

    // 3. Delegate to the application service. The placeholder
    //    returns an empty page + `next_cursor = None` until the SP
    //    arrives — the wire shape is stable so swapping in the
    //    real implementation doesn't require any client-side
    //    change.
    //    M19: forward `user.id()` as caller for the SP admin gate.
    let page = match state.user.list_users(q.after, limit, user.id()).await {
        Ok(p) => p,
        Err(e) => return Err(ApiError::from(e).into_localized_response(&state).await),
    };

    let meta = PageMeta {
        limit: limit as usize,
        has_next: page.next_cursor.is_some(),
        next_cursor: page.next_cursor,
    };
    Ok((StatusCode::OK, paginated(page.items, meta)).into_response())
}

// ============================================================================
// M17: GET /api/v1/admin/users/:guid/permissions  (per-user detail)
// ============================================================================
//
// Admin-only lookup of one user's detailed permission rows.
// Backed by `dbo.SP_PERMISSION_USER_DETAIL_FIND_BY_GUID`, which
// accepts a **GUID** (`@p_user_guid`) directly — no GUID→username
// translation in Rust. The permission flow now owns its SP end-to-end.
//
// **M17: decoupled from `UserRepository`.** Before M17 this
// endpoint shared `UserRepository::find_by_id` +
// `find_user_permissions_by_username` with the login/auth flow
// and the permission page. It now goes through the dedicated
// [`PermissionUserRepository`] port (same as the permission page)
// so the permission-domain code path is shared by exactly two
// callers (admin + permission page) and zero coupling to login.

/// `GET /api/v1/admin/users/:guid/permissions`
///
/// Return the per-user permission detail for the admin permission
/// management screen. Wire shape: [`PermissionUserGroup`] — user
/// identity hoisted to the outer object, per-permission rows
/// nested under `permissions` (one entry per `(user, permission)`
/// pair).
///
/// The SP takes a **GUID** (`@p_user_guid`) directly. The handler
/// no longer needs the GUID→username translation step the M16
/// design used; the URL's GUID is forwarded as-is.
///
/// RBAC: `Admin` or `SuperAdmin` only.
///
/// Errors:
/// - 401 `unauthorized` — missing / invalid Bearer token.
/// - 403 `admin_required` — caller lacks `Admin` / `SuperAdmin`.
/// - 404 `not_found` — GUID doesn't resolve to a user.
/// - 500 `internal` — unexpected backend failure (via
///   `into_localized_response`).
///
/// An empty `permissions: []` (200) is a legitimate response when
/// the user exists but holds no effective permissions — the admin UI
/// renders an empty-state placeholder.
#[utoipa::path(
    get,
    path = "/api/v1/admin/users/{guid}/permissions",
    tag = "admin",
    params(
        ("guid" = Uuid, Path, description = "User GUID (36-char UUID)"),
    ),
    responses(
        (status = 200, description = "Grouped permission detail for the user", body = PermissionUserGroup),
        (status = 401, description = "Not authenticated", body = crate::openapi::ApiError),
        (status = 403, description = "Not an admin", body = crate::openapi::ApiError),
        (status = 404, description = "User not found", body = crate::openapi::ApiError),
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_user_permissions_admin(
    State(state): State<AppState>,
    user: AuthnUser,
    Path(guid): Path<Uuid>,
) -> Result<Response, Response> {
    // 1. RBAC: only admins / super_admins may inspect per-user perms.
    if !user.has_role(Role::Admin) && !user.has_role(Role::SuperAdmin) {
        let localized = tr("err_auth.admin_required", &current_locale(), &[]);
        return Err(
            ApiError::from(AppError::AdminRequired.with_message(localized)).into_response(),
        );
    }

    // 2. axum's `Path<Uuid>` extractor already rejected any
    //    non-UUID path segment with 400 before this point. The
    //    handler no longer needs a defensive GUID check.

    // 3. Delegate to the permission-page application service. The
    //    service calls `dbo.SP_PERMISSION_USER_DETAIL_FIND_BY_GUID`
    //    with the GUID directly — no GUID→username translation
    //    needed (M17). The wire payload is the grouped
    //    [`PermissionUserGroup`] (user identity hoisted, per-permission
    //    rows nested).
    //    M19: forward `user.id()` as caller for the SP admin gate.
    let group = match state
        .permission
        .get_permission_user_group(guid, user.id())
        .await
    {
        Ok(g) => g,
        Err(RepoError::NotFound(_)) => {
            let locale = current_locale();
            let localized = tr("err_auth.user_not_found", &locale, &[&guid.to_string()]);
            return Err(ApiError::from(
                AppError::NotFound(guid.to_string()).with_message(localized),
            )
            .into_response());
        }
        Err(e) => return Err(ApiError::from(e).into_localized_response(&state).await),
    };

    // 4. Standard 200 envelope (no `meta` — a single-user lookup
    //    isn't paginated; if a "paged effective permissions per
    //    user" endpoint lands later, add `meta` then).
    Ok((
        StatusCode::OK,
        Json(ApiResponse::<PermissionUserGroup> {
            success: true,
            data: Some(group),
            error: None,
            meta: None,
        }),
    )
        .into_response())
}

// ============================================================================
// M16 round 2 cleanup: the second copy of `ListUsersQuery` +
// `list_users_admin` + `list_user_permissions_admin` that used to live
// below was dead code (no caller — `router.rs` and `openapi.rs` reference
// the live versions further up in this file) and it blocked
// compilation because of conflicting `#[derive(utoipa::IntoParams)]` +
// `#[utoipa::path(...)]` macro expansions on the same names. Removed
// in this commit so `cargo check` / `cargo test` can run again. The
// live handlers + struct live in the M16 sections above.
// ============================================================================

// ============================================================================
// M15-prep: GET /api/v1/admin/permissions
// ============================================================================

/// Query parameters for the role × permission matrix endpoint.
///
/// `mode` is **required** — it's the pass-through literal the SP
/// uses to scope which role set to return (e.g. `SELECT_ADMIN`,
/// `SELECT_EMPLOYEE`). The handler does not validate the value;
/// unknown modes return zero rows from the SP, which the wire
/// payload surfaces as an empty list (graceful, not 404).
#[derive(Debug, Deserialize, utoipa::ToSchema, utoipa::IntoParams)]
pub struct PermissionsQuery {
    /// Pass-through mode literal forwarded to the SP (e.g.
    /// `SELECT_ADMIN`, `SELECT_EMPLOYEE`). Application-defined;
    /// unknown values return an empty list.
    pub mode: String,
}

/// `GET /api/v1/admin/permissions?mode=<literal>`
///
/// Read-only view of the role × permission matrix, grouped by
/// role. The `mode` is forwarded to the SP verbatim — no
/// transformation, no enum validation, no trimming.
///
/// RBAC: `Admin` or `SuperAdmin` only. The `admin_flag`
/// middleware (T-31) also gates the route behind the Strangler
/// flag, so flipping `KOKKAK_MIDDLEWARE__FEATURES__ADMIN=false`
/// hands the route back to the legacy ASP.NET service.
#[utoipa::path(
    get,
    path = "/api/v1/admin/permissions",
    tag = "admin",
    params(PermissionsQuery),
    responses(
        (status = 200, description = "Role × permission matrix (grouped by role)", body = Vec<UserRoleWithPermissions>),
        (status = 400, description = "Missing or empty `mode` query parameter", body = crate::openapi::ApiError),
        (status = 401, description = "Not authenticated", body = crate::openapi::ApiError),
        (status = 403, description = "Not an admin", body = crate::openapi::ApiError),
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_permissions(
    State(state): State<AppState>,
    user: AuthnUser,
    Query(q): Query<PermissionsQuery>,
) -> Result<Response, Response> {
    // 1. RBAC: only admins / super_admins may inspect the matrix.
    if !user.has_role(Role::Admin) && !user.has_role(Role::SuperAdmin) {
        let locale = current_locale();
        let localized = tr("err_auth.admin_required", &locale, &[]);
        return Err(
            ApiError::from(AppError::AdminRequired.with_message(localized)).into_response(),
        );
    }

    // 2. Validate the mode literal. We only require it to be
    //    non-empty (the SP accepts the value verbatim — no
    //    closed-set check on the Rust side). The endpoint has
    //    no other input dimensions, so the only way to surface
    //    a 400 here is an empty `mode=` query string.
    let mode = q.mode.trim();
    if mode.is_empty() {
        let locale = current_locale();
        let msg = tr("err_permission.mode_required", &locale, &[]);
        return Err(bad_request_envelope(&msg, ErrorCode::BAD_REQUEST));
    }

    // 3. Delegate to the application service. Repo failures
    //    map to 500 via the standard `into_localized_response`
    //    path (which falls back to the i18n-catalog message for
    //    `err_repo.backend`). Explicit type annotation is
    //    intentional — the handler return type is the generic
    //    `Result<Response, Response>` and the utoipa
    //    `body = Vec<UserRoleWithPermissions>` annotation needs
    //    the concrete type to be in scope.
    //    M19: forward `user.id()` as caller for the SP admin gate.
    let groups: Vec<UserRoleWithPermissions> =
        match state.user_roles.list_permissions(mode, user.id()).await {
            Ok(r) => r,
            Err(e) => return Err(ApiError::from(e).into_localized_response(&state).await),
        };

    Ok((
        StatusCode::OK,
        Json(ApiResponse {
            success: true,
            data: Some(groups),
            error: None,
            meta: None,
        }),
    )
        .into_response())
}

/// Build a 400 envelope in the standard shape. Ponytail helper
/// to keep the single early-return branch in `list_permissions`
/// readable — same envelope, same code, same key naming as the
/// other handlers' 400 paths.
fn bad_request_envelope(message: &str, code: &'static str) -> Response {
    let envelope: ApiResponse<()> = ApiResponse {
        success: false,
        data: None,
        error: Some(kokkak_common::error::ApiErrorBody {
            code: code.into(),
            message: message.to_string(),
        }),
        meta: None,
    };
    (StatusCode::BAD_REQUEST, Json(envelope)).into_response()
}

// ============================================================================
// M15: POST /api/v1/admin/permissions
// ============================================================================

/// Maximum number of items the bulk endpoint accepts in one request.
///
/// 500 is a deliberate ceiling — the typical admin UI sends the full
/// permission catalog for one role (currently 80+ permissions and
/// growing), so 500 covers the foreseeable headroom without inviting
/// accidental DoS. Anything larger should be split client-side.
///
/// Typed as `u64` because the `validator` crate's `length` validator
/// for `Vec<T>` accepts `min` / `max` as `u64` (matches the
/// `TryFrom<usize>` convention it uses for slice lengths).
const MAX_BULK_PERMISSION_UPDATES: u64 = 500;

/// One item inside [`UpdatePermissionsRequest::updates`].
///
/// The struct is flat (no nested object) so mobile / admin SDKs can
/// generate strongly-typed arrays without manual mapping. Field names
/// match the SP parameter names verbatim — the handler passes them
/// through to the service without rename. `Serialize` is required by
/// `utoipa::ToSchema` (the macro generates the OpenAPI schema by
/// running the type through `serde_json`).
#[derive(Debug, Deserialize, Serialize, Validate, utoipa::ToSchema)]
pub struct PermissionUpdateItem {
    /// `user_role_guid` — 36-char UUID.
    #[validate(length(min = 36, max = 36, message = "user_role_guid must be a 36-char GUID"))]
    pub user_role_guid: String,
    /// `user_permission_guid` — 36-char UUID.
    #[validate(length(
        min = 36,
        max = 36,
        message = "user_permission_guid must be a 36-char GUID"
    ))]
    pub user_permission_guid: String,
    /// `1` = grant, `0` = revoke. The handler rejects anything else
    /// with 422 + `validation` before the SP sees it.
    #[validate(custom(
        function = "validate_status",
        message = "user_role_permission_status must be 0 or 1"
    ))]
    pub user_role_permission_status: i32,
}

/// `validator` function: `status` must be `0` or `1`.
fn validate_status(status: i32) -> Result<(), ValidationError> {
    if status == 0 || status == 1 {
        Ok(())
    } else {
        Err(ValidationError::new("invalid_status"))
    }
}

/// Validate the GUID-string format (`Uuid::parse_str` succeeds).
/// Used by the handler after `validator` runs to surface a more
/// precise 422 message ("not a valid UUID") than the generic
/// length-only check.
fn is_valid_guid(s: &str) -> bool {
    Uuid::parse_str(s).is_ok()
}

/// Body for `POST /api/v1/admin/permissions`.
///
/// Wraps a list of [`PermissionUpdateItem`]s plus an optional
/// `update_by` audit field. `update_by` defaults to the
/// authenticated admin's GUID when omitted (the typical case).
#[derive(Debug, Deserialize, Validate, utoipa::ToSchema)]
pub struct UpdatePermissionsRequest {
    /// 1–500 per-item updates. Empty lists fail validation here
    /// (callers should not POST an empty batch).
    #[validate(length(
        min = 1,
        max = MAX_BULK_PERMISSION_UPDATES,
        message = "updates must have 1-500 items"
    ))]
    #[validate(nested)]
    pub updates: Vec<PermissionUpdateItem>,
    /// Audit field — recorded in `user_role_permission_update_by`.
    /// Optional: the handler defaults to the authenticated admin's
    /// GUID so the audit trail is never empty.
    #[validate(length(max = 36, message = "update_by must be at most 36 chars"))]
    pub update_by: Option<String>,
}

/// Per-item outcome — one entry per input item, in input order.
///
/// The fields mirror [`PermissionUpdateRow`] 1:1 (the service returns
/// the domain type and the handler projects it onto the wire). We
/// keep the SP's English `message` (admin-only debug surface — not
/// localized through i18n) and surface `success` + `code` for the
/// admin UI to pattern-match.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct PermissionUpdateResultItem {
    /// Echo of the input `user_role_guid` for caller convenience.
    pub user_role_guid: String,
    /// Echo of the input `user_permission_guid`.
    pub user_permission_guid: String,
    /// `true` for `UPDATED` / `CREATED` / `NO_CHANGE`,
    /// `false` for the per-item error codes.
    pub success: bool,
    /// Stable machine code — one of `UPDATED`, `CREATED`,
    /// `NO_CHANGE`, `ROLE_NOT_FOUND`, `PERMISSION_NOT_FOUND`,
    /// `INVALID_STATUS`.
    pub code: String,
    /// `user_role_permission_guid` when the row was mutated, else
    /// `None`. Serialized as `null` so the field stays present in
    /// the wire shape (admin UI relies on the key existing).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_role_permission_guid: Option<String>,
    /// Echo of the input status.
    pub user_role_permission_status: i32,
    /// Human-readable English message from the SP.
    pub message: String,
}

impl From<PermissionUpdateRow> for PermissionUpdateResultItem {
    fn from(r: PermissionUpdateRow) -> Self {
        Self {
            user_role_guid: r.user_role_guid,
            user_permission_guid: r.user_permission_guid,
            success: r.success,
            code: r.code,
            user_role_permission_guid: r.user_role_permission_guid,
            user_role_permission_status: r.user_role_permission_status,
            message: r.message,
        }
    }
}

/// Aggregated response — per-item results plus a top-level summary
/// so the admin UI can render "N updated, M failed" without
/// re-scanning the array.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct UpdatePermissionsResponse {
    /// Total items in the request.
    pub total: usize,
    /// Items where the SP flipped an existing row.
    pub updated: usize,
    /// Items where the SP created a new row.
    pub created: usize,
    /// Items where the SP returned no row (status = 0 on a pair
    /// that wasn't granted — silent no-op, treated as success).
    pub no_change: usize,
    /// Items where the SP rejected the input
    /// (`ROLE_NOT_FOUND`, `PERMISSION_NOT_FOUND`, `INVALID_STATUS`).
    pub failed: usize,
    /// Per-item results in input order.
    pub results: Vec<PermissionUpdateResultItem>,
}

/// `POST /api/v1/admin/permissions` — bulk grant / revoke.
///
/// Wraps `dbo.SP_USER_ROLE_PERMISSION_UPDATE` per item. The handler
/// does **not** wrap the loop in a transaction on purpose: each
/// item commits independently so a single bad GUID doesn't roll
/// back the entire batch (admin UX). The response surfaces
/// per-item status so the operator can retry just the failed
/// items.
///
/// RBAC: `Admin` or `SuperAdmin`. Same gate as the existing
/// `GET /api/v1/admin/permissions` route — the `admin_flag`
/// middleware also gates the Strangler flag at the boundary.
///
/// ponytail: validation is layered (length by `validator`, GUID
/// shape by `Uuid::parse_str`, status range by a custom
/// `validator` fn). The ceiling is when `validator` becomes a
/// tax for 90% of trivial cases — at that point a derive macro
/// on `PermissionUpdateItem` that emits the per-field checks
/// earns its keep.
#[utoipa::path(
    post,
    path = "/api/v1/admin/permissions",
    tag = "admin",
    request_body = UpdatePermissionsRequest,
    responses(
        (status = 200, description = "Per-item results (always 200; per-item `success` field carries the outcome)", body = UpdatePermissionsResponse),
        (status = 400, description = "Malformed JSON body", body = crate::openapi::ApiError),
        (status = 401, description = "Not authenticated", body = crate::openapi::ApiError),
        (status = 403, description = "Not an admin", body = crate::openapi::ApiError),
        (status = 422, description = "Validation error (empty list, bad GUID, status not in {0,1})", body = crate::openapi::ApiError),
    ),
    security(("bearer_auth" = []))
)]
pub async fn update_permissions_admin(
    State(state): State<AppState>,
    user: AuthnUser,
    ValidatedJson(req): ValidatedJson<UpdatePermissionsRequest>,
) -> Result<Response, Response> {
    // 1. RBAC — Admin or SuperAdmin only.
    if !user.has_role(Role::Admin) && !user.has_role(Role::SuperAdmin) {
        let locale = current_locale();
        let localized = tr("err_auth.admin_required", &locale, &[]);
        return Err(
            ApiError::from(AppError::AdminRequired.with_message(localized)).into_response(),
        );
    }

    // 2. Per-item GUID shape — `validator`'s length check
    //    accepts 36-char strings that aren't valid UUIDs
    //    (e.g. "00000000-0000-0000-0000-00000000000Z").
    //    We layer a `Uuid::parse_str` check on top so the
    //    error message is precise.
    for (i, item) in req.updates.iter().enumerate() {
        if !is_valid_guid(&item.user_role_guid) {
            let locale = current_locale();
            let msg = tr("err_permission.invalid_role_guid", &locale, &[]);
            return Err(validation_envelope(&msg, i, "user_role_guid"));
        }
        if !is_valid_guid(&item.user_permission_guid) {
            let locale = current_locale();
            let msg = tr("err_permission.invalid_permission_guid", &locale, &[]);
            return Err(validation_envelope(&msg, i, "user_permission_guid"));
        }
    }

    // 3. Resolve `update_by` — request body override first,
    //    fall back to the authenticated admin's GUID so the
    //    audit column is never empty.
    let update_by: Option<String> = req.update_by.or_else(|| Some(user.id().to_string()));

    // 4. Delegate to the service. The service loops, calling
    //    the SP once per item. `RepoError::Backend` (real DB
    //    failure, e.g. connection dropped mid-batch) maps to
    //    500 via the standard localized envelope — same path
    //    every other admin handler uses.
    let input = UpdatePermissionsInput {
        updates: req
            .updates
            .iter()
            .map(|i| PermissionUpdateInput {
                user_role_guid: i.user_role_guid.clone(),
                user_permission_guid: i.user_permission_guid.clone(),
                user_role_permission_status: i.user_role_permission_status,
            })
            .collect(),
        update_by,
    };
    let rows = match state.user_roles.update_permissions(input).await {
        Ok(r) => r,
        Err(e) => return Err(ApiError::from(e).into_localized_response(&state).await),
    };

    // 5. Aggregate the per-item results into the response
    //    shape the admin UI consumes. The categorization
    //    happens once on the Rust side so the UI doesn't have
    //    to re-scan the array to render "N updated, M failed".
    let mut updated = 0usize;
    let mut created = 0usize;
    let mut no_change = 0usize;
    let mut failed = 0usize;
    let results: Vec<PermissionUpdateResultItem> = rows
        .into_iter()
        .map(|r| {
            match r.code.as_str() {
                PermissionUpdateRow::CODE_UPDATED => updated += 1,
                PermissionUpdateRow::CODE_CREATED => created += 1,
                PermissionUpdateRow::CODE_NO_CHANGE => no_change += 1,
                _ => failed += 1,
            }
            PermissionUpdateResultItem::from(r)
        })
        .collect();
    let total = results.len();

    Ok((
        StatusCode::OK,
        Json(ApiResponse {
            success: true,
            data: Some(UpdatePermissionsResponse {
                total,
                updated,
                created,
                no_change,
                failed,
                results,
            }),
            error: None,
            meta: None,
        }),
    )
        .into_response())
}

/// 422 envelope for per-item GUID-shape validation failures.
///
/// Mirrors the existing `bad_request_envelope` ponytail helper
/// but uses the `validation` error code (422 status) since the
/// request was syntactically valid JSON — the issue is the
/// contents of one item. `index` and `field` are appended to the
/// message so the admin UI can scroll to the offending row.
fn validation_envelope(message: &str, index: usize, field: &str) -> Response {
    let full = format!("{message} (item #{index}, field `{field}`)");
    let envelope: ApiResponse<()> = ApiResponse {
        success: false,
        data: None,
        error: Some(kokkak_common::error::ApiErrorBody {
            code: ErrorCode::VALIDATION.into(),
            message: full,
        }),
        meta: None,
    };
    (StatusCode::UNPROCESSABLE_ENTITY, Json(envelope)).into_response()
}
