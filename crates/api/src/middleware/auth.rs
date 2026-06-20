//! Authentication middleware + extractor (M2 + M11 i18n).
//!
//! `AuthnUser` is an axum `FromRequestParts` extractor: any handler
//! that takes `AuthnUser` as a parameter requires a valid Bearer
//! token. The extractor:
//! 1. Reads `Authorization: Bearer <token>`.
//! 2. Verifies the token via the configured `JwtService`.
//! 3. Builds an `AuthSession` with user id, roles, and expiry.
//!
//! User-visible error strings are rendered via
//! `kokkak_common::i18n::tr` against the file-based catalog (the
//! extractor is sync and runs before the i18n middleware's
//! translation repo wiring; per-tenant overrides are still
//! picked up by `tr` because `rust_i18n::set_locale` has already
//! run by the time the extractor is invoked).

use axum::{
    extract::FromRequestParts,
    http::request::Parts,
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Utc};
use kokkak_common::i18n::{current_locale, tr};
use kokkak_common::response::ApiResponse;
use kokkak_domain::{AuthError, AuthSession, Role};
use uuid::Uuid;

use crate::state::AppState;

/// Authenticated user injected into handlers.
#[derive(Debug, Clone)]
pub struct AuthnUser(pub AuthSession);

impl AuthnUser {
    /// Borrow the inner session.
    pub fn session(&self) -> &AuthSession {
        &self.0
    }

    /// Convenience: user id.
    pub fn id(&self) -> Uuid {
        self.0.user_id
    }

    /// Convenience: roles list.
    pub fn roles(&self) -> &[Role] {
        &self.0.roles
    }

    /// Convenience: has the given role.
    pub fn has_role(&self, role: Role) -> bool {
        self.0.has_role(role)
    }
}

#[axum::async_trait]
impl FromRequestParts<AppState> for AuthnUser {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let locale = current_locale();
        // Read the Authorization header.
        let auth_header = parts
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok());
        let token = match auth_header {
            Some(s) if s.starts_with("Bearer ") => &s[7..],
            _ => {
                return Err(unauthorized(
                    tr("err_auth.missing_header", &locale, &[]),
                    "unauthorized",
                ));
            }
        };
        // Verify via JWT service.
        let claims = state.jwt.verify(token).map_err(|e| match e {
            AuthError::TokenExpired => {
                unauthorized(tr("err_auth.token_expired", &locale, &[]), "token_expired")
            }
            _ => unauthorized(
                tr("err_auth.invalid_token", &locale, &[&e.to_string()]),
                "invalid_token",
            ),
        })?;
        if claims.kind != kokkak_domain::TokenKind::Access {
            return Err(unauthorized(
                tr("err_auth.not_access_token", &locale, &[]),
                "invalid_token",
            ));
        }
        // Build AuthSession.
        let expires_at = DateTime::<Utc>::from_timestamp(claims.exp, 0).unwrap_or_else(Utc::now);
        let session = AuthSession {
            user_id: claims.sub,
            roles: claims.roles,
            expires_at,
            scope: claims.scope,
        };
        Ok(AuthnUser(session))
    }
}

/// 401 response in the standard envelope. `message` is the
/// pre-resolved localized string; `code` is the stable
/// snake-case identifier.
fn unauthorized(message: String, code: &str) -> Response {
    let envelope: ApiResponse<()> = ApiResponse {
        success: false,
        data: None,
        error: Some(kokkak_common::error::ApiErrorBody {
            code: code.into(),
            message,
        }),
        meta: None,
    };
    (StatusCode::UNAUTHORIZED, Json(envelope)).into_response()
}

/// Build a 403 response (for use in handlers that check roles).
/// `message` is the pre-resolved localized string.
pub fn forbidden(message: String) -> Response {
    let envelope: ApiResponse<()> = ApiResponse {
        success: false,
        data: None,
        error: Some(kokkak_common::error::ApiErrorBody {
            code: "forbidden".into(),
            message,
        }),
        meta: None,
    };
    (StatusCode::FORBIDDEN, Json(envelope)).into_response()
}

/// RBAC helper: `assert_role` returns `Ok(())` if the user has the
/// role, otherwise returns the 403 response built from a
/// pre-resolved localized message (the caller resolves the
/// message via `tr_with_repo` because `assert_role` is sync).
///
/// ponytail: `Response` is ~256 B (axum::body::Body), so boxing
/// the error would save a few bytes per call but force every
/// caller to unbox. We keep `Response` inline + allow the lint.
#[allow(clippy::result_large_err)]
pub fn assert_role(user: &AuthnUser, role: Role, message: String) -> Result<(), Response> {
    if user.has_role(role) {
        Ok(())
    } else {
        Err(forbidden(message))
    }
}
