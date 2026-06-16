//! Auth HTTP handlers (M2).
//!
//! - POST /api/v1/auth/register
//! - POST /api/v1/auth/login
//! - POST /api/v1/auth/refresh
//! - POST /api/v1/auth/logout  (stateless for now — client drops token)

use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use kokkak_application::auth::{LoginInput, RegisterInput};
use kokkak_common::response::{created, ApiResponse};
use kokkak_domain::Role;
use serde::{Deserialize, Serialize};

use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct RegisterRequest {
    pub email: String,
    pub password: String,
    pub display_name: String,
    /// Optional role. Defaults to `customer`.
    pub role: Option<String>,
    pub locale: Option<String>,
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
        email: req.email,
        password: req.password,
        display_name: req.display_name,
        role,
        locale: req.locale.unwrap_or_else(|| "lo".into()),
    };
    let outcome = state
        .auth
        .register(input)
        .await
        .map_err(auth_error_to_response)?;
    Ok((StatusCode::CREATED, created(AuthResponse::from(outcome))).into_response())
}

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub email: String,
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
        email: req.email,
        password: req.password,
        scope: req.scope.unwrap_or_else(|| "mobile".into()),
    };
    let outcome = state
        .auth
        .login(input)
        .await
        .map_err(auth_error_to_response)?;
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
    let outcome = state
        .auth
        .refresh(&req.refresh_token, &scope)
        .await
        .map_err(auth_error_to_response)?;
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

/// Map `AuthError` to the standard envelope + HTTP status.
fn auth_error_to_response(err: kokkak_domain::AuthError) -> Response {
    use kokkak_domain::AuthError::*;
    let (status, code) = match &err {
        InvalidCredentials => (StatusCode::UNAUTHORIZED, "unauthorized"),
        TokenExpired => (StatusCode::UNAUTHORIZED, "token_expired"),
        InvalidToken(_) => (StatusCode::UNAUTHORIZED, "invalid_token"),
        Forbidden(_) => (StatusCode::FORBIDDEN, "forbidden"),
        EmailTaken => (StatusCode::CONFLICT, "email_taken"),
        Validation(_) => (StatusCode::UNPROCESSABLE_ENTITY, "validation"),
        Backend(_) => (StatusCode::INTERNAL_SERVER_ERROR, "internal"),
    };
    let envelope: ApiResponse<()> = ApiResponse {
        success: false,
        data: None,
        error: Some(kokkak_common::error::ApiErrorBody {
            code: code.into(),
            message: err.to_string(),
        }),
        meta: None,
    };
    (status, Json(envelope)).into_response()
}
