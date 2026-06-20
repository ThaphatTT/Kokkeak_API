//! Admin HTTP handlers (M14.5 register-role split).
//!
//! ponytail: single endpoint right now (`POST /api/v1/admin/users`).
//! Future admin endpoints (list users, suspend, change roles, etc.)
//! live here too — the file is the home for any route that requires
//! `Admin` / `SuperAdmin` privileges.

use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use kokkak_application::auth::RegisterInput;
use kokkak_common::i18n::{current_locale, tr, tr_with_repo};
use kokkak_common::response::{created, ApiResponse};
use kokkak_domain::Role;
use serde::Deserialize;

use crate::handlers::auth::{auth_error_to_response, AuthResponse};
use crate::middleware::auth::AuthnUser;
use crate::state::AppState;

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct CreateUserRequest {
    pub username: String,
    pub password: String,
    pub first_name: String,
    pub last_name: String,
    /// Required for the admin endpoint: must be one of
    /// `customer` / `technician` / `admin` / `super_admin`.
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
    Json(req): Json<CreateUserRequest>,
) -> Result<Response, Response> {
    let locale = current_locale();

    // 1. RBAC: only admins / super_admins may create accounts here.
    if !user.has_role(Role::Admin) && !user.has_role(Role::SuperAdmin) {
        let msg = tr("err_auth.admin_required", &locale, &[]);
        return Err(forbidden("admin_required", msg));
    }

    // 2. Parse the role. Unlike the public register endpoint, all
    //    four roles are accepted here; an unknown string is a 422.
    let role = match Role::from_code(&req.role) {
        Some(r) => r,
        None => {
            let msg = format!(
                "unknown role '{}'; expected customer, technician, admin, or super_admin",
                req.role
            );
            let localized = tr_with_repo(
                &*state.translation,
                &locale,
                "err_auth.validation",
                &[msg.as_str()],
            )
            .await;
            return Err(validation(localized));
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
        Err(e) => return Err(auth_error_to_response(e, &state).await),
    };
    Ok((StatusCode::CREATED, created(AuthResponse::from(outcome))).into_response())
}

fn forbidden(code: &'static str, message: String) -> Response {
    (
        StatusCode::FORBIDDEN,
        Json(ApiResponse::<()> {
            success: false,
            data: None,
            error: Some(kokkak_common::error::ApiErrorBody {
                code: code.into(),
                message,
            }),
            meta: None,
        }),
    )
        .into_response()
}

fn validation(message: String) -> Response {
    (
        StatusCode::UNPROCESSABLE_ENTITY,
        Json(ApiResponse::<()> {
            success: false,
            data: None,
            error: Some(kokkak_common::error::ApiErrorBody {
                code: "validation".into(),
                message,
            }),
            meta: None,
        }),
    )
        .into_response()
}
