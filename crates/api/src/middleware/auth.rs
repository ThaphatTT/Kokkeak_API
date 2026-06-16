//! Authentication middleware + extractor (M2).
//!
//! `AuthnUser` is an axum `FromRequestParts` extractor: any handler
//! that takes `AuthnUser` as a parameter requires a valid Bearer
//! token. The extractor:
//! 1. Reads `Authorization: Bearer <token>`.
//! 2. Verifies the token via the configured `JwtService`.
//! 3. Builds an `AuthSession` with user id, roles, and expiry.

use axum::{
    extract::FromRequestParts,
    http::request::Parts,
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Utc};
use kokkak_common::response::ApiResponse;
use kokkak_domain::{AuthError, AuthSession, Role};
use serde_json::json;
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
        // Read the Authorization header.
        let auth_header = parts
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok());
        let token = match auth_header {
            Some(s) if s.starts_with("Bearer ") => &s[7..],
            _ => return Err(unauthorized("missing or invalid Authorization header")),
        };
        // Verify via JWT service.
        let claims = state.jwt.verify(token).map_err(|e| match e {
            AuthError::TokenExpired => unauthorized("token expired"),
            _ => unauthorized(&e.to_string()),
        })?;
        if claims.kind != kokkak_domain::TokenKind::Access {
            return Err(unauthorized("not an access token"));
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

/// 401 response in the standard envelope.
fn unauthorized(msg: &str) -> Response {
    let envelope: ApiResponse<()> = ApiResponse {
        success: false,
        data: None,
        error: Some(kokkak_common::error::ApiErrorBody {
            code: "unauthorized".into(),
            message: msg.into(),
        }),
        meta: None,
    };
    (StatusCode::UNAUTHORIZED, Json(envelope)).into_response()
}

/// Build a 403 response (for use in handlers that check roles).
pub fn forbidden(msg: &str) -> Response {
    let envelope: ApiResponse<()> = ApiResponse {
        success: false,
        data: None,
        error: Some(kokkak_common::error::ApiErrorBody {
            code: "forbidden".into(),
            message: msg.into(),
        }),
        meta: None,
    };
    (StatusCode::FORBIDDEN, Json(envelope)).into_response()
}

/// RBAC helper: `assert_role` returns `Ok(())` if the user has the
/// role, otherwise returns the 403 response.
pub fn assert_role(user: &AuthnUser, role: Role) -> Result<(), Response> {
    if user.has_role(role) {
        Ok(())
    } else {
        Err(forbidden(&format!("role_required={}", role.as_str())))
    }
}

// Suppress an unused import warning.
#[allow(dead_code)]
fn _silence_json() -> serde_json::Value {
    json!({})
}
