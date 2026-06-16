//! User HTTP handlers (M2).
//!
//! - GET /api/v1/users/me

use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use kokkak_common::response::ApiResponse;

use crate::middleware::auth::AuthnUser;
use crate::state::AppState;

/// GET /api/v1/users/me
pub async fn get_me(State(state): State<AppState>, user: AuthnUser) -> Result<Response, Response> {
    let me = state
        .user
        .get_me(user.id())
        .await
        .map_err(|e| auth_error_to_response(e))?;
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

fn auth_error_to_response(err: kokkak_domain::AuthError) -> Response {
    use kokkak_domain::AuthError::*;
    let (status, code) = match &err {
        InvalidCredentials => (StatusCode::UNAUTHORIZED, "unauthorized"),
        TokenExpired => (StatusCode::UNAUTHORIZED, "token_expired"),
        InvalidToken(_) => (StatusCode::UNAUTHORIZED, "invalid_token"),
        Forbidden(_) => (StatusCode::FORBIDDEN, "forbidden"),
        EmailTaken => (StatusCode::CONFLICT, "email_taken"),
        Validation(_) => (StatusCode::UNPROCESSABLE_ENTITY, "validation"),
        Backend(_) => (StatusCode::INTERNAL_SERVER_ERROR, "internal"),
    };
    let envelope: ApiResponse<()> = ApiResponse {
        success: false,
        data: None,
        error: Some(kokkak_common::error::ApiErrorBody {
            code: code.into(),
            message: err.to_string(),
        }),
        meta: None,
    };
    (status, Json(envelope)).into_response()
}
