//! Admin HTTP handlers (M14.5 register-role split + M15 + M16 + M20-b + T-06 refactor).
//!
//! - `POST /api/v1/admin/users` — admin-only **simple** user
//!   creation (M14.5). Wraps `dbo.API_USER_REGISTER` — accepts
//!   only first / last name + username + password + role.
//! - `POST /api/v1/admin/users/full` — admin-only **rich** user
//!   creation (M20-b). Wraps `dbo.SP_USER_INSERT_FULL` —
//!   accepts the full admin form (address, bank, position,
//!   salary, schedule, attachments). See the
//!   `admin_insert_user_full` handler below.
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

use std::sync::Arc;

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Utc};
use kokkak_application::admin_user::AdminInsertUserFullInput;
use kokkak_application::auth::{PasswordHasherPort, RegisterInput};
use kokkak_application::user_role::{PermissionUpdateInput, UpdatePermissionsInput};
use kokkak_common::error::AppError;
use kokkak_common::error_codes::ErrorCode;
use kokkak_common::i18n::{current_locale, tr};
use kokkak_common::response::{created, paginated, ApiResponse, PageMeta};
use kokkak_domain::{
    AdminInsertUserError, AdminInsertUserResult, PermissionUpdateRow, PermissionUserGroup,
    RepoError, Role, UserListRow, UserRoleWithPermissions,
};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::{Validate, ValidationError};

use crate::error::{ApiError, IntoLocalizedResponse};
use crate::extractors::ValidatedJson;
use crate::handlers::auth::AuthResponse;
use crate::middleware::auth::AuthnUser;
use crate::state::AppState;
use kokkak_infra::image_processor::UserImageKind;

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

// ============================================================================
// M20-b: POST /api/v1/admin/users/full
// ============================================================================
//
// Wraps `dbo.SP_USER_INSERT_FULL` — the rich admin-side user
// creation flow. The simpler `POST /api/v1/admin/users` (above)
// uses `API_USER_REGISTER` and is sufficient for provisioning
// basic admin accounts without an address / bank / position.
// This endpoint is for the full admin form (every column the
// legacy ASP.NET page collected).
//
// The handler is **thin**: it validates the wire DTO, hashes
// the password, and delegates to [`AdminUserService`] which in
// turn calls the SP via [`UserRepository::admin_insert_full`].
// SP error codes are mapped to HTTP status + i18n message via
// [`sp_insert_full_status`] below.

/// One day of the weekly working schedule on the wire.
///
/// Mirrors the SP's `monday_*` ... `sunday_*` columns 1:1.
/// When `is_working = true`, both `start_time` and `end_time`
/// must be `"HH:MM:SS"` strings; when `false`, both are
/// ignored (sent as NULL to the SP).
#[derive(Debug, Deserialize, Serialize, Validate, utoipa::ToSchema, Default, Clone)]
pub struct DayScheduleDto {
    /// Whether this weekday is a working day.
    pub is_working: bool,
    /// `HH:MM:SS` string. Required when `is_working = true`.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(max = 8, message = "start_time must be `HH:MM:SS`"))]
    pub start_time: Option<String>,
    /// `HH:MM:SS` string. Required when `is_working = true`.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(max = 8, message = "end_time must be `HH:MM:SS`"))]
    pub end_time: Option<String>,
}

/// Weekly working schedule — seven `DayScheduleDto`s, one per weekday.
#[derive(Debug, Deserialize, Serialize, Validate, utoipa::ToSchema, Default, Clone)]
pub struct WeeklyScheduleDto {
    /// Monday.
    #[validate(nested)]
    pub monday: DayScheduleDto,
    /// Tuesday.
    #[validate(nested)]
    pub tuesday: DayScheduleDto,
    /// Wednesday.
    #[validate(nested)]
    pub wednesday: DayScheduleDto,
    /// Thursday.
    #[validate(nested)]
    pub thursday: DayScheduleDto,
    /// Friday.
    #[validate(nested)]
    pub friday: DayScheduleDto,
    /// Saturday.
    #[validate(nested)]
    pub saturday: DayScheduleDto,
    /// Sunday.
    #[validate(nested)]
    pub sunday: DayScheduleDto,
}

/// Request body for `POST /api/v1/admin/users/full`.
///
/// Flat shape (no nested address / bank struct) to keep the
/// wire contract 1:1 with the SP parameters — the mobile /
/// admin SDKs can generate strongly-typed arrays without
/// manual mapping. Field names mirror the SP parameter names
/// verbatim (`snake_case`).
///
/// `password` is the **plaintext** — the service hashes it
/// inside the use case. Plaintext never reaches the DB
/// driver (AGENTS.md § 12.1).
#[derive(Debug, Deserialize, Validate, utoipa::ToSchema)]
pub struct AdminInsertUserRequest {
    /// Optional. The SP generates a NEWID when absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(max = 36, message = "user_guid must be a 36-char UUID"))]
    pub user_guid: Option<String>,

    #[validate(length(min = 1, max = 100, message = "first_name must be 1-100 characters"))]
    pub first_name: String,

    #[validate(length(min = 1, max = 100, message = "last_name must be 1-100 characters"))]
    pub last_name: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(max = 100, message = "id_card must be at most 100 characters"))]
    pub id_card: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(max = 50, message = "tel must be at most 50 characters"))]
    pub tel: Option<String>,

    #[validate(length(min = 1, max = 255, message = "email must be 1-255 characters"))]
    pub email: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(max = 50, message = "gender must be at most 50 characters"))]
    pub gender: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(max = 36, message = "country_guid must be a 36-char UUID"))]
    pub country_guid: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(max = 255, message = "province must be at most 255 characters"))]
    pub province: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(max = 255, message = "district must be at most 255 characters"))]
    pub district: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(max = 255, message = "sub_district must be at most 255 characters"))]
    pub sub_district: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(max = 255, message = "village must be at most 255 characters"))]
    pub village: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(max = 50, message = "post must be at most 50 characters"))]
    pub post: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(max = 4000, message = "description must be at most 4000 characters"))]
    pub description: Option<String>,

    pub is_foreign: bool,
    pub is_customer_company: bool,
    pub is_customer: bool,
    pub is_admin: bool,
    pub is_employee: bool,
    pub is_freelance: bool,

    /// 1 = active, 0 = inactive. SP rejects anything else.
    #[validate(custom(function = "validate_user_status", message = "status must be 0 or 1"))]
    pub status: i32,

    #[validate(length(min = 3, max = 255, message = "username must be 3-255 characters"))]
    pub username: String,

    /// Plaintext password — the handler hands it to the service
    /// which hashes it before the SP call. Plaintext is never
    /// stored or logged.
    #[validate(length(min = 8, max = 128, message = "password must be 8-128 characters"))]
    pub password: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(max = 500, message = "profile_img_path must be at most 500 characters"))]
    pub profile_img_path: Option<String>,

    /// Optional. Base64-encoded raw image (JPEG / PNG / …) for
    /// the user profile. When present, the handler decodes it,
    /// transcodes to lossy WebP via [`ImageProcessor`], stores
    /// under
    /// `users/{user_guid}/profile/{uuid}.webp`, and writes the
    /// resulting key into `profile_img_path` before the SP
    /// call. Accepts both plain base64 and `data:image/...;base64,...`
    /// prefixes. Decoded size capped at
    /// `KOKKAK_IMAGE__MAX_INPUT_BYTES` (default 1 MiB).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_img_b64: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(max = 36, message = "company_guid must be a 36-char UUID"))]
    pub company_guid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(max = 255, message = "company_name must be at most 255 characters"))]
    pub company_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(max = 50, message = "company_tel must be at most 50 characters"))]
    pub company_tel: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub company_type: Option<i32>,
    pub company_status: i32,

    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(max = 36, message = "department_guid must be a 36-char UUID"))]
    pub department_guid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(max = 36, message = "department_team_guid must be a 36-char UUID"))]
    pub department_team_guid: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(max = 36, message = "position_guid must be a 36-char UUID"))]
    pub position_guid: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub position_start_at: Option<DateTime<Utc>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub salary_amount: Option<Decimal>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(max = 10, message = "salary_currency must be at most 10 characters"))]
    pub salary_currency: Option<String>,

    #[validate(nested)]
    pub schedule: WeeklyScheduleDto,

    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(max = 255, message = "bank_name must be at most 255 characters"))]
    pub bank_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(max = 50, message = "bank_code must be at most 50 characters"))]
    pub bank_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(max = 100, message = "bank_account_no must be at most 100 characters"))]
    pub bank_account_no: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(
        max = 255,
        message = "bank_account_name must be at most 255 characters"
    ))]
    pub bank_account_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(
        max = 500,
        message = "bank_book_img_path must be at most 500 characters"
    ))]
    pub bank_book_img_path: Option<String>,

    /// Optional base64-encoded bank-book cover image. See
    /// `profile_img_b64` for the contract; the storage path
    /// becomes `users/{user_guid}/bank-book/{uuid}.webp`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bank_book_img_b64: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(
        max = 500,
        message = "id_card_front_path must be at most 500 characters"
    ))]
    pub id_card_front_path: Option<String>,
    /// Optional base64 ID-card front image. See `profile_img_b64`
    /// for the contract; storage path becomes
    /// `users/{user_guid}/attachments/id-card-front/{uuid}.webp`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id_card_front_b64: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(
        max = 500,
        message = "id_card_back_path must be at most 500 characters"
    ))]
    pub id_card_back_path: Option<String>,
    /// Optional base64 ID-card back image. See `profile_img_b64`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id_card_back_b64: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(
        max = 500,
        message = "proof_of_address_path must be at most 500 characters"
    ))]
    pub proof_of_address_path: Option<String>,
    /// Optional base64 proof-of-address image. See `profile_img_b64`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proof_of_address_b64: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(
        max = 500,
        message = "source_of_funds_statement_path must be at most 500 characters"
    ))]
    pub source_of_funds_statement_path: Option<String>,
    /// Optional base64 source-of-funds-statement image. See
    /// `profile_img_b64`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_of_funds_statement_b64: Option<String>,
}

/// `validator` fn: `status` must be 0 or 1.
fn validate_user_status(s: i32) -> Result<(), ValidationError> {
    if s == 0 || s == 1 {
        Ok(())
    } else {
        Err(ValidationError::new("invalid_user_status"))
    }
}

/// Response body for `POST /api/v1/admin/users/full`.
///
/// Mirrors the SP's success row verbatim + the actor's
/// `user_username_guid` (the admin can paste it into the
/// "edit login" page for password reset etc.).
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct AdminInsertUserResponse {
    /// Just-created `[user].user_guid`.
    pub user_guid: String,
    /// Just-created `[user_username].user_username_guid`.
    pub user_username_guid: String,
    /// The username (echoed by the SP).
    pub username: String,
    /// `user_role_guid` that the SP assigned (ADMIN / EMPLOYEE /
    /// `None` when neither flag was set).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assigned_role_guid: Option<String>,
}

impl From<AdminInsertUserResult> for AdminInsertUserResponse {
    fn from(r: AdminInsertUserResult) -> Self {
        Self {
            user_guid: r.user_guid,
            user_username_guid: r.user_username_guid,
            username: r.username,
            assigned_role_guid: r.assigned_role_guid,
        }
    }
}

/// `POST /api/v1/admin/users/full` — rich admin user creation.
///
/// Wraps `dbo.SP_USER_INSERT_FULL`. The handler does NOT
/// pre-validate uniqueness or duplicate checks — those live
/// inside the SP (the DB is the source of truth). The
/// handler's job is:
///
/// 1. RBAC gate (already covered by `admin_flag` middleware,
///    but we double-check here for symmetry with the rest of
///    the admin endpoints + to emit the localized
///    `admin_required` message).
/// 2. Validate the wire DTO (`validator` crate).
/// 3. Map DTO → application input.
/// 4. Delegate to [`AdminUserService::admin_insert_full`]
///    which hashes the password + runs the SP.
/// 5. Map SP failure codes → HTTP status + `error.code` via
///    [`sp_insert_full_status`].
///
/// ponytail: validation is layered — `validator` handles
/// length / required-field checks, the service short-circuits
/// working-schedule checks, the SP handles all business rules
/// (uniqueness, role lookups, etc.). Each layer covers gaps
/// the other can't see cleanly.
#[utoipa::path(
    post,
    path = "/api/v1/admin/users/full",
    tag = "admin",
    request_body = AdminInsertUserRequest,
    responses(
        (status = 201, description = "User created (admin-provisioned)", body = AdminInsertUserResponse),
        (status = 400, description = "Malformed JSON body or ACTOR_REQUIRED", body = crate::openapi::ApiError),
        (status = 401, description = "Not authenticated or ACTOR_NOT_FOUND", body = crate::openapi::ApiError),
        (status = 403, description = "Not an admin (PERMISSION_DENIED)", body = crate::openapi::ApiError),
        (status = 409, description = "Username / email / id_card / user_guid collision", body = crate::openapi::ApiError),
        (status = 422, description = "Validation error (required field, role/position/company not found)", body = crate::openapi::ApiError),
        (status = 500, description = "Internal server error (ADMIN/EMPLOYEE role missing)", body = crate::openapi::ApiError),
    ),
    security(("bearer_auth" = []))
)]
pub async fn admin_insert_user_full(
    State(state): State<AppState>,
    user: AuthnUser,
    ValidatedJson(req): ValidatedJson<AdminInsertUserRequest>,
) -> Result<Response, Response> {
    // 1. RBAC — Admin or SuperAdmin only. The middleware also
    //    gates this, but we re-check here so the localized
    //    `admin_required` message surfaces even if the
    //    middleware config changes.
    if !user.has_role(Role::Admin) && !user.has_role(Role::SuperAdmin) {
        let locale = current_locale();
        let localized = tr("err_auth.admin_required", &locale, &[]);
        return Err(
            ApiError::from(AppError::AdminRequired.with_message(localized)).into_response(),
        );
    }

    // 2. DTO → application input. The `schedule` is the only
    //    nested object — flatten it into 7 `DaySchedule` structs
    //    before the call so the service / repo layers stay flat.
    let schedule = kokkak_domain::admin_user::WeeklySchedule {
        monday: dto_to_day_schedule(&req.schedule.monday),
        tuesday: dto_to_day_schedule(&req.schedule.tuesday),
        wednesday: dto_to_day_schedule(&req.schedule.wednesday),
        thursday: dto_to_day_schedule(&req.schedule.thursday),
        friday: dto_to_day_schedule(&req.schedule.friday),
        saturday: dto_to_day_schedule(&req.schedule.saturday),
        sunday: dto_to_day_schedule(&req.schedule.sunday),
    };

    // 3. M9 / T-16 extra: image upload via base64 in JSON.
    //
    //    The caller may send raw image bytes inline (no API
    //    endpoint, per project rules). For each `*_img_b64`
    //    field present, we:
    //      a. resolve the storage user_guid (caller-provided,
    //         or a freshly-minted UUID v7 we pass to the SP),
    //      b. decode the base64 (strip `data:...;base64,`
    //         prefix, drop whitespace),
    //      c. call `state.image.process_and_store(...)` which
    //         decodes, transcodes to WebP, and stores via the
    //         Storage port,
    //      d. write the resulting `StorageKey` into the
    //         matching `*_img_path` field.
    //
    //    ponytail: we serialise the user_guid resolution +
    //    base64 decode + processor call into one async
    //    block (sequential) rather than `tokio::try_join!`
    //    because (1) each call's user_guid is the same, (2)
    //    the processor holds an `Arc<dyn Storage>` already
    //    guarded by the Storage adapter, so concurrent calls
    //    buy nothing, and (3) the simpler code path is easier
    //    to reason about when a single failure should
    //    surface as 422 instead of one of N parallel failures.
    //
    //    Failure mode: any decode / store error is mapped to
    //    a 422 `validation` response with a localized
    //    message — callers see which field failed.
    let user_guid = match req.user_guid.as_deref() {
        Some(g) => g.to_string(),
        None => Uuid::now_v7().to_string(),
    };

    let profile_img_path = match resolve_b64_image(
        state.image.clone(),
        &user_guid,
        req.profile_img_b64.as_deref(),
        UserImageKind::Profile,
    )
    .await
    {
        Ok(v) => v.or(req.profile_img_path),
        Err(e) => return Err(image_error_envelope(&state, "profile_img_b64", e)),
    };
    let bank_book_img_path = match resolve_b64_image(
        state.image.clone(),
        &user_guid,
        req.bank_book_img_b64.as_deref(),
        UserImageKind::BankBook,
    )
    .await
    {
        Ok(v) => v.or(req.bank_book_img_path),
        Err(e) => return Err(image_error_envelope(&state, "bank_book_img_b64", e)),
    };
    let id_card_front_path = match resolve_b64_image(
        state.image.clone(),
        &user_guid,
        req.id_card_front_b64.as_deref(),
        UserImageKind::Attachment(kokkak_infra::storage::UserAttachment::IdCardFront),
    )
    .await
    {
        Ok(v) => v.or(req.id_card_front_path),
        Err(e) => return Err(image_error_envelope(&state, "id_card_front_b64", e)),
    };
    let id_card_back_path = match resolve_b64_image(
        state.image.clone(),
        &user_guid,
        req.id_card_back_b64.as_deref(),
        UserImageKind::Attachment(kokkak_infra::storage::UserAttachment::IdCardBack),
    )
    .await
    {
        Ok(v) => v.or(req.id_card_back_path),
        Err(e) => return Err(image_error_envelope(&state, "id_card_back_b64", e)),
    };
    let proof_of_address_path = match resolve_b64_image(
        state.image.clone(),
        &user_guid,
        req.proof_of_address_b64.as_deref(),
        UserImageKind::Attachment(kokkak_infra::storage::UserAttachment::ProofOfAddress),
    )
    .await
    {
        Ok(v) => v.or(req.proof_of_address_path),
        Err(e) => return Err(image_error_envelope(&state, "proof_of_address_b64", e)),
    };
    let source_of_funds_statement_path = match resolve_b64_image(
        state.image.clone(),
        &user_guid,
        req.source_of_funds_statement_b64.as_deref(),
        UserImageKind::Attachment(kokkak_infra::storage::UserAttachment::SourceOfFunds),
    )
    .await
    {
        Ok(v) => v.or(req.source_of_funds_statement_path),
        Err(e) => {
            return Err(image_error_envelope(
                &state,
                "source_of_funds_statement_b64",
                e,
            ));
        }
    };

    let input = AdminInsertUserFullInput {
        user_guid: Some(user_guid),
        first_name: req.first_name,
        last_name: req.last_name,
        id_card: req.id_card,
        tel: req.tel,
        email: req.email,
        gender: req.gender,
        country_guid: req.country_guid,
        province: req.province,
        district: req.district,
        sub_district: req.sub_district,
        village: req.village,
        post: req.post,
        description: req.description,
        is_foreign: req.is_foreign,
        is_customer_company: req.is_customer_company,
        is_customer: req.is_customer,
        is_admin: req.is_admin,
        is_employee: req.is_employee,
        is_freelance: req.is_freelance,
        status: req.status,
        username: req.username,
        password: req.password,
        profile_img_path,
        company_guid: req.company_guid,
        company_name: req.company_name,
        company_tel: req.company_tel,
        company_type: req.company_type,
        company_status: req.company_status,
        department_guid: req.department_guid,
        department_team_guid: req.department_team_guid,
        position_guid: req.position_guid,
        position_start_at: req.position_start_at,
        salary_amount: req.salary_amount,
        salary_currency: req.salary_currency,
        schedule,
        bank_name: req.bank_name,
        bank_code: req.bank_code,
        bank_account_no: req.bank_account_no,
        bank_account_name: req.bank_account_name,
        bank_book_img_path,
        id_card_front_path,
        id_card_back_path,
        proof_of_address_path,
        source_of_funds_statement_path,
    };

    // 3. Delegate.
    let result = match state.admin_users.admin_insert_full(user.id(), input).await {
        Ok(r) => r,
        Err(e) => return Err(sp_error_envelope(&state, e)),
    };

    // 4. 201 + response DTO.
    Ok((
        StatusCode::CREATED,
        created(AdminInsertUserResponse::from(result)),
    )
        .into_response())
}

/// Convert a wire DTO day into the domain `DaySchedule`.
fn dto_to_day_schedule(d: &DayScheduleDto) -> kokkak_domain::admin_user::DaySchedule {
    kokkak_domain::admin_user::DaySchedule {
        is_working: d.is_working,
        start_time: d.start_time.clone(),
        end_time: d.end_time.clone(),
    }
}

/// Decode (optional) base64 raw image bytes, run them through
/// the [`ImageProcessor`], and return the resulting
/// `StorageKey` as `Some(String)`.
///
/// Returns `Ok(None)` when the input is `None` (caller didn't
/// send a `*_img_b64` for this field). The caller can then
/// fall back to the pre-existing `*_img_path` if it has one.
///
/// `Ok(Some(key))` when the image was successfully transcoded
/// to WebP and stored. The key matches the folder layout in
/// `kokkak_infra::storage::keys`.
async fn resolve_b64_image(
    processor: Arc<kokkak_infra::image_processor::ImageProcessor>,
    user_guid: &str,
    b64: Option<&str>,
    kind: kokkak_infra::image_processor::UserImageKind,
) -> Result<Option<String>, kokkak_infra::image_processor::ImageError> {
    let Some(s) = b64 else { return Ok(None) };
    let bytes =
        decode_base64_payload(s).map_err(kokkak_infra::image_processor::ImageError::Decode)?;
    let result = processor.process_and_store(&bytes, user_guid, kind).await?;
    Ok(Some(result.key.as_str().to_string()))
}

/// Decode a base64 payload, stripping any `data:<mime>;base64,`
/// prefix and whitespace. Errors carry the original
/// [`base64::DecodeError`] stringified.
fn decode_base64_payload(s: &str) -> Result<Vec<u8>, String> {
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine as _;
    let payload = if let Some(idx) = s.find("base64,") {
        &s[idx + "base64,".len()..]
    } else {
        s
    };
    // Drop ASCII whitespace (newlines / spaces typical in
    // line-wrapped base64).
    let cleaned: String = payload.chars().filter(|c| !c.is_whitespace()).collect();
    STANDARD
        .decode(cleaned.as_bytes())
        .map_err(|e| e.to_string())
}

/// Build a 422 `validation` envelope when one of the
/// `*_img_b64` fields fails to decode / transcode / store.
/// The error code is `validation` (matches the rest of the
/// admin DTO contract) and the localized message points at
/// the offending field name.
fn image_error_envelope(
    state: &AppState,
    field: &str,
    err: kokkak_infra::image_processor::ImageError,
) -> Response {
    let locale = current_locale();
    tracing::warn!(
        field = field,
        error = %err,
        "image upload failed"
    );
    let i18n_key = "err_admin_user.image_invalid";
    let localized = tr(i18n_key, &locale, &[field]);
    let _ = state; // reserved for future audit hook
    let envelope: ApiResponse<()> = ApiResponse {
        success: false,
        data: None,
        error: Some(kokkak_common::error::ApiErrorBody {
            code: "validation".into(),
            message: localized,
        }),
        meta: None,
    };
    (StatusCode::UNPROCESSABLE_ENTITY, Json(envelope)).into_response()
}

/// Map a [`AdminInsertUserError`] into an axum [`Response`].
///
/// The SP emits ~25 distinct stable string codes (see the
/// `SP_USER_INSERT_FULL` body). Each one maps to:
///
/// 1. An HTTP status (so the admin UI's HTTP-level error
///    branching works without parsing the body).
/// 2. A stable machine-readable `error.code` string from
///    [`ErrorCode`] (catalog-served via
///    `GET /api/error-codes.json`).
/// 3. A localized message from `err_admin_user.*` (th / en /
///    lo) — admin operators see messages in their UI
///    language, not the SP's English debug string.
///
/// Unknown codes fall back to **500 / internal** with the SP's
/// raw message — surfaces a regression in the SP-side catalog
/// before it ships to admins.
fn sp_error_envelope(state: &AppState, err: AdminInsertUserError) -> Response {
    let (status, code, i18n_key) = sp_insert_full_status(&err.code);
    let locale = current_locale();
    let localized = tr(i18n_key, &locale, &[]);
    // The SP's English `message` is logged at WARN for
    // postmortem visibility — never surfaced to the wire.
    tracing::warn!(
        sp_code = %err.code,
        sp_message = %err.message,
        localized_code = code,
        "SP_USER_INSERT_FULL rejected request"
    );
    let envelope: ApiResponse<()> = ApiResponse {
        success: false,
        data: None,
        error: Some(kokkak_common::error::ApiErrorBody {
            code: code.into(),
            message: localized,
        }),
        meta: None,
    };
    // `state` is currently unused by the helper but kept in
    // the signature for symmetry with the other helpers (room
    // to grow: per-tenant translation overrides, etc.).
    let _ = state;
    (status, Json(envelope)).into_response()
}

/// `(HTTP status, ErrorCode, i18n key)` for every SP code we
/// surface from `SP_USER_INSERT_FULL`.
///
/// ponytail: a 3-column match table is the simplest possible
/// mapping — exhaustive `match` forces the compiler to catch
/// new SP codes at compile time. When the catalog grows past
/// ~30 entries, switch to a `phf` static map keyed by code;
/// right now a match is faster to read and the compile-time
/// exhaustiveness check is the point.
fn sp_insert_full_status(sp_code: &str) -> (StatusCode, &'static str, &'static str) {
    match sp_code {
        // ---- Actor / RBAC ----
        "actor_required" => (
            StatusCode::BAD_REQUEST,
            ErrorCode::ACTOR_REQUIRED,
            "err_admin_user.actor_required",
        ),
        "actor_not_found" => (
            StatusCode::UNAUTHORIZED,
            ErrorCode::ACTOR_NOT_FOUND,
            "err_admin_user.actor_not_found",
        ),
        "permission_denied" => (
            StatusCode::FORBIDDEN,
            ErrorCode::PERMISSION_DENIED,
            "err_admin_user.permission_denied",
        ),

        // ---- Required field validation ----
        "first_name_required" => (
            StatusCode::UNPROCESSABLE_ENTITY,
            ErrorCode::FIRST_NAME_REQUIRED,
            "err_admin_user.first_name_required",
        ),
        "last_name_required" => (
            StatusCode::UNPROCESSABLE_ENTITY,
            ErrorCode::LAST_NAME_REQUIRED,
            "err_admin_user.last_name_required",
        ),
        "email_required" => (
            StatusCode::UNPROCESSABLE_ENTITY,
            ErrorCode::EMAIL_REQUIRED,
            "err_admin_user.email_required",
        ),
        "username_required" => (
            StatusCode::UNPROCESSABLE_ENTITY,
            ErrorCode::USERNAME_REQUIRED,
            "err_admin_user.username_required",
        ),
        "password_hash_required" => (
            StatusCode::UNPROCESSABLE_ENTITY,
            ErrorCode::PASSWORD_HASH_REQUIRED,
            "err_admin_user.password_hash_required",
        ),
        "invalid_user_status" => (
            StatusCode::UNPROCESSABLE_ENTITY,
            ErrorCode::INVALID_USER_STATUS,
            "err_admin_user.invalid_user_status",
        ),

        // ---- Uniqueness conflicts ----
        "user_guid_exists" => (
            StatusCode::CONFLICT,
            ErrorCode::USER_GUID_EXISTS,
            "err_admin_user.user_guid_exists",
        ),
        "username_exists" => (
            StatusCode::CONFLICT,
            ErrorCode::USERNAME_TAKEN,
            "err_admin_user.username_exists",
        ),
        "email_exists" => (
            StatusCode::CONFLICT,
            ErrorCode::EMAIL_TAKEN,
            "err_admin_user.email_exists",
        ),
        "id_card_exists" => (
            StatusCode::CONFLICT,
            ErrorCode::ID_CARD_TAKEN,
            "err_admin_user.id_card_exists",
        ),

        // ---- Reference-data validation ----
        "country_required" => (
            StatusCode::UNPROCESSABLE_ENTITY,
            ErrorCode::COUNTRY_REQUIRED,
            "err_admin_user.country_required",
        ),
        "country_not_found" => (
            StatusCode::UNPROCESSABLE_ENTITY,
            ErrorCode::COUNTRY_NOT_FOUND,
            "err_admin_user.country_not_found",
        ),
        "company_required" => (
            StatusCode::UNPROCESSABLE_ENTITY,
            ErrorCode::COMPANY_REQUIRED,
            "err_admin_user.company_required",
        ),
        "company_not_found" => (
            StatusCode::UNPROCESSABLE_ENTITY,
            ErrorCode::COMPANY_NOT_FOUND,
            "err_admin_user.company_not_found",
        ),
        "department_not_found" => (
            StatusCode::UNPROCESSABLE_ENTITY,
            ErrorCode::DEPARTMENT_NOT_FOUND,
            "err_admin_user.department_not_found",
        ),
        "department_team_not_found" => (
            StatusCode::UNPROCESSABLE_ENTITY,
            ErrorCode::DEPARTMENT_TEAM_NOT_FOUND,
            "err_admin_user.department_team_not_found",
        ),
        "department_team_mismatch" => (
            StatusCode::UNPROCESSABLE_ENTITY,
            ErrorCode::DEPARTMENT_TEAM_MISMATCH,
            "err_admin_user.department_team_mismatch",
        ),
        "position_not_found" => (
            StatusCode::UNPROCESSABLE_ENTITY,
            ErrorCode::POSITION_NOT_FOUND,
            "err_admin_user.position_not_found",
        ),
        "invalid_salary" => (
            StatusCode::UNPROCESSABLE_ENTITY,
            ErrorCode::INVALID_SALARY,
            "err_admin_user.invalid_salary",
        ),
        "work_time_required" => (
            StatusCode::UNPROCESSABLE_ENTITY,
            ErrorCode::WORK_TIME_REQUIRED,
            "err_admin_user.work_time_required",
        ),

        // ---- Server-side configuration ----
        "admin_role_not_found" => (
            StatusCode::INTERNAL_SERVER_ERROR,
            ErrorCode::ADMIN_ROLE_NOT_FOUND,
            "err_admin_user.admin_role_not_found",
        ),
        "employee_role_not_found" => (
            StatusCode::INTERNAL_SERVER_ERROR,
            ErrorCode::EMPLOYEE_ROLE_NOT_FOUND,
            "err_admin_user.employee_role_not_found",
        ),

        // ---- Unknown / driver error ----
        _ => (
            StatusCode::INTERNAL_SERVER_ERROR,
            ErrorCode::INTERNAL,
            "err.internal",
        ),
    }
}

// Silence dead-code warning: `PasswordHasherPort` is imported
// here so a future self-contained handler can re-use the
// hasher without reaching back through `state.admin_users`.
#[allow(dead_code)]
fn _hasher_anchor(_: &dyn PasswordHasherPort) {}

#[cfg(test)]
mod base64_decode_tests {
    //! Lock the contract of `decode_base64_payload` — the helper
    //! that strips `data:image/...;base64,` prefixes and ASCII
    //! whitespace before base64-decoding. The admin handler
    //! chains it directly into the `ImageProcessor`, so any
    //! regression here silently breaks image uploads.

    use super::decode_base64_payload;

    /// Tiny base64 of `hi` (no prefix, no whitespace).
    const PLAIN: &str = "aGk=";

    #[test]
    fn decodes_plain_payload() {
        let out = decode_base64_payload(PLAIN).unwrap();
        assert_eq!(out, b"hi");
    }

    #[test]
    fn decodes_data_url_prefix() {
        let s = format!("data:image/jpeg;base64,{PLAIN}");
        assert_eq!(decode_base64_payload(&s).unwrap(), b"hi");
    }

    #[test]
    fn drops_ascii_whitespace() {
        // Line-wrapped base64 (common in email-style encoders).
        let s = "data:image/png;base64,aGk=\n";
        assert_eq!(decode_base64_payload(s).unwrap(), b"hi");
        let s = "aG k=";
        assert_eq!(decode_base64_payload(s).unwrap(), b"hi");
    }

    #[test]
    fn rejects_invalid_base64() {
        // Not valid base64.
        assert!(decode_base64_payload("not!base64$").is_err());
    }
}

#[cfg(test)]
mod resolve_b64_image_tests {
    //! Smoke test for the wiring between the admin handler's
    //! base64 input and `ImageProcessor::process_and_store`.
    //! Verifies the generated key lands under the expected
    //! `users/{guid}/<kind>/...webp` folder.

    use super::*;
    use kokkak_infra::image_processor::{ImageProcessor, ImageProcessorConfig};
    use kokkak_infra::storage::MemoryStorage;

    fn proc() -> Arc<ImageProcessor> {
        Arc::new(ImageProcessor::new(
            Arc::new(MemoryStorage::new()),
            ImageProcessorConfig {
                max_input_bytes: 1024 * 1024,
                max_dimension_px: 256,
                webp_quality: 80,
            },
        ))
    }

    /// 1x1 red PNG (smallest possible valid PNG). Defined for
    /// future tests that want to build payloads without going
    /// through base64; the active tests use the base64 form
    /// below.
    #[allow(dead_code)]
    fn tiny_png() -> Vec<u8> {
        vec![
            0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00,
            0x00, 0x90, 0x77, 0x53, 0xde, 0x00, 0x00, 0x00, 0x0c, 0x49, 0x44, 0x41, 0x54, 0x08,
            0xd7, 0x63, 0xf8, 0xcf, 0xc0, 0x00, 0x00, 0x00, 0x03, 0x00, 0x01, 0x5b, 0x6b, 0x4f,
            0xa6, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
        ]
    }

    #[tokio::test]
    async fn none_returns_none() {
        let p = proc();
        let r = resolve_b64_image(
            p,
            "u-1",
            None,
            kokkak_infra::image_processor::UserImageKind::Profile,
        )
        .await
        .unwrap();
        assert!(r.is_none());
    }

    #[tokio::test]
    async fn decodes_and_stores_under_kind_folder() {
        let p = proc();
        // base64 of the 1x1 red PNG above.
        let b64 = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8/5+hHgAHggJ/PchI7wAAAABJRU5ErkJggg==";
        let r = resolve_b64_image(
            p,
            "u-1",
            Some(b64),
            kokkak_infra::image_processor::UserImageKind::Profile,
        )
        .await
        .unwrap();
        let key = r.expect("b64 was Some");
        assert!(key.starts_with("users/u-1/profile/"));
        assert!(key.ends_with(".webp"));
    }

    #[tokio::test]
    async fn attachment_kind_lands_in_subfolder() {
        let p = proc();
        let b64 = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8/5+hHgAHggJ/PchI7wAAAABJRU5ErkJggg==";
        let r = resolve_b64_image(
            p,
            "u-2",
            Some(b64),
            kokkak_infra::image_processor::UserImageKind::Attachment(
                kokkak_infra::storage::UserAttachment::IdCardFront,
            ),
        )
        .await
        .unwrap();
        let key = r.expect("b64 was Some");
        assert!(key.contains("/attachments/id-card-front/"));
    }

    #[tokio::test]
    async fn invalid_base64_surfaces_decode_error() {
        let p = proc();
        let r = resolve_b64_image(
            p,
            "u-3",
            Some("not!base64$"),
            kokkak_infra::image_processor::UserImageKind::Profile,
        )
        .await;
        assert!(matches!(
            r,
            Err(kokkak_infra::image_processor::ImageError::Decode(_))
        ));
    }
}

#[cfg(test)]
mod admin_insert_full_tests {
    //! Unit tests for the SP code → (HTTP, ErrorCode, i18n key)
    //! mapping. Exhaustive coverage — every SP code must have a
    //! mapping so the admin UI can rely on a stable wire
    //! contract.

    use super::*;

    /// Every known SP error code. The list MUST match the codes
    /// `SP_USER_INSERT_FULL` emits (see the SP body). When the
    /// DBA adds a new code, add it here AND to the `match` arm
    /// in `sp_insert_full_status`; the test catches drift in
    /// both directions.
    const KNOWN_SP_CODES: &[&str] = &[
        "actor_required",
        "actor_not_found",
        "permission_denied",
        "first_name_required",
        "last_name_required",
        "email_required",
        "username_required",
        "password_hash_required",
        "invalid_user_status",
        "user_guid_exists",
        "username_exists",
        "email_exists",
        "id_card_exists",
        "country_required",
        "country_not_found",
        "company_required",
        "company_not_found",
        "department_not_found",
        "department_team_not_found",
        "department_team_mismatch",
        "position_not_found",
        "invalid_salary",
        "work_time_required",
        "admin_role_not_found",
        "employee_role_not_found",
    ];

    #[test]
    fn all_known_sp_codes_resolve_to_distinct_statuses() {
        let mut seen = std::collections::HashSet::new();
        for code in KNOWN_SP_CODES {
            let (status, ec, key) = sp_insert_full_status(code);
            // Stable wire contract: every known code maps to a
            // distinct (status, code) pair so the admin UI can
            // branch on it without parsing the body.
            assert!(
                seen.insert((status, ec)),
                "duplicate (status, code) for SP code `{code}`: {status} + {ec}"
            );
            assert!(key.starts_with("err_"), "i18n key must live under err_*");
            assert!(
                key == "err.internal" || key.starts_with("err_admin_user."),
                "i18n key for admin-user codes must start with err_admin_user. (got `{key}`)"
            );
        }
    }

    #[test]
    fn actor_codes_map_to_400_401_403() {
        assert_eq!(
            sp_insert_full_status("actor_required").0,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            sp_insert_full_status("actor_not_found").0,
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            sp_insert_full_status("permission_denied").0,
            StatusCode::FORBIDDEN
        );
    }

    #[test]
    fn conflict_codes_map_to_409() {
        for code in [
            "user_guid_exists",
            "username_exists",
            "email_exists",
            "id_card_exists",
        ] {
            assert_eq!(
                sp_insert_full_status(code).0,
                StatusCode::CONFLICT,
                "{code} should map to 409"
            );
        }
    }

    #[test]
    fn validation_codes_map_to_422() {
        for code in [
            "first_name_required",
            "last_name_required",
            "email_required",
            "username_required",
            "password_hash_required",
            "invalid_user_status",
            "country_required",
            "country_not_found",
            "company_required",
            "company_not_found",
            "department_not_found",
            "department_team_not_found",
            "department_team_mismatch",
            "position_not_found",
            "invalid_salary",
            "work_time_required",
        ] {
            assert_eq!(
                sp_insert_full_status(code).0,
                StatusCode::UNPROCESSABLE_ENTITY,
                "{code} should map to 422"
            );
        }
    }

    #[test]
    fn role_seed_codes_map_to_500() {
        assert_eq!(
            sp_insert_full_status("admin_role_not_found").0,
            StatusCode::INTERNAL_SERVER_ERROR
        );
        assert_eq!(
            sp_insert_full_status("employee_role_not_found").0,
            StatusCode::INTERNAL_SERVER_ERROR
        );
    }

    #[test]
    fn unknown_code_falls_back_to_500_internal() {
        // A future SP code we haven't catalogued yet. The
        // mapping must degrade gracefully — 500 + `internal`
        // — so the admin UI surfaces a "something went wrong"
        // instead of crashing on an unmapped code.
        let (status, ec, key) = sp_insert_full_status("SOME_FUTURE_CODE");
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(ec, ErrorCode::INTERNAL);
        assert_eq!(key, "err.internal");
    }

    #[test]
    fn username_exists_maps_to_existing_username_taken_catalog_code() {
        // The catalog already has `username_taken` (used by the
        // public register endpoint). The admin SP code
        // `username_exists` should reuse the same wire string
        // so mobile clients pattern-match on one code, not two.
        assert_eq!(
            sp_insert_full_status("username_exists").1,
            ErrorCode::USERNAME_TAKEN
        );
    }
}
