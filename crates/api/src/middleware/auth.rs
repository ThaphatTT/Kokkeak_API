use std::sync::Arc;

use axum::{
    extract::FromRequestParts,
    extract::Request,
    http::request::Parts,
    http::{header, StatusCode},
    middleware::Next,
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

pub const SCOPE_ADMIN_PAGE: &str = "admin_page";
pub const SCOPE_LANDING_PAGE: &str = "landing_page";
pub const SCOPE_MOBILE: &str = "mobile";

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

    pub fn has_scope(&self, scope: &str) -> bool {
        self.0.scope == scope
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
            jti: claims.jti,
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

pub fn assert_scope(user: &AuthnUser, scope: &str, message: String) -> Result<(), Response> {
    if user.has_scope(scope) {
        Ok(())
    } else {
        Err(forbidden(message))
    }
}

#[allow(clippy::result_large_err)]
pub fn assert_scope_admin_page(user: &AuthnUser, message: String) -> Result<(), Response> {
    assert_scope(user, SCOPE_ADMIN_PAGE, message)
}

#[allow(clippy::result_large_err)]
pub fn assert_scope_landing_page(user: &AuthnUser, message: String) -> Result<(), Response> {
    assert_scope(user, SCOPE_LANDING_PAGE, message)
}

#[allow(clippy::result_large_err)]
pub fn assert_scope_mobile(user: &AuthnUser, message: String) -> Result<(), Response> {
    assert_scope(user, SCOPE_MOBILE, message)
}

pub fn require_scope(
    state: Arc<AppState>,
    required_scope: &'static str,
) -> impl Fn(Request, Next) -> std::pin::Pin<Box<dyn std::future::Future<Output = Response> + Send>>
       + Clone
       + Send
       + Sync
       + 'static {
    move |req, next| {
        let state = state.clone();
        Box::pin(async move {
            let locale = current_locale();
            let (parts, body) = req.into_parts();

            let auth_header = parts
                .headers
                .get(header::AUTHORIZATION)
                .and_then(|v| v.to_str().ok());
            let token = match auth_header {
                Some(s) if s.starts_with("Bearer ") => &s[7..],
                _ => {
                    return unauthorized(
                        tr("err_auth.missing_header", &locale, &[]),
                        "unauthorized",
                    );
                }
            };

            let claims = match state.jwt.verify(token) {
                Ok(c) => c,
                Err(AuthError::TokenExpired) => {
                    return unauthorized(
                        tr("err_auth.token_expired", &locale, &[]),
                        "token_expired",
                    );
                }
                Err(e) => {
                    return unauthorized(
                        tr("err_auth.invalid_token", &locale, &[&e.to_string()]),
                        "invalid_token",
                    );
                }
            };

            if claims.scope != required_scope {
                return forbidden(tr("err_auth.forbidden", &locale, &[]));
            }

            let req = Request::from_parts(parts, body);
            next.run(req).await
        })
    }
}
