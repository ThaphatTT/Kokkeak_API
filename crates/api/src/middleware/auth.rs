

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
use kokkak_domain::{AuthError, AuthSession, Permission, Role};
use kokkak_infra::permission_checker::PermissionChecker;
use uuid::Uuid;

use crate::state::AppState;

#[derive(Debug, Clone)]
pub struct AuthnUser(pub AuthSession);

impl AuthnUser {

    pub fn session(&self) -> &AuthSession {
        &self.0
    }

    pub fn id(&self) -> Uuid {
        self.0.user_id
    }

    pub fn roles(&self) -> &[Role] {
        &self.0.roles
    }

    pub fn has_role(&self, role: Role) -> bool {
        self.0.has_role(role)
    }

    pub async fn has_permission(&self, code: Permission, checker: &PermissionChecker) -> bool {
        match checker.has_permission(self.0.user_id, code).await {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(
                    user_id = %self.0.user_id,
                    code = code.code(),
                    error = %e,
                    "AuthnUser::has_permission failed — denying (fail-secure)"
                );
                false
            }
        }
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

#[allow(clippy::result_large_err)]
pub fn assert_role(user: &AuthnUser, role: Role, message: String) -> Result<(), Response> {
    if user.has_role(role) {
        Ok(())
    } else {
        Err(forbidden(message))
    }
}
