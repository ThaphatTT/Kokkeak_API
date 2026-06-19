"""Create M0 T04 file structure: AppError + Response envelope."""

from pathlib import Path

ROOT = Path(r"C:\Users\crybo\Desktop\Develop\Kokkeak_API")

# ---------- 1. Update common/Cargo.toml: add axum for IntoResponse ----------
(ROOT / "crates" / "common" / "Cargo.toml").write_text(
    """[package]
name = "kokkak-common"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
authors.workspace = true
description = "Common utilities: error, config, telemetry, response envelope"

[dependencies]
serde = { workspace = true }
figment = { workspace = true }
thiserror = { workspace = true }

# Axum (for IntoResponse impl + Json helpers)
axum = { workspace = true }

# Telemetry
tracing = { workspace = true }
tracing-subscriber = { workspace = true, features = ["env-filter", "json"] }
metrics = { workspace = true }
metrics-exporter-prometheus = { workspace = true }

[dev-dependencies]
serde_json = { workspace = true }
""",
    encoding="utf-8",
)
print("  crates/common/Cargo.toml")

# ---------- 2. Update common/src/lib.rs ----------
(ROOT / "crates" / "common" / "src" / "lib.rs").write_text(
    """//! Common layer
//!
//! Houses shared infrastructure used by every other crate:
//! error types, configuration loader, telemetry, response envelope,
//! and small utilities (UUID v7, time, decimal).
//!
//! See AGENTS.md § 3, 11, 12, 14 for the standards this layer enforces.

#![deny(unsafe_code)]
#![warn(missing_docs)]

pub mod config;
pub mod error;
pub mod response;
pub mod telemetry;

pub use config::{ConfigError, LogFormat, LogSettings, ServerSettings, Settings};
pub use error::{ApiErrorBody, AppError};
pub use response::{created, ok, paginated, ApiResponse, PageMeta};
""",
    encoding="utf-8",
)
print("  crates/common/src/lib.rs")

# ---------- 3. Create common/src/error.rs ----------
(ROOT / "crates" / "common" / "src" / "error.rs").write_text(
    """//! Application error type + JSON body shape.
//!
//! Maps to HTTP status codes per AGENTS.md § 11.3. Renders to a
//! standard `ApiResponse` envelope via `IntoResponse` (see [`ApiResponse`]).

use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
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
""",
    encoding="utf-8",
)
print("  crates/common/src/error.rs")

# ---------- 4. Create common/src/response.rs ----------
(ROOT / "crates" / "common" / "src" / "response.rs").write_text(
    """//! Standard response envelope used by every API handler.
//!
//! Shape (per AGENTS.md § 11.2):
//! ```json
//! { "success": true, "data": {...}, "error": null, "meta": { "page": {...} } }
//! ```
//!
//! Use [`ok`], [`created`], [`paginated`] to build success responses, and
//! [`ApiResponse::error`] (or [`crate::error::AppError`] via
//! `IntoResponse`) for failures.

use axum::Json;
use serde::Serialize;

use crate::error::ApiErrorBody;

/// Standard envelope wrapping either a `data` payload, an `error` body,
/// or both. `error` and `meta` are omitted from the serialized JSON
/// when `None` to keep payloads tidy.
#[derive(Debug, Serialize, Clone)]
pub struct ApiResponse<T> {
    /// `true` for `2xx` responses, `false` for `4xx`/`5xx`.
    pub success: bool,

    /// Success payload. `None` on failure.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,

    /// Failure payload. `None` on success.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ApiErrorBody>,

    /// Pagination metadata. `None` if not applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<PageMeta>,
}

/// Pagination metadata embedded in [`ApiResponse::meta`].
#[derive(Debug, Serialize, Clone)]
pub struct PageMeta {
    /// Page size (capped at the configured maximum).
    pub limit: usize,
    /// `true` if more rows are available after this page.
    pub has_next: bool,
    /// Opaque cursor to fetch the next page; absent when `has_next = false`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

impl<T> ApiResponse<T> {
    /// Build an error envelope with no data/meta.
    pub fn error(code: impl Into<String>, message: impl Into<String>) -> ApiResponse<T> {
        ApiResponse {
            success: false,
            data: None,
            error: Some(ApiErrorBody {
                code: code.into(),
                message: message.into(),
            }),
            meta: None,
        }
    }
}

/// 200 OK envelope: `{"success": true, "data": <T>}`.
pub fn ok<T: Serialize>(data: T) -> Json<ApiResponse<T>> {
    Json(ApiResponse {
        success: true,
        data: Some(data),
        error: None,
        meta: None,
    })
}

/// 201 Created envelope (same shape as `ok`; the status is set by the
/// call site — typically by returning `(StatusCode::CREATED, created(...))`).
pub fn created<T: Serialize>(data: T) -> Json<ApiResponse<T>> {
    Json(ApiResponse {
        success: true,
        data: Some(data),
        error: None,
        meta: None,
    })
}

/// 200 OK envelope with pagination metadata.
pub fn paginated<T: Serialize>(data: T, meta: PageMeta) -> Json<ApiResponse<T>> {
    Json(ApiResponse {
        success: true,
        data: Some(data),
        error: None,
        meta: Some(meta),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn ok_envelope_shape() {
        let resp: ApiResponse<i32> = ok(42);
        let v = serde_json::to_value(&resp).unwrap();
        assert_eq!(v, json!({ "success": true, "data": 42 }));
    }

    #[test]
    fn created_envelope_shape() {
        let resp: ApiResponse<&str> = created("ok");
        let v = serde_json::to_value(&resp).unwrap();
        assert_eq!(v, json!({ "success": true, "data": "ok" }));
    }

    #[test]
    fn paginated_envelope_with_cursor() {
        let meta = PageMeta {
            limit: 20,
            has_next: true,
            next_cursor: Some("abc".into()),
        };
        let resp: ApiResponse<Vec<i32>> = paginated(vec![1, 2, 3], meta);
        let v = serde_json::to_value(&resp).unwrap();
        assert_eq!(
            v,
            json!({
                "success": true,
                "data": [1, 2, 3],
                "meta": { "limit": 20, "has_next": true, "next_cursor": "abc" }
            })
        );
    }

    #[test]
    fn paginated_no_next_cursor_omits_field() {
        let meta = PageMeta {
            limit: 20,
            has_next: false,
            next_cursor: None,
        };
        let resp: ApiResponse<Vec<i32>> = paginated(vec![1], meta);
        let v = serde_json::to_value(&resp).unwrap();
        assert_eq!(
            v,
            json!({
                "success": true,
                "data": [1],
                "meta": { "limit": 20, "has_next": false }
            })
        );
    }

    #[test]
    fn error_envelope_shape() {
        let resp: ApiResponse<i32> = ApiResponse::error("not_found", "user 42");
        let v = serde_json::to_value(&resp).unwrap();
        assert_eq!(
            v,
            json!({
                "success": false,
                "error": { "code": "not_found", "message": "user 42" }
            })
        );
    }
}
""",
    encoding="utf-8",
)
print("  crates/common/src/response.rs")

print("\n=== Files created/updated ===")
for p in sorted((ROOT / "crates" / "common" / "src").rglob("*")):
    if p.is_file():
        print(f"  {p.relative_to(ROOT)}")
