

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;
use thiserror::Error;

use crate::response::ApiResponse;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum AppError {

    #[error("bad request: {0}")]
    BadRequest(String),

    #[error("unauthorized")]
    Unauthorized,

    #[error("invalid token: {0}")]
    InvalidToken(String),

    #[error("token expired")]
    TokenExpired,

    #[error("forbidden: {0}")]
    Forbidden(String),

    #[error("admin role required")]
    AdminRequired,

    #[error("not found: {0}")]
    NotFound(String),

    #[error("conflict: {0}")]
    Conflict(String),

    #[error("username already taken")]
    UsernameTaken,

    #[error("validation: {0}")]
    Validation(String),

    #[error("role not allowed: {0}")]
    RoleNotAllowed(String),

    #[error("rate limited")]
    RateLimited {

        retry_after_secs: u64,
    },

    #[error("internal: {0}")]
    Internal(String),

    #[error("localized error ({code}): {message}")]
    Localized {

        status: StatusCode,

        code: &'static str,

        message: String,
    },
}

impl AppError {

    pub fn status(&self) -> StatusCode {
        match self {
            Self::BadRequest(_) => StatusCode::BAD_REQUEST,
            Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::InvalidToken(_) => StatusCode::UNAUTHORIZED,
            Self::TokenExpired => StatusCode::UNAUTHORIZED,
            Self::Forbidden(_) => StatusCode::FORBIDDEN,
            Self::AdminRequired => StatusCode::FORBIDDEN,
            Self::NotFound(_) => StatusCode::NOT_FOUND,
            Self::Conflict(_) => StatusCode::CONFLICT,
            Self::UsernameTaken => StatusCode::CONFLICT,
            Self::Validation(_) => StatusCode::UNPROCESSABLE_ENTITY,
            Self::RoleNotAllowed(_) => StatusCode::UNPROCESSABLE_ENTITY,
            Self::RateLimited { .. } => StatusCode::TOO_MANY_REQUESTS,
            Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Self::Localized { status, .. } => *status,
        }
    }

    pub fn code(&self) -> &'static str {
        match self {
            Self::BadRequest(_) => "bad_request",
            Self::Unauthorized => "unauthorized",
            Self::InvalidToken(_) => "invalid_token",
            Self::TokenExpired => "token_expired",
            Self::Forbidden(_) => "forbidden",
            Self::AdminRequired => "admin_required",
            Self::NotFound(_) => "not_found",
            Self::Conflict(_) => "conflict",
            Self::UsernameTaken => "username_taken",
            Self::Validation(_) => "validation",
            Self::RoleNotAllowed(_) => "role_not_allowed",
            Self::RateLimited { .. } => "rate_limited",
            Self::Internal(_) => "internal",
            Self::Localized { code, .. } => code,
        }
    }

    pub fn body(&self) -> ApiErrorBody {
        match self {
            Self::Localized { code, message, .. } => ApiErrorBody {
                code: (*code).to_string(),
                message: message.clone(),
            },
            other => ApiErrorBody {
                code: other.code().to_string(),
                message: other.to_string(),
            },
        }
    }

    pub fn with_message(self, message: String) -> Self {
        Self::Localized {
            status: self.status(),
            code: self.code(),
            message,
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let envelope: ApiResponse<()> = ApiResponse {
            success: false,
            data: None,
            error: Some(self.body()),
            meta: None,
        };
        let status = self.status();
        let mut resp = (status, Json(envelope)).into_response();

        if status == StatusCode::UNAUTHORIZED {
            resp.headers_mut().insert(
                axum::http::header::WWW_AUTHENTICATE,
                axum::http::HeaderValue::from_static("Bearer realm=\"kokkeak\""),
            );
        }

        if let Self::RateLimited { retry_after_secs } = &self {

            if let Ok(v) = axum::http::HeaderValue::from_str(&retry_after_secs.to_string()) {
                resp.headers_mut()
                    .insert(axum::http::header::RETRY_AFTER, v);
            }
        }

        resp
    }
}

#[derive(Debug, Serialize, Clone)]
pub struct ApiErrorBody {

    pub code: String,

    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_codes_per_variant() {
        assert_eq!(AppError::BadRequest("x".into()).status(), 400);
        assert_eq!(AppError::Unauthorized.status(), 401);
        assert_eq!(AppError::InvalidToken("x".into()).status(), 401);
        assert_eq!(AppError::TokenExpired.status(), 401);
        assert_eq!(AppError::Forbidden("x".into()).status(), 403);
        assert_eq!(AppError::AdminRequired.status(), 403);
        assert_eq!(AppError::NotFound("x".into()).status(), 404);
        assert_eq!(AppError::Conflict("x".into()).status(), 409);
        assert_eq!(AppError::UsernameTaken.status(), 409);
        assert_eq!(AppError::Validation("x".into()).status(), 422);
        assert_eq!(AppError::RoleNotAllowed("x".into()).status(), 422);
        assert_eq!(
            AppError::RateLimited {
                retry_after_secs: 60
            }
            .status(),
            429
        );
        assert_eq!(AppError::Internal("x".into()).status(), 500);
    }

    #[test]
    fn codes_are_snake_case() {
        assert_eq!(AppError::BadRequest("x".into()).code(), "bad_request");
        assert_eq!(AppError::Unauthorized.code(), "unauthorized");
        assert_eq!(AppError::InvalidToken("x".into()).code(), "invalid_token");
        assert_eq!(AppError::TokenExpired.code(), "token_expired");
        assert_eq!(AppError::Forbidden("x".into()).code(), "forbidden");
        assert_eq!(AppError::AdminRequired.code(), "admin_required");
        assert_eq!(AppError::NotFound("x".into()).code(), "not_found");
        assert_eq!(AppError::Conflict("x".into()).code(), "conflict");
        assert_eq!(AppError::UsernameTaken.code(), "username_taken");
        assert_eq!(AppError::Validation("x".into()).code(), "validation");
        assert_eq!(
            AppError::RoleNotAllowed("x".into()).code(),
            "role_not_allowed"
        );
        assert_eq!(
            AppError::RateLimited {
                retry_after_secs: 60
            }
            .code(),
            "rate_limited"
        );
        assert_eq!(AppError::Internal("x".into()).code(), "internal");
    }

    #[test]
    fn codes_match_error_code_catalog() {

        let pairs: &[(AppError, &str)] = &[
            (
                AppError::BadRequest("x".into()),
                crate::error_codes::ErrorCode::BAD_REQUEST,
            ),
            (
                AppError::Unauthorized,
                crate::error_codes::ErrorCode::UNAUTHORIZED,
            ),
            (
                AppError::InvalidToken("x".into()),
                crate::error_codes::ErrorCode::INVALID_TOKEN,
            ),
            (
                AppError::TokenExpired,
                crate::error_codes::ErrorCode::TOKEN_EXPIRED,
            ),
            (
                AppError::Forbidden("x".into()),
                crate::error_codes::ErrorCode::FORBIDDEN,
            ),
            (
                AppError::AdminRequired,
                crate::error_codes::ErrorCode::ADMIN_REQUIRED,
            ),
            (
                AppError::NotFound("x".into()),
                crate::error_codes::ErrorCode::NOT_FOUND,
            ),
            (
                AppError::Conflict("x".into()),
                crate::error_codes::ErrorCode::CONFLICT,
            ),
            (
                AppError::UsernameTaken,
                crate::error_codes::ErrorCode::USERNAME_TAKEN,
            ),
            (
                AppError::Validation("x".into()),
                crate::error_codes::ErrorCode::VALIDATION,
            ),
            (
                AppError::RoleNotAllowed("x".into()),
                crate::error_codes::ErrorCode::ROLE_NOT_ALLOWED,
            ),
            (
                AppError::RateLimited {
                    retry_after_secs: 60,
                },
                crate::error_codes::ErrorCode::RATE_LIMITED,
            ),
            (
                AppError::Internal("x".into()),
                crate::error_codes::ErrorCode::INTERNAL,
            ),
        ];
        for (err, expected) in pairs {
            assert_eq!(err.code(), *expected, "variant {:?} has code mismatch", err);
        }
    }

    #[test]
    fn error_messages_include_context() {
        assert_eq!(
            AppError::NotFound("user 42".into()).to_string(),
            "not found: user 42"
        );
        assert_eq!(
            AppError::Conflict("duplicate".into()).to_string(),
            "conflict: duplicate"
        );
        assert_eq!(AppError::Unauthorized.to_string(), "unauthorized");
        assert_eq!(AppError::TokenExpired.to_string(), "token expired");
        assert_eq!(
            AppError::UsernameTaken.to_string(),
            "username already taken"
        );
    }

    #[test]
    fn body_carries_code_and_message() {
        let body = AppError::NotFound("widget x".into()).body();
        assert_eq!(body.code, "not_found");
        assert_eq!(body.message, "not found: widget x");
    }

    #[test]
    fn with_message_wraps_into_localized_preserving_status_and_code() {
        let err = AppError::Validation("must be > 0".into()).with_message("ต้องมากกว่า 0".into());
        match &err {
            AppError::Localized {
                status,
                code,
                message,
            } => {
                assert_eq!(*status, StatusCode::UNPROCESSABLE_ENTITY);
                assert_eq!(*code, "validation");
                assert_eq!(message, "ต้องมากกว่า 0");
            }
            other => panic!("expected Localized, got {other:?}"),
        }

        assert_eq!(err.status(), 422);
        assert_eq!(err.code(), "validation");
    }

    #[test]
    fn localized_body_uses_provided_message_not_display() {
        let err = AppError::NotFound("user 42".into()).with_message("找不到使用者".into());
        let body = err.body();
        assert_eq!(body.code, "not_found");
        assert_eq!(body.message, "找不到使用者");
    }

    #[test]
    fn localized_status_and_code_independent_of_variant() {

        let err = AppError::Localized {
            status: StatusCode::SERVICE_UNAVAILABLE,
            code: "maintenance",
            message: "ปิดปรับปรุง".into(),
        };
        assert_eq!(err.status(), 503);
        assert_eq!(err.code(), "maintenance");
        assert_eq!(err.body().message, "ปิดปรับปรุง");
    }

    fn header_value(resp: &Response, name: axum::http::header::HeaderName) -> Option<String> {
        resp.headers()
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned)
    }

    #[test]
    fn unauthorized_response_carries_www_authenticate_bearer() {

        let resp = AppError::Unauthorized.into_response();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        let challenge =
            header_value(&resp, axum::http::header::WWW_AUTHENTICATE).expect("header must exist");
        assert!(
            challenge.starts_with("Bearer "),
            "scheme must be Bearer, got `{challenge}`"
        );
        assert!(
            challenge.contains("realm="),
            "challenge must include realm, got `{challenge}`"
        );
    }

    #[test]
    fn invalid_token_response_carries_www_authenticate_bearer() {

        let resp = AppError::InvalidToken("bad sig".into()).into_response();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        let challenge = header_value(&resp, axum::http::header::WWW_AUTHENTICATE)
            .expect("header must exist on 401");
        assert!(challenge.starts_with("Bearer "));
    }

    #[test]
    fn token_expired_response_carries_www_authenticate_bearer() {
        let resp = AppError::TokenExpired.into_response();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        assert!(
            header_value(&resp, axum::http::header::WWW_AUTHENTICATE).is_some(),
            "TokenExpired is still 401 — must carry WWW-Authenticate"
        );
    }

    #[test]
    fn non_401_responses_do_not_carry_www_authenticate() {

        for (err, expected_status) in [
            (AppError::BadRequest("x".into()), 400),
            (AppError::Forbidden("x".into()), 403),
            (AppError::NotFound("x".into()), 404),
            (AppError::Conflict("x".into()), 409),
            (AppError::Validation("x".into()), 422),
            (AppError::Internal("x".into()), 500),
        ] {
            let resp = err.into_response();
            assert_eq!(resp.status().as_u16(), expected_status);
            assert!(
                header_value(&resp, axum::http::header::WWW_AUTHENTICATE).is_none(),
                "{expected_status} must not carry WWW-Authenticate"
            );
        }
    }

    #[test]
    fn rate_limited_response_carries_retry_after_header() {
        let resp = AppError::RateLimited {
            retry_after_secs: 7,
        }
        .into_response();
        assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
        let retry = header_value(&resp, axum::http::header::RETRY_AFTER)
            .expect("Retry-After must be set on 429");
        assert_eq!(
            retry, "7",
            "value must replay the variant's seconds verbatim"
        );
    }

    #[test]
    fn rate_limited_response_retry_after_uses_clamped_minimum() {

        for secs in [1u64, 5, 30, 60, 300, 3600] {
            let resp = AppError::RateLimited {
                retry_after_secs: secs,
            }
            .into_response();
            let retry = header_value(&resp, axum::http::header::RETRY_AFTER)
                .unwrap_or_else(|| panic!("Retry-After missing for {secs}s"));
            assert_eq!(retry, secs.to_string());
        }
    }

    #[test]
    fn non_429_responses_do_not_carry_retry_after() {

        let resp = AppError::Internal("db down".into()).into_response();
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert!(
            header_value(&resp, axum::http::header::RETRY_AFTER).is_none(),
            "500 must not carry Retry-After"
        );
    }
}
