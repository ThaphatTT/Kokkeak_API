//! Auth HTTP handlers (M2 + M11 i18n + M14 NEW_DB + M14.5 split +
//! T-06 AppError refactor).
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
//! **T-06 refactor**: domain errors (`AuthError`) flow through
//! `ApiError::from(...)` + `IntoLocalizedResponse::into_localized_response(&state)`,
//! replacing the previous per-handler `auth_error_to_response`
//! helper. The result is one line per error site instead of a
//! bespoke function, and the i18n key lookup is centralized in
//! `crate::error::l10n_key_for_app_error`.
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
//!   returns 422 with a localized `err_auth.role_not_allowed` message.
//! - Admin role creation lives at `POST /api/v1/admin/users` and
//!   requires a JWT carrying `Admin` or `SuperAdmin`.

use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use kokkak_application::auth::{LoginInput, RegisterInput};
use kokkak_common::response::{created, ApiResponse};
use kokkak_common::{error::AppError, i18n::current_locale};
use kokkak_domain::Role;
use serde::{Deserialize, Serialize};
use validator::Validate;

use crate::error::{ApiError, IntoLocalizedResponse};
use crate::extractors::ValidatedJson;
use crate::state::AppState;

#[derive(Debug, Deserialize, Validate, utoipa::ToSchema)]
pub struct RegisterRequest {
    /// Login username (lowercased on the server). In practice this can be
    /// an email, phone, or alphanumeric handle.
    #[validate(length(min = 3, max = 64, message = "username must be 3-64 characters"))]
    pub username: String,
    /// Plain-text password. Hashing happens server-side via argon2.
    /// Min length 8 enforced here; full strength policy lives in
    /// `application::auth`.
    #[validate(length(min = 8, max = 128, message = "password must be 8-128 characters"))]
    pub password: String,
    /// First name (`[user].user_first_name`).
    #[validate(length(min = 1, max = 100, message = "first_name must be 1-100 characters"))]
    pub first_name: String,
    /// Last name (`[user].user_last_name`).
    #[validate(length(min = 1, max = 100, message = "last_name must be 1-100 characters"))]
    pub last_name: String,
    /// Optional role. Defaults to `customer`. Validated structurally
    /// here (length) and semantically in `parse_public_register_role`.
    #[validate(length(max = 20, message = "role must be 20 characters or fewer"))]
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
///
/// T-06: handler returns `Result<Response, Response>`; the error
/// arm builds a localized envelope via
/// [`IntoLocalizedResponse::into_localized_response`].
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
    ValidatedJson(req): ValidatedJson<RegisterRequest>,
) -> Result<Response, Response> {
    // M14.5: reject roles the public endpoint isn't allowed to
    // grant. Both cases share the same 422 + role_not_allowed code —
    // the message tells the client which case it is.
    let role = match parse_public_register_role(req.role.as_deref()) {
        Ok(r) => r,
        Err(PublicRoleError::Restricted(other)) => {
            return Err(
                ApiError::from(AppError::RoleNotAllowed(other.as_str().to_string()))
                    .into_localized_response(&state)
                    .await,
            );
        }
        Err(PublicRoleError::Unknown(s)) => {
            return Err(ApiError::from(AppError::RoleNotAllowed(s))
                .into_localized_response(&state)
                .await);
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
        Err(e) => {
            return Err(ApiError::from(e).into_localized_response(&state).await);
        }
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

#[derive(Debug, Deserialize, Validate, utoipa::ToSchema)]
pub struct LoginRequest {
    #[validate(length(min = 1, max = 64, message = "username must be 1-64 characters"))]
    pub username: String,
    #[validate(length(min = 1, max = 128, message = "password must be 1-128 characters"))]
    pub password: String,
    /// Token scope (`mobile` / `web` / `admin`). Defaults to `mobile`.
    #[validate(length(max = 16, message = "scope must be 16 characters or fewer"))]
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
    ValidatedJson(req): ValidatedJson<LoginRequest>,
) -> Result<Response, Response> {
    let input = LoginInput {
        username: req.username,
        password: req.password,
        scope: req.scope.unwrap_or_else(|| "mobile".into()),
    };
    let outcome = match state.auth.login(input).await {
        Ok(o) => o,
        Err(e) => return Err(ApiError::from(e).into_localized_response(&state).await),
    };
    Ok(ok(AuthResponse::from(outcome)))
}

#[derive(Debug, Deserialize, Validate, utoipa::ToSchema)]
pub struct RefreshRequest {
    /// JWT refresh token. Min length 20 — a real HS256 token is
    /// significantly longer; this only catches obvious garbage.
    #[validate(length(min = 20, max = 4096, message = "refresh_token length invalid"))]
    pub refresh_token: String,
    #[validate(length(max = 16, message = "scope must be 16 characters or fewer"))]
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
    ValidatedJson(req): ValidatedJson<RefreshRequest>,
) -> Result<Response, Response> {
    let scope = req.scope.unwrap_or_else(|| "mobile".into());
    let outcome = match state.auth.refresh(&req.refresh_token, &scope).await {
        Ok(o) => o,
        Err(e) => return Err(ApiError::from(e).into_localized_response(&state).await),
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
    // `current_locale` import is preserved above so the locale stack
    // is observable in handler metrics later; the logout response
    // has no user-facing message.
    let _ = current_locale();
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

#[cfg(test)]
mod tests {
    use super::*;
    use validator::Validate;

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

    // ---- T-07: structural validation on request DTOs ----

    fn valid_register() -> RegisterRequest {
        RegisterRequest {
            username: "alice".into(),
            password: "correct horse battery staple".into(),
            first_name: "Alice".into(),
            last_name: "Doe".into(),
            role: None,
        }
    }

    #[test]
    fn register_request_accepts_minimum_valid_payload() {
        assert!(valid_register().validate().is_ok());
    }

    #[test]
    fn register_request_rejects_short_username() {
        let mut r = valid_register();
        r.username = "ab".into(); // min 3
        let err = r.validate().unwrap_err().to_string();
        assert!(err.contains("username"), "got: {err}");
    }

    #[test]
    fn register_request_rejects_long_username() {
        let mut r = valid_register();
        r.username = "x".repeat(65); // max 64
        assert!(r.validate().is_err());
    }

    #[test]
    fn register_request_rejects_short_password() {
        let mut r = valid_register();
        r.password = "short".into(); // min 8
        let err = r.validate().unwrap_err().to_string();
        assert!(err.contains("password"), "got: {err}");
    }

    #[test]
    fn register_request_rejects_empty_first_name() {
        let mut r = valid_register();
        r.first_name = String::new(); // min 1
        assert!(r.validate().is_err());
    }

    #[test]
    fn register_request_rejects_empty_last_name() {
        let mut r = valid_register();
        r.last_name = String::new();
        assert!(r.validate().is_err());
    }

    #[test]
    fn register_request_rejects_oversized_role() {
        let mut r = valid_register();
        r.role = Some("x".repeat(21)); // max 20
        assert!(r.validate().is_err());
    }

    fn valid_login() -> LoginRequest {
        LoginRequest {
            username: "alice".into(),
            password: "any".into(),
            scope: None,
        }
    }

    #[test]
    fn login_request_accepts_short_password_login() {
        // Login does NOT enforce the 8-char floor (that's a
        // registration rule). It only needs something non-empty to
        // attempt a hash compare.
        assert!(valid_login().validate().is_ok());
    }

    #[test]
    fn login_request_rejects_empty_username() {
        let mut r = valid_login();
        r.username = String::new();
        assert!(r.validate().is_err());
    }

    fn valid_refresh() -> RefreshRequest {
        RefreshRequest {
            // A realistic HS256 refresh token is ~250+ bytes; we only
            // check the floor here.
            refresh_token: "x".repeat(150),
            scope: None,
        }
    }

    #[test]
    fn refresh_request_accepts_realistic_token() {
        assert!(valid_refresh().validate().is_ok());
    }

    #[test]
    fn refresh_request_rejects_short_token() {
        let mut r = valid_refresh();
        r.refresh_token = "short".into(); // min 20
        let err = r.validate().unwrap_err().to_string();
        assert!(err.contains("refresh_token"), "got: {err}");
    }
}
