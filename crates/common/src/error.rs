//! Application error type + JSON body shape.
//!
//! Maps to HTTP status codes per AGENTS.md § 11.3. Renders to a
//! standard `ApiResponse` envelope via `IntoResponse` (see [`ApiResponse`]).
//!
//! ## Variants
//!
//! Each variant maps to a specific HTTP status via [`AppError::status`]
//! and a stable snake-case code via [`AppError::code`]. New codes
//! require extending the [`crate::error_codes::ErrorCode`] catalog and
//! adding the matching variant here — codes are STABLE, never renamed.
//!
//! ## Localization
//!
//! The sync [`AppError::IntoResponse`] impl uses the variant's `Display`
//! string, which is fine for logs and tests. For request-scoped
//! localized messages, handlers convert the error to an [`AppError`]
//! and wrap it with [`AppError::with_message`] (or call the api-layer
//! `IntoLocalizedResponse::into_localized_response` extension). The
//! `Localized` variant carries a pre-rendered message; `IntoResponse`
//! surfaces that message verbatim instead of the English `Display`.
//!
//! See [`crate::i18n::tr_with_repo`] for the message lookup used to
//! fill `Localized`.

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
#[derive(Debug, Error, PartialEq, Eq)]
pub enum AppError {
    /// 400 — request is malformed (e.g. invalid JSON, missing field).
    #[error("bad request: {0}")]
    BadRequest(String),

    /// 401 — no/invalid/expired credentials.
    #[error("unauthorized")]
    Unauthorized,

    /// 401 — bearer token signature / format invalid.
    #[error("invalid token: {0}")]
    InvalidToken(String),

    /// 401 — bearer token past its `exp`.
    #[error("token expired")]
    TokenExpired,

    /// 403 — authenticated but lacks required permission.
    #[error("forbidden: {0}")]
    Forbidden(String),

    /// 403 — admin role required (admin-only endpoints).
    #[error("admin role required")]
    AdminRequired,

    /// 404 — resource not found.
    #[error("not found: {0}")]
    NotFound(String),

    /// 409 — state conflict (e.g. unique-key collision).
    #[error("conflict: {0}")]
    Conflict(String),

    /// 409 — username already taken (registration, admin user create).
    #[error("username already taken")]
    UsernameTaken,

    /// 422 — semantic validation failure.
    #[error("validation: {0}")]
    Validation(String),

    /// 422 — role string not in the public-registration allow-list.
    #[error("role not allowed: {0}")]
    RoleNotAllowed(String),

    /// 429 — rate limit hit. The payload is the seconds-until-retry
    /// hint from the rate limiter; [`IntoResponse`] echoes it back in
    /// the `Retry-After` response header (RFC 6585 §4) so well-behaved
    /// clients back off automatically. The HTTP-layer rate limit
    /// middleware and the per-(username, IP) login rate limiter both
    /// produce this variant.
    #[error("rate limited")]
    RateLimited {
        /// Seconds the client should wait before retrying.
        retry_after_secs: u64,
    },

    /// 500 — unexpected internal error. Use for catch-all.
    #[error("internal: {0}")]
    Internal(String),

    /// i18n carrier — wraps any (status, code) with a pre-localized
    /// message. Handlers convert a domain error to [`AppError`] and
    /// then wrap via [`AppError::with_message`] after looking up the
    /// translation key. `IntoResponse` surfaces `message` verbatim
    /// instead of the English `Display`.
    #[error("localized error ({code}): {message}")]
    Localized {
        /// HTTP status the carrier represents. Inherited from the
        /// wrapped variant when produced by [`AppError::with_message`].
        status: StatusCode,
        /// Stable snake-case code (e.g. `"validation"`). Inherited
        /// from the wrapped variant; see [`crate::error_codes`].
        code: &'static str,
        /// Pre-rendered, locale-specific message. Surfaced verbatim
        /// by `IntoResponse` instead of the English `Display`.
        message: String,
    },
}

impl AppError {
    /// Map to the HTTP status code that should be returned.
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

    /// Stable snake-case code (safe for clients to switch on).
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

    /// Build the serializable body that goes in the envelope's `error`
    /// field. For the `Localized` variant, the pre-rendered message is
    /// used verbatim; all other variants fall back to `Display`.
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

    /// Wrap the error in [`AppError::Localized`] with a pre-rendered
    /// message. Use after looking up the i18n key via
    /// [`crate::i18n::tr_with_repo`] so the user sees the request's
    /// locale instead of the English `Display`.
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

        // RFC 7235 §3.1: a 401 response MUST carry a
        // `WWW-Authenticate` challenge. We use Bearer since the API
        // authenticates with JWT bearer tokens. Affects all 401
        // variants (`Unauthorized` / `InvalidToken` / `TokenExpired`).
        // ponytail: realm is a human-readable scope hint, not a
        // security boundary — the API does not run multiple auth
        // schemes that would need realm disambiguation.
        if status == StatusCode::UNAUTHORIZED {
            resp.headers_mut().insert(
                axum::http::header::WWW_AUTHENTICATE,
                axum::http::HeaderValue::from_static("Bearer realm=\"kokkeak\""),
            );
        }

        // RFC 6585 §4: 429 responses SHOULD carry `Retry-After` so
        // well-behaved clients (OkHttp, axios, browser fetch)
        // back off automatically. The seconds come from the rate
        // limiter that produced the error (login RL or HTTP RL).
        if let Self::RateLimited { retry_after_secs } = &self {
            // `u64::to_string()` always produces ASCII digits, which
            // is a valid `HeaderValue` per RFC 7230. The `if let Ok`
            // is defensive only — if a future refactor passes a
            // non-numeric value through this path, the response
            // stays correct (just without the header).
            if let Ok(v) = axum::http::HeaderValue::from_str(&retry_after_secs.to_string()) {
                resp.headers_mut()
                    .insert(axum::http::header::RETRY_AFTER, v);
            }
        }

        resp
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
        // Every code returned by AppError::code() must exist in the
        // ErrorCode catalog (T-17). Adding a variant here requires
        // extending the catalog.
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
        // Status + code are inherited from the original variant.
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
        // The Localized variant can stand on its own — e.g. if a
        // handler wants to surface a localized message that doesn't
        // map to any typed AppError variant.
        let err = AppError::Localized {
            status: StatusCode::SERVICE_UNAVAILABLE,
            code: "maintenance",
            message: "ปิดปรับปรุง".into(),
        };
        assert_eq!(err.status(), 503);
        assert_eq!(err.code(), "maintenance");
        assert_eq!(err.body().message, "ปิดปรับปรุง");
    }

    // ---- Security headers (RFC 7235 §3.1, RFC 6585 §4) ----
    //
    // 401 responses MUST carry a `WWW-Authenticate` challenge
    // (RFC 7235). Without it, well-behaved HTTP clients (browsers,
    // OkHttp, axios interceptors) cannot properly handle auth
    // challenges. 429 responses SHOULD carry `Retry-After` (RFC 6585)
    // so clients back off automatically.

    fn header_value(resp: &Response, name: axum::http::header::HeaderName) -> Option<String> {
        resp.headers()
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned)
    }

    #[test]
    fn unauthorized_response_carries_www_authenticate_bearer() {
        // Login-style failure (no specific reason). The body is
        // generic; the header signals the auth scheme.
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
        // Bearer token with bad signature/format. Same challenge as
        // `Unauthorized` — both are 401, same auth scheme.
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
        // The challenge only makes sense on 401. Other 4xx/5xx
        // must NOT set it (clients would interpret the 4xx as a
        // fresh auth challenge and re-prompt).
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
        // The login rate limiter clamps retry-after to ≥1s (see
        // `RateLimitDecision::retry_after_secs`). The header should
        // reflect whatever the variant carries — the limiter, not
        // the renderer, owns the floor.
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
        // Retry-After is rate-limit-specific. A 503 (Service
        // Unavailable) might also use it in some specs, but
        // `AppError::Internal` is a 500 catch-all — no retry hint.
        let resp = AppError::Internal("db down".into()).into_response();
        assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert!(
            header_value(&resp, axum::http::header::RETRY_AFTER).is_none(),
            "500 must not carry Retry-After"
        );
    }
}
