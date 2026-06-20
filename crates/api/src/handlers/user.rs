//! User HTTP handlers (M2 + M11 i18n + T-06 AppError refactor).
//!
//! - GET /api/v1/users/me
//!
//! **T-06**: the duplicate `auth_error_to_response` helper that
//! used to live here was deleted; errors now flow through
//! [`crate::error::ApiError`] + `IntoLocalizedResponse`, identical
//! to the auth handlers.

use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use kokkak_common::response::ApiResponse;

use crate::error::{ApiError, IntoLocalizedResponse};
use crate::middleware::auth::AuthnUser;
use crate::state::AppState;

/// GET /api/v1/users/me
#[utoipa::path(
    get,
    path = "/api/v1/users/me",
    tag = "users",
    responses(
        (status = 200, description = "Current user", body = kokkak_domain::PublicUser),
        (status = 401, description = "Not authenticated", body = crate::openapi::ApiError),
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_me(State(state): State<AppState>, user: AuthnUser) -> Result<Response, Response> {
    let me = match state.user.get_me(user.id()).await {
        Ok(u) => u,
        Err(e) => return Err(ApiError::from(e).into_localized_response(&state).await),
    };
    Ok((
        StatusCode::OK,
        Json(ApiResponse {
            success: true,
            data: Some(me),
            error: None,
            meta: None,
        }),
    )
        .into_response())
}
