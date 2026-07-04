

use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use kokkak_application::auth::{LoginInput, RegisterInput};
use kokkak_common::response::{created, ApiResponse};
use kokkak_common::{
    error::AppError,
    i18n::{current_locale, set_locale},
};
use kokkak_domain::Role;
use serde::{Deserialize, Serialize};
use validator::Validate;

use crate::error::{ApiError, IntoLocalizedResponse};
use crate::extractors::{ClientIp, ValidatedJson};
use crate::state::AppState;

#[derive(Debug, Deserialize, Validate, utoipa::ToSchema)]
pub struct RegisterRequest {

    #[validate(length(min = 3, max = 64, message = "username must be 3-64 characters"))]
    pub username: String,

    #[validate(length(min = 8, max = 128, message = "password must be 8-128 characters"))]
    pub password: String,

    #[validate(length(min = 1, max = 100, message = "first_name must be 1-100 characters"))]
    pub first_name: String,

    #[validate(length(min = 1, max = 100, message = "last_name must be 1-100 characters"))]
    pub last_name: String,

    #[validate(length(max = 20, message = "role must be 20 characters or fewer"))]
    pub role: Option<String>,

    #[validate(length(max = 8, message = "language must be 8 characters or fewer"))]
    pub language: Option<String>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct AuthResponse {
    pub user: kokkak_domain::PublicUser,
    pub access_token: String,
    pub refresh_token: String,
    pub token_type: &'static str,
    pub access_ttl_secs: i64,
    pub refresh_ttl_secs: i64,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_login_ip: Option<std::net::IpAddr>,
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

            last_login_ip: None,
        }
    }
}

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

    apply_login_language(req.language.as_deref());

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

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum PublicRoleError {

    Restricted(Role),

    Unknown(String),
}

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

    #[validate(length(max = 16, message = "scope must be 16 characters or fewer"))]
    pub scope: Option<String>,

    #[validate(length(max = 8, message = "language must be 8 characters or fewer"))]
    pub language: Option<String>,
}

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

    ClientIp(ip): ClientIp,

    ValidatedJson(req): ValidatedJson<LoginRequest>,
) -> Result<Response, Response> {

    apply_login_language(req.language.as_deref());

    let input = LoginInput {
        username: req.username,
        password: req.password,
        scope: req.scope.unwrap_or_else(|| "mobile".into()),
        ip,
    };
    let outcome = match state.auth.login(input).await {
        Ok(o) => o,
        Err(e) => return Err(ApiError::from(e).into_localized_response(&state).await),
    };

    let mut resp = AuthResponse::from(outcome);
    resp.last_login_ip = ip;
    Ok(ok(resp))
}

fn apply_login_language(language: Option<&str>) {
    if let Some(locale) = parse_login_language(language) {
        set_locale(&locale);
    }
}

fn parse_login_language(language: Option<&str>) -> Option<String> {
    let lang = language?;
    let primary = lang.split('-').next().unwrap_or("").trim().to_lowercase();
    match primary.as_str() {
        "th" | "en" | "lo" | "zh" => Some(primary),
        _ => None,
    }
}

#[derive(Debug, Deserialize, Validate, utoipa::ToSchema)]
pub struct RefreshRequest {

    #[validate(length(min = 20, max = 4096, message = "refresh_token length invalid"))]
    pub refresh_token: String,
    #[validate(length(max = 16, message = "scope must be 16 characters or fewer"))]
    pub scope: Option<String>,

    #[validate(length(max = 8, message = "language must be 8 characters or fewer"))]
    pub language: Option<String>,
}

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

    apply_login_language(req.language.as_deref());

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

#[utoipa::path(
    post,
    path = "/api/v1/auth/logout",
    tag = "auth",
    responses(
        (status = 200, description = "Logged out", body = LogoutResponse),
    )
)]
pub async fn logout() -> Response {

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

    fn valid_register() -> RegisterRequest {
        RegisterRequest {
            username: "alice".into(),
            password: "correct horse battery staple".into(),
            first_name: "Alice".into(),
            last_name: "Doe".into(),
            role: None,
            language: None,
        }
    }

    #[test]
    fn register_request_accepts_minimum_valid_payload() {
        assert!(valid_register().validate().is_ok());
    }

    #[test]
    fn register_request_rejects_short_username() {
        let mut r = valid_register();
        r.username = "ab".into();
        let err = r.validate().unwrap_err().to_string();
        assert!(err.contains("username"), "got: {err}");
    }

    #[test]
    fn register_request_rejects_long_username() {
        let mut r = valid_register();
        r.username = "x".repeat(65);
        assert!(r.validate().is_err());
    }

    #[test]
    fn register_request_rejects_short_password() {
        let mut r = valid_register();
        r.password = "short".into();
        let err = r.validate().unwrap_err().to_string();
        assert!(err.contains("password"), "got: {err}");
    }

    #[test]
    fn register_request_rejects_empty_first_name() {
        let mut r = valid_register();
        r.first_name = String::new();
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
        r.role = Some("x".repeat(21));
        assert!(r.validate().is_err());
    }

    fn valid_login() -> LoginRequest {
        LoginRequest {
            username: "alice".into(),
            password: "any".into(),
            scope: None,
            language: None,
        }
    }

    #[test]
    fn login_request_accepts_short_password_login() {

        assert!(valid_login().validate().is_ok());
    }

    #[test]
    fn login_request_rejects_empty_username() {
        let mut r = valid_login();
        r.username = String::new();
        assert!(r.validate().is_err());
    }

    #[test]
    fn login_request_accepts_language_codes_within_length() {

        for lang in ["th", "en", "lo", "zh", "en-US", "th-TH", "zh-CN"] {
            let mut r = valid_login();
            r.language = Some(lang.into());
            assert!(r.validate().is_ok(), "language={lang} should be valid");
        }
    }

    #[test]
    fn login_request_rejects_oversized_language() {
        let mut r = valid_login();
        r.language = Some("x".repeat(9));
        assert!(r.validate().is_err());
    }

    #[test]
    fn parse_login_language_accepts_supported_codes() {

        assert_eq!(parse_login_language(Some("th")), Some("th".into()));
        assert_eq!(parse_login_language(Some("en")), Some("en".into()));
        assert_eq!(parse_login_language(Some("lo")), Some("lo".into()));
        assert_eq!(parse_login_language(Some("zh")), Some("zh".into()));

        assert_eq!(parse_login_language(Some("en-US")), Some("en".into()));
        assert_eq!(parse_login_language(Some("th-TH")), Some("th".into()));
        assert_eq!(parse_login_language(Some("zh-CN")), Some("zh".into()));

        assert_eq!(parse_login_language(Some("TH")), Some("th".into()));
        assert_eq!(parse_login_language(Some("ZH")), Some("zh".into()));
        assert_eq!(parse_login_language(Some("  en  ")), Some("en".into()));
    }

    #[test]
    fn parse_login_language_rejects_unsupported_codes() {

        assert_eq!(parse_login_language(Some("fr")), None);
        assert_eq!(parse_login_language(Some("de")), None);
        assert_eq!(parse_login_language(Some("ja")), None);
        assert_eq!(parse_login_language(Some("")), None);
        assert_eq!(parse_login_language(None), None);
    }

    fn valid_refresh() -> RefreshRequest {
        RefreshRequest {

            refresh_token: "x".repeat(150),
            scope: None,
            language: None,
        }
    }

    #[test]
    fn refresh_request_accepts_realistic_token() {
        assert!(valid_refresh().validate().is_ok());
    }

    #[test]
    fn refresh_request_rejects_short_token() {
        let mut r = valid_refresh();
        r.refresh_token = "short".into();
        let err = r.validate().unwrap_err().to_string();
        assert!(err.contains("refresh_token"), "got: {err}");
    }
}
