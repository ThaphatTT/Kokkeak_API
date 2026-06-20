//! User HTTP handlers (M2 + M11 i18n).
//!
//! - GET /api/v1/users/me

use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use kokkak_common::i18n::{current_locale, tr_with_repo};
use kokkak_common::response::ApiResponse;
use kokkak_domain::{AuthError, LocalizedError};

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
        Err(e) => return Err(auth_error_to_response(e, &state).await),
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

async fn auth_error_to_response(err: AuthError, state: &AppState) -> Response {
    use AuthError::*;
    let (status, code) = match &err {
        InvalidCredentials => (StatusCode::UNAUTHORIZED, "unauthorized"),
        TokenExpired => (StatusCode::UNAUTHORIZED, "token_expired"),
        InvalidToken(_) => (StatusCode::UNAUTHORIZED, "invalid_token"),
        Forbidden(_) => (StatusCode::FORBIDDEN, "forbidden"),
        // M14: renamed to match NEW_DB's username-based login.
        UsernameTaken => (StatusCode::CONFLICT, "username_taken"),
        Validation(_) => (StatusCode::UNPROCESSABLE_ENTITY, "validation"),
        Backend(_) => (StatusCode::INTERNAL_SERVER_ERROR, "internal"),
    };
    let locale = current_locale();
    let args: Vec<String> = err.l10n_args();
    let args_ref: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let message = tr_with_repo(&*state.translation, &locale, err.l10n_key(), &args_ref).await;
    let envelope: ApiResponse<()> = ApiResponse {
        success: false,
        data: None,
        error: Some(kokkak_common::error::ApiErrorBody {
            code: code.into(),
            message,
        }),
        meta: None,
    };
    (status, Json(envelope)).into_response()
}
