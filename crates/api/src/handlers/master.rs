

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use kokkak_common::response::ApiResponse;

#[allow(unused_imports)]
use kokkak_domain::{
    MasterDropdownRow, MasterPositionAutocompleteRow, UserDepartmentTeamAutocompleteRow,
};
use serde::Deserialize;

use crate::error::{ApiError, IntoLocalizedResponse};
use crate::state::AppState;

#[derive(Debug, Deserialize, utoipa::ToSchema, utoipa::IntoParams)]
pub struct CountriesQuery {

    pub keyword: Option<String>,

    pub status: Option<i32>,
}

#[utoipa::path(
    get,
    path = "/api/v1/master/countries",
    tag = "master",
    params(CountriesQuery),
    responses(
        (status = 200, description = "Country dropdown rows", body = Vec<MasterDropdownRow>),
        (status = 401, description = "Not authenticated", body = crate::openapi::ApiError),
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_countries(
    State(state): State<AppState>,
    Query(q): Query<CountriesQuery>,
) -> Result<Response, Response> {
    let rows = match state
        .master
        .list_countries(q.keyword.as_deref(), q.status)
        .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(error = %e, "master.list_countries failed");

            return Err(ApiError::from(e).into_localized_response(&state).await);
        }
    };

    Ok((
        StatusCode::OK,
        Json(ApiResponse {
            success: true,
            data: Some(rows),
            error: None,
            meta: None,
        }),
    )
        .into_response())
}

#[derive(Debug, Deserialize, utoipa::ToSchema, utoipa::IntoParams)]
pub struct AutocompleteUserDepartmentTeamQuery {

    pub user_department_guid: Option<String>,

    pub keyword: Option<String>,

    pub take: Option<i32>,
}

#[utoipa::path(
    get,
    path = "/api/v1/master/user-department-teams/autocomplete",
    tag = "master",
    params(AutocompleteUserDepartmentTeamQuery),
    responses(
        (status = 200, description = "User-department-team autocomplete rows", body = Vec<UserDepartmentTeamAutocompleteRow>),
        (status = 401, description = "Not authenticated", body = crate::openapi::ApiError),
    ),
    security(("bearer_auth" = []))
)]
pub async fn autocomplete_user_department_team(
    State(state): State<AppState>,
    Query(q): Query<AutocompleteUserDepartmentTeamQuery>,
) -> Result<Response, Response> {
    let rows = match state
        .master
        .autocomplete_user_department_team(
            q.user_department_guid.as_deref(),
            q.keyword.as_deref(),
            q.take,
        )
        .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(error = %e, "master.autocomplete_user_department_team failed");
            return Err(ApiError::from(e).into_localized_response(&state).await);
        }
    };

    Ok((
        StatusCode::OK,
        Json(ApiResponse {
            success: true,
            data: Some(rows),
            error: None,
            meta: None,
        }),
    )
        .into_response())
}

#[derive(Debug, Deserialize, utoipa::ToSchema, utoipa::IntoParams)]
pub struct AutocompleteUserDepartmentQuery {

    pub keyword: Option<String>,

    pub take: Option<i32>,
}

#[utoipa::path(
    get,
    path = "/api/v1/master/user-departments/autocomplete",
    tag = "master",
    params(AutocompleteUserDepartmentQuery),
    responses(
        (status = 200, description = "User-department autocomplete rows", body = Vec<MasterDropdownRow>),
        (status = 401, description = "Not authenticated", body = crate::openapi::ApiError),
    ),
    security(("bearer_auth" = []))
)]
pub async fn autocomplete_user_department(
    State(state): State<AppState>,
    Query(q): Query<AutocompleteUserDepartmentQuery>,
) -> Result<Response, Response> {
    let rows = match state
        .master
        .autocomplete_user_department(q.keyword.as_deref(), q.take)
        .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(error = %e, "master.autocomplete_user_department failed");
            return Err(ApiError::from(e).into_localized_response(&state).await);
        }
    };

    Ok((
        StatusCode::OK,
        Json(ApiResponse {
            success: true,
            data: Some(rows),
            error: None,
            meta: None,
        }),
    )
        .into_response())
}

#[derive(Debug, Deserialize, utoipa::ToSchema, utoipa::IntoParams)]
pub struct PositionsAutocompleteQuery {

    pub department_team_guid: Option<String>,

    pub keyword: Option<String>,

    pub take: Option<i32>,
}

#[utoipa::path(
    get,
    path = "/api/v1/master/positions/autocomplete",
    tag = "master",
    params(PositionsAutocompleteQuery),
    responses(
        (status = 200, description = "Master-position autocomplete rows", body = Vec<MasterPositionAutocompleteRow>),
        (status = 401, description = "Not authenticated", body = crate::openapi::ApiError),
    ),
    security(("bearer_auth" = []))
)]
pub async fn autocomplete_master_positions(
    State(state): State<AppState>,
    Query(q): Query<PositionsAutocompleteQuery>,
) -> Result<Response, Response> {
    let rows = match state
        .master
        .autocomplete_master_positions(
            q.department_team_guid.as_deref(),
            q.keyword.as_deref(),
            q.take,
        )
        .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(error = %e, "master.autocomplete_master_positions failed");
            return Err(ApiError::from(e).into_localized_response(&state).await);
        }
    };

    Ok((
        StatusCode::OK,
        Json(ApiResponse {
            success: true,
            data: Some(rows),
            error: None,
            meta: None,
        }),
    )
        .into_response())
}
