//! Application error type + JSON body shape.
//!
//! Maps to HTTP status codes per AGENTS.md § 11.3. Renders to a
//! standard `ApiResponse` envelope via `IntoResponse` (see [`ApiResponse`]).

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;
use thiserror::Error;

use crate::response::ApiResponse;

/// Top-level application error.
///
/// Each variant maps to a specific HTTP status code via [`AppError::status`]
/// and a stable string code via [`AppError::code`].
#[derive(Debug, Error)]
pub enum AppError {
    /// 400 — request is malformed (e.g. invalid JSON, missing field).
    #[error("bad request: {0}")]
    BadRequest(String),

    /// 401 — no/invalid/expired credentials.
    #[error("unauthorized")]
    Unauthorized,

    /// 403 — authenticated but lacks required permission.
    #[error("forbidden: {0}")]
    Forbidden(String),

    /// 404 — resource not found.
    #[error("not found: {0}")]
    NotFound(String),

    /// 409 — state conflict (e.g. unique-key collision).
    #[error("conflict: {0}")]
    Conflict(String),

    /// 422 — semantic validation failure.
    #[error("validation: {0}")]
    Validation(String),

    /// 429 — rate limit hit.
    #[error("rate limited")]
    RateLimited,

    /// 500 — unexpected internal error. Use for catch-all.
    #[error("internal: {0}")]
    Internal(String),
}

impl AppError {
    /// Map to the HTTP status code that should be returned.
    pub fn status(&self) -> StatusCode {
        match self {
            Self::BadRequest(_) => StatusCode::BAD_REQUEST,
            Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::Forbidden(_) => StatusCode::FORBIDDEN,
            Self::NotFound(_) => StatusCode::NOT_FOUND,
            Self::Conflict(_) => StatusCode::CONFLICT,
            Self::Validation(_) => StatusCode::UNPROCESSABLE_ENTITY,
            Self::RateLimited => StatusCode::TOO_MANY_REQUESTS,
            Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    /// Stable snake-case code (safe for clients to switch on).
    pub fn code(&self) -> &'static str {
        match self {
            Self::BadRequest(_) => "bad_request",
            Self::Unauthorized => "unauthorized",
            Self::Forbidden(_) => "forbidden",
            Self::NotFound(_) => "not_found",
            Self::Conflict(_) => "conflict",
            Self::Validation(_) => "validation",
            Self::RateLimited => "rate_limited",
            Self::Internal(_) => "internal",
        }
    }

    /// Build the serializable body that goes in the envelope's `error` field.
    pub fn body(&self) -> ApiErrorBody {
        ApiErrorBody {
            code: self.code().to_string(),
            message: self.to_string(),
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
        (self.status(), Json(envelope)).into_response()
    }
}

/// JSON body returned in the envelope's `error` field.
#[derive(Debug, Serialize, Clone)]
pub struct ApiErrorBody {
    /// Stable, snake-case error code (e.g. `"not_found"`).
    pub code: String,
    /// Human-readable error message (safe to log/display).
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_codes_per_variant() {
        assert_eq!(AppError::BadRequest("x".into()).status(), 400);
        assert_eq!(AppError::Unauthorized.status(), 401);
        assert_eq!(AppError::Forbidden("x".into()).status(), 403);
        assert_eq!(AppError::NotFound("x".into()).status(), 404);
        assert_eq!(AppError::Conflict("x".into()).status(), 409);
        assert_eq!(AppError::Validation("x".into()).status(), 422);
        assert_eq!(AppError::RateLimited.status(), 429);
        assert_eq!(AppError::Internal("x".into()).status(), 500);
    }

    #[test]
    fn codes_are_snake_case() {
        assert_eq!(AppError::BadRequest("x".into()).code(), "bad_request");
        assert_eq!(AppError::Unauthorized.code(), "unauthorized");
        assert_eq!(AppError::Forbidden("x".into()).code(), "forbidden");
        assert_eq!(AppError::NotFound("x".into()).code(), "not_found");
        assert_eq!(AppError::Conflict("x".into()).code(), "conflict");
        assert_eq!(AppError::Validation("x".into()).code(), "validation");
        assert_eq!(AppError::RateLimited.code(), "rate_limited");
        assert_eq!(AppError::Internal("x".into()).code(), "internal");
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
    }

    #[test]
    fn body_carries_code_and_message() {
        let body = AppError::NotFound("widget x".into()).body();
        assert_eq!(body.code, "not_found");
        assert_eq!(body.message, "not found: widget x");
    }
}
