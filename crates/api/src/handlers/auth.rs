//! Auth HTTP handlers (M2 + M11 i18n + M14 NEW_DB + M14.5 split).
//!
//! - POST /api/v1/auth/register   — PUBLIC; role restricted to
//!   `customer` / `technician`. Admin / super_admin must go through
//!   the admin endpoint to prevent open admin registration.
//! - POST /api/v1/auth/login
//! - POST /api/v1/auth/refresh
//! - POST /api/v1/auth/logout  (stateless for now — client drops token)
//!
//! All user-visible error strings are rendered through
//! `kokkak_common::i18n::tr_with_repo` against the per-tenant
//! `TranslationRepository`; the file-based catalog is the
//! fallback. The locale is set per request by
//! `crate::middleware::i18n::locale_middleware`.
//!
//! **M14 changes** (NEW_DB.txt alignment):
//! - `email` → `username` in `RegisterRequest` / `LoginRequest`
//! - `display_name` → `first_name` + `last_name`
//! - `locale` field removed (locale is per-request via header/JWT per M11)
//! - JSON error code `email_taken` → `username_taken`
//!
//! **M14.5 split** (register role security):
//! - Public `/auth/register` accepts only `customer` / `technician`
//!   (default `customer`). Anything else (admin/super_admin/unknown)
//!   returns 422 with a localized `err_auth.validation` message.
//! - Admin role creation lives at `POST /api/v1/admin/users` and
//!   requires a JWT carrying `Admin` or `SuperAdmin`.

use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use kokkak_application::auth::{LoginInput, RegisterInput};
use kokkak_common::i18n::{current_locale, tr_with_repo};
use kokkak_common::response::{created, ApiResponse};
use kokkak_domain::{AuthError, LocalizedError, Role};
use serde::{Deserialize, Serialize};

use crate::state::AppState;

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct RegisterRequest {
    /// Login username (lowercased on the server). In practice this can be
    /// an email, phone, or alphanumeric handle.
    pub username: String,
    pub password: String,
    /// First name (`[user].user_first_name`).
    pub first_name: String,
    /// Last name (`[user].user_last_name`).
    pub last_name: String,
    /// Optional role. Defaults to `customer`.
    pub role: Option<String>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct AuthResponse {
    pub user: kokkak_domain::PublicUser,
    pub access_token: String,
    pub refresh_token: String,
    pub token_type: &'static str,
    pub access_ttl_secs: i64,
    pub refresh_ttl_secs: i64,
}

impl From<kokkak_application::auth::AuthOutcome> for AuthResponse {
    fn from(o: kokkak_application::auth::AuthOutcome) -> Self {
        Self {
            user: o.user,
            access_token: o.tokens.access_token,
            refresh_token: o.tokens.refresh_token,
            token_type: o.tokens.token_type,
            access_ttl_secs: o.tokens.access_ttl_secs,
            refresh_ttl_secs: o.tokens.refresh_ttl_secs,
        }
    }
}

/// POST /api/v1/auth/register
///
/// M14.5 split: public registration is restricted to `customer` /
/// `technician`. Admin and super_admin accounts must be created via
/// `POST /api/v1/admin/users` (which requires an admin JWT). The
/// previous behaviour — accepting `{"role":"admin"}` from an
/// unauthenticated client — was an open admin registration
/// vulnerability.
#[utoipa::path(
    post,
    path = "/api/v1/auth/register",
    tag = "auth",
    request_body = RegisterRequest,
    responses(
        (status = 201, description = "Account created", body = AuthResponse),
        (status = 400, description = "Idempotency-Key required", body = crate::openapi::ApiError),
        (status = 409, description = "Username already taken", body = crate::openapi::ApiError),
        (status = 422, description = "Role not allowed (admin/super_admin must use admin endpoint)", body = crate::openapi::ApiError),
        (status = 500, description = "Internal error", body = crate::openapi::ApiError),
    ),
    params(
        ("Idempotency-Key" = String, Header, description = "Unique per-request token. Mobile retries MUST send the same key to dedupe the registration."),
    )
)]
pub async fn register(
    State(state): State<AppState>,
    Json(req): Json<RegisterRequest>,
) -> Result<Response, Response> {
    let role = match parse_public_register_role(req.role.as_deref()) {
        Ok(r) => r,
        Err(PublicRoleError::Restricted(other)) => {
            let msg = format!(
                "role '{}' is restricted to admin endpoint; public registration accepts only customer/technician",
                other.as_str()
            );
            return Err(auth_error_to_response(AuthError::Validation(msg), &state).await);
        }
        Err(PublicRoleError::Unknown(s)) => {
            let msg = format!("unknown role '{s}'; expected customer or technician");
            return Err(auth_error_to_response(AuthError::Validation(msg), &state).await);
        }
    };
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

/// Role-parsing outcome for the public register endpoint.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum PublicRoleError {
    /// The role is well-formed but not allowed for public
    /// registration (admin / super_admin).
    Restricted(Role),
    /// The string did not match any known role.
    Unknown(String),
}

/// Parse the optional `role` field from the public register
/// endpoint. Defaults to `Customer` when omitted; accepts
/// `Customer` / `Technician`; rejects `Admin` / `SuperAdmin` and
/// unknown strings.
///
/// ponytail: pure function (no async, no IO) so the role-allowlist
/// can be unit-tested without spinning up the full AppState /
/// Router. Ceiling: if more public roles land (e.g. "staff"), add
/// them to the allowlist here, not in the handler.
pub(crate) fn parse_public_register_role(raw: Option<&str>) -> Result<Role, PublicRoleError> {
    match raw {
        None => Ok(Role::Customer),
        Some(s) => match Role::from_code(s) {
            Some(Role::Customer) => Ok(Role::Customer),
            Some(Role::Technician) => Ok(Role::Technician),
            Some(other) => Err(PublicRoleError::Restricted(other)),
            None => Err(PublicRoleError::Unknown(s.to_string())),
        },
    }
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
    /// Token scope (`mobile` / `web` / `admin`). Defaults to `mobile`.
    pub scope: Option<String>,
}

/// POST /api/v1/auth/login
#[utoipa::path(
    post,
    path = "/api/v1/auth/login",
    tag = "auth",
    request_body = LoginRequest,
    responses(
        (status = 200, description = "Logged in", body = AuthResponse),
        (status = 401, description = "Invalid credentials", body = crate::openapi::ApiError),
        (status = 500, description = "Internal error", body = crate::openapi::ApiError),
    )
)]
pub async fn login(
    State(state): State<AppState>,
    Json(req): Json<LoginRequest>,
) -> Result<Response, Response> {
    let input = LoginInput {
        username: req.username,
        password: req.password,
        scope: req.scope.unwrap_or_else(|| "mobile".into()),
    };
    let outcome = match state.auth.login(input).await {
        Ok(o) => o,
        Err(e) => return Err(auth_error_to_response(e, &state).await),
    };
    Ok(ok(AuthResponse::from(outcome)))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct RefreshRequest {
    pub refresh_token: String,
    pub scope: Option<String>,
}

/// POST /api/v1/auth/refresh
#[utoipa::path(
    post,
    path = "/api/v1/auth/refresh",
    tag = "auth",
    request_body = RefreshRequest,
    responses(
        (status = 200, description = "Token refreshed", body = AuthResponse),
        (status = 401, description = "Refresh token invalid or expired", body = crate::openapi::ApiError),
        (status = 500, description = "Internal error", body = crate::openapi::ApiError),
    )
)]
pub async fn refresh(
    State(state): State<AppState>,
    Json(req): Json<RefreshRequest>,
) -> Result<Response, Response> {
    let scope = req.scope.unwrap_or_else(|| "mobile".into());
    let outcome = match state.auth.refresh(&req.refresh_token, &scope).await {
        Ok(o) => o,
        Err(e) => return Err(auth_error_to_response(e, &state).await),
    };
    Ok(ok(AuthResponse::from(outcome)))
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct LogoutResponse {
    pub logged_out: bool,
}

/// POST /api/v1/auth/logout
///
/// Stateless for now: the client discards the token. When Redis
/// refresh-token blacklist lands, this endpoint will add the
/// refresh-token jti to a TTL'd set.
#[utoipa::path(
    post,
    path = "/api/v1/auth/logout",
    tag = "auth",
    responses(
        (status = 200, description = "Logged out", body = LogoutResponse),
    )
)]
pub async fn logout() -> Response {
    (
        StatusCode::OK,
        Json(ApiResponse {
            success: true,
            data: Some(LogoutResponse { logged_out: true }),
            error: None,
            meta: None,
        }),
    )
        .into_response()
}

fn ok<T: Serialize>(data: T) -> Response {
    (
        StatusCode::OK,
        Json(ApiResponse {
            success: true,
            data: Some(data),
            error: None,
            meta: None,
        }),
    )
        .into_response()
}

/// Build a localized error response for an `AuthError` variant.
/// Resolves the user-visible message via
/// [`kokkak_common::i18n::tr_with_repo`] against the
/// per-tenant `TranslationRepository` and falls through to the
/// file-based catalog when no override is set.
pub async fn auth_error_to_response(err: AuthError, state: &AppState) -> Response {
    use AuthError::*;
    let (status, code) = match &err {
        InvalidCredentials => (StatusCode::UNAUTHORIZED, "unauthorized"),
        TokenExpired => (StatusCode::UNAUTHORIZED, "token_expired"),
        InvalidToken(_) => (StatusCode::UNAUTHORIZED, "invalid_token"),
        Forbidden(_) => (StatusCode::FORBIDDEN, "forbidden"),
        // M14: renamed to match NEW_DB's username-based login.
        UsernameTaken => (StatusCode::CONFLICT, "username_taken"),
        Validation(_) => (StatusCode::UNPROCESSABLE_ENTITY, "validation"),
        Backend(_) => (StatusCode::INTERNAL_SERVER_ERROR, "internal"),
    };
    let locale = current_locale();
    let args: Vec<String> = err.l10n_args();
    let args_ref: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let message = tr_with_repo(&*state.translation, &locale, err.l10n_key(), &args_ref).await;
    envelope(status, code, message)
}

/// Build the standard error envelope.
fn envelope(status: StatusCode, code: &str, message: String) -> Response {
    let envelope: ApiResponse<()> = ApiResponse {
        success: false,
        data: None,
        error: Some(kokkak_common::error::ApiErrorBody {
            code: code.into(),
            message,
        }),
        meta: None,
    };
    (status, Json(envelope)).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_public_register_role_defaults_to_customer() {
        assert_eq!(
            parse_public_register_role(None),
            Ok(kokkak_domain::Role::Customer)
        );
    }

    #[test]
    fn parse_public_register_role_accepts_customer() {
        assert_eq!(
            parse_public_register_role(Some("customer")),
            Ok(kokkak_domain::Role::Customer)
        );
    }

    #[test]
    fn parse_public_register_role_accepts_technician() {
        assert_eq!(
            parse_public_register_role(Some("technician")),
            Ok(kokkak_domain::Role::Technician)
        );
    }

    #[test]
    fn parse_public_register_role_rejects_admin() {
        assert_eq!(
            parse_public_register_role(Some("admin")),
            Err(PublicRoleError::Restricted(kokkak_domain::Role::Admin))
        );
    }

    #[test]
    fn parse_public_register_role_rejects_super_admin() {
        assert_eq!(
            parse_public_register_role(Some("super_admin")),
            Err(PublicRoleError::Restricted(kokkak_domain::Role::SuperAdmin))
        );
    }

    #[test]
    fn parse_public_register_role_rejects_unknown() {
        assert_eq!(
            parse_public_register_role(Some("wizard")),
            Err(PublicRoleError::Unknown("wizard".into()))
        );
    }
}
