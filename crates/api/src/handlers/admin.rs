//! Admin HTTP handlers (M14.5 register-role split + T-06 refactor).
//!
//! - `POST /api/v1/admin/users` — admin-only user creation (M14.5).
//! - `GET /api/v1/admin/permissions?mode=<literal>` — role × permission
//!   matrix (M15-prep). The `mode` is a pass-through literal the SP
//!   uses to scope which role set to return (e.g. `SELECT_ADMIN`,
//!   `SELECT_EMPLOYEE`); the service does not validate it.
//!
//! **T-06**: the bespoke `forbidden` / `validation` envelope
//! helpers were deleted; role + RBAC failures now build an
//! [`ApiError`] and call [`crate::error::IntoLocalizedResponse::into_localized_response`]
//! like every other handler.

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use kokkak_application::auth::RegisterInput;
use kokkak_common::error::AppError;
use kokkak_common::error_codes::ErrorCode;
use kokkak_common::i18n::{current_locale, tr};
use kokkak_common::response::{created, ApiResponse};
use kokkak_domain::{Role, UserRoleWithPermissions};
use serde::Deserialize;
use validator::Validate;

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
    let groups: Vec<UserRoleWithPermissions> = match state.user_roles.list_permissions(mode).await {
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
