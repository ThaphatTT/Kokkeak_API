//! Admin HTTP handlers (M14.5 register-role split + T-06 refactor).
//!
//! ponytail: single endpoint right now (`POST /api/v1/admin/users`).
//! Future admin endpoints (list users, suspend, change roles, etc.)
//! live here too — the file is the home for any route that requires
//! `Admin` / `SuperAdmin` privileges.
//!
//! **T-06**: the bespoke `forbidden` / `validation` envelope
//! helpers were deleted; role + RBAC failures now build an
//! [`ApiError`] and call [`crate::error::IntoLocalizedResponse::into_localized_response`]
//! like every other handler.

use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use kokkak_application::auth::RegisterInput;
use kokkak_common::error::AppError;
use kokkak_common::i18n::{current_locale, tr};
use kokkak_common::response::created;
use kokkak_domain::Role;
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

// Re-import the auth response shape is at the top of the file.
