use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use kokkak_common::i18n::{current_locale, tr};
use kokkak_common::response::{ok, ApiResponse};
use kokkak_domain::Permission;
use serde::Deserialize;

use crate::error::{ApiError, IntoLocalizedResponse};
use crate::middleware::auth::{assert_scope_admin_page, AuthnUser};
use crate::state::AppState;

#[derive(Debug, Deserialize, utoipa::IntoParams, utoipa::ToSchema)]
pub struct UserAutocompleteQuery {
    pub keyword: Option<String>,

    pub page_number: Option<i32>,

    pub page_size: Option<i32>,
}

#[utoipa::path(
    get,
    path = "/api/v1/users/autocomplete",
    tag = "users",
    params(UserAutocompleteQuery),
    responses(
        (status = 200, description = "User autocomplete page", body = kokkak_domain::UserAutocompletePage),
        (status = 401, description = "Not authenticated", body = crate::openapi::ApiError),
        (status = 422, description = "Validation error", body = crate::openapi::ApiError),
    ),
    security(("bearer_auth" = []))
)]
pub async fn autocomplete_users_admin(
    State(state): State<AppState>,
    user: AuthnUser,
    Query(q): Query<UserAutocompleteQuery>,
) -> Result<Response, Response> {
    let locale = current_locale();
    assert_scope_admin_page(&user, tr("err_auth.forbidden", &locale, &[]))?;

    if !user
        .has_permission(Permission::JobsCreate, &state.permission_checker)
        .await
    {
        let envelope: ApiResponse<()> = ApiResponse {
            success: false,
            data: None,
            error: Some(kokkak_common::error::ApiErrorBody {
                code: "permission_denied".into(),
                message: tr("err_auth.permission_denied", &locale, &["JOBS_CREATE"]),
            }),
            meta: None,
        };
        return Err((StatusCode::FORBIDDEN, Json(envelope)).into_response());
    }

    if let Some(ps) = q.page_size {
        if !(1..=100).contains(&ps) {
            let envelope: ApiResponse<()> = ApiResponse {
                success: false,
                data: None,
                error: Some(kokkak_common::error::ApiErrorBody {
                    code: "validation".into(),
                    message: tr(
                        "err_auth.validation",
                        &locale,
                        &["page_size must be between 1 and 100"],
                    ),
                }),
                meta: None,
            };
            return Err((StatusCode::UNPROCESSABLE_ENTITY, Json(envelope)).into_response());
        }
    }

    let input = kokkak_domain::UserAutocompleteInput {
        keyword: q.keyword,
        page_number: q.page_number,
        page_size: q.page_size,
    };

    let rows = match state.user.autocomplete(input).await {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(error = %e, "user.autocomplete failed");
            return Err(ApiError::from(e).into_localized_response(&state).await);
        }
    };

    Ok((StatusCode::OK, ok(rows)).into_response())
}

#[derive(Debug, Deserialize, utoipa::IntoParams, utoipa::ToSchema)]
pub struct UserAddressQuery {
    pub page_number: Option<i32>,

    pub page_size: Option<i32>,
}

#[utoipa::path(
    get,
    path = "/api/v1/users/{user_guid}/addresses",
    tag = "users",
    params(
        ("user_guid" = String, Path, description = "User GUID"),
        UserAddressQuery,
    ),
    responses(
        (status = 200, description = "User addresses", body = kokkak_domain::UserAddressPage),
        (status = 401, description = "Not authenticated", body = crate::openapi::ApiError),
        (status = 403, description = "Permission denied", body = crate::openapi::ApiError),
        (status = 422, description = "Validation error", body = crate::openapi::ApiError),
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_user_addresses_admin(
    State(state): State<AppState>,
    user: AuthnUser,
    axum::extract::Path(user_guid): axum::extract::Path<String>,
    Query(q): Query<UserAddressQuery>,
) -> Result<Response, Response> {
    let locale = current_locale();
    assert_scope_admin_page(&user, tr("err_auth.forbidden", &locale, &[]))?;

    if !user
        .has_permission(Permission::JobsCreate, &state.permission_checker)
        .await
    {
        let envelope: ApiResponse<()> = ApiResponse {
            success: false,
            data: None,
            error: Some(kokkak_common::error::ApiErrorBody {
                code: "permission_denied".into(),
                message: tr("err_auth.permission_denied", &locale, &["JOBS_CREATE"]),
            }),
            meta: None,
        };
        return Err((StatusCode::FORBIDDEN, Json(envelope)).into_response());
    }

    let trimmed = user_guid.trim().to_string();
    if trimmed.is_empty() {
        let envelope: ApiResponse<()> = ApiResponse {
            success: false,
            data: None,
            error: Some(kokkak_common::error::ApiErrorBody {
                code: "validation".into(),
                message: tr("err_auth.validation", &locale, &["user_guid is required"]),
            }),
            meta: None,
        };
        return Err((StatusCode::UNPROCESSABLE_ENTITY, Json(envelope)).into_response());
    }

    let input = kokkak_domain::UserAddressInput {
        user_guid: trimmed,
        page_number: q.page_number,
        page_size: q.page_size,
    };

    let page = match state.user.get_addresses_by_user_guid(input).await {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(error = %e, "user.get_addresses_by_user_guid failed");
            return Err(ApiError::from(e).into_localized_response(&state).await);
        }
    };

    Ok((StatusCode::OK, ok(page)).into_response())
}

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
