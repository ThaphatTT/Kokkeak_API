//! Auth HTTP handlers (M2 + M11 i18n + M14 NEW_DB).
//!
//! - POST /api/v1/auth/register
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

#[derive(Debug, Deserialize)]
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

#[derive(Debug, Serialize)]
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
pub async fn register(
    State(state): State<AppState>,
    Json(req): Json<RegisterRequest>,
) -> Result<Response, Response> {
    let role = match req.role.as_deref() {
        Some("technician") => Role::Technician,
        Some("admin") => Role::Admin,
        Some("super_admin") => Role::SuperAdmin,
        _ => Role::Customer,
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

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
    /// Token scope (`mobile` / `web` / `admin`). Defaults to `mobile`.
    pub scope: Option<String>,
}

/// POST /api/v1/auth/login
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

#[derive(Debug, Deserialize)]
pub struct RefreshRequest {
    pub refresh_token: String,
    pub scope: Option<String>,
}

/// POST /api/v1/auth/refresh
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

#[derive(Debug, Serialize)]
pub struct LogoutResponse {
    pub logged_out: bool,
}

/// POST /api/v1/auth/logout
///
/// Stateless for now: the client discards the token. When Redis
/// refresh-token blacklist lands, this endpoint will add the
/// refresh-token jti to a TTL'd set.
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
