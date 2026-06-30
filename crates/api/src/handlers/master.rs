//! Master-data HTTP handlers (M20).
//!
//! Shared reference-data endpoints consumed by every client (mobile /
//! customer web / admin web). The routes are intentionally
//! **authenticated-only** (no admin gate) — master data is consumed
//! by every role. The handler pattern below scales one-for-one:
//! each new master type needs (a) one SP, (b) one trait method,
//! (c) one service method, (d) one handler + query struct, (e) one
//! route under `/api/v1/master/<type>` (dropdown) or
//! `/api/v1/master/<type>/autocomplete` (typeahead).
//!
//! ## Dropdown filter contract (`/api/v1/master/countries`)
//!
//! `?keyword=<text>&status=<0|1|2>` — both optional.
//!
//! - `keyword` is forwarded as-is (trim handled by the SP).
//! - `status=0` returns inactive countries; `status=1` active;
//!   no `status=` (default) also returns active (1). Status=3
//!   (deleted) is hard-excluded by the SP regardless.
//!
//! ## Autocomplete filter contract (`/api/v1/master/<type>/autocomplete`)
//!
//! `?keyword=<text>&take=<int>` — both optional.
//!
//! - `keyword` → SP applies prefix-LIKE on name + code.
//! - `take` → SP defaults to 20, clamps to `[1, 100]`. The infra
//!   adapter re-clamps so the trait contract is self-documenting.

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use kokkak_common::response::ApiResponse;
// `MasterDropdownRow` / `UserDepartmentTeamAutocompleteRow` are
// referenced only inside utoipa `body = ...` annotation strings —
// Rust's unused-imports lint doesn't see those as real uses, so the
// `#[allow]` is the documented escape hatch for type-only imports
// consumed by proc macros / derive attributes.
#[allow(unused_imports)]
use kokkak_domain::{
    MasterDropdownRow, MasterPositionAutocompleteRow, UserDepartmentTeamAutocompleteRow,
};
use serde::Deserialize;

use crate::state::AppState;

/// Query string for `GET /api/v1/master/countries`.
///
/// Mirrors the SP's `@p_keyword` / `@p_master_country_status`
/// shape. Both fields are `Option` — the handler treats absence
/// as "no filter" (the SP defaults `@p_master_country_status=1`
/// when the adapter binds the absent case to active-only).
#[derive(Debug, Deserialize, utoipa::ToSchema, utoipa::IntoParams)]
pub struct CountriesQuery {
    /// Free-text filter against `master_country_name` /
    /// `master_country_code` (LIKE %keyword%).
    pub keyword: Option<String>,
    /// `master_country_status` to filter on. Omit for active-only
    /// (`1`). Pass `0` to include inactive, `2` for any other
    /// non-deleted status. `3` (deleted) is hard-excluded.
    pub status: Option<i32>,
}

/// `GET /api/v1/master/countries?keyword=&status=`
///
/// Country dropdown endpoint shared by mobile / customer web /
/// admin web. Returns one entry per country (`value` = stable
/// GUID string, `label` = display name). The wire shape is the
/// [`MasterDropdownRow`] DTO — one method, one DTO, every master
/// type routes through the same contract.
///
/// Authenticated only (any role). The dropdown is not admin-gated
/// — adding `@p_user_guid` would break mobile clients.
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
            // RepoError today only has `Backend` (no NotFound in
            // dropdown flow); map to 500 generic.
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::<()> {
                    success: false,
                    data: None,
                    error: Some(kokkak_common::error::ApiErrorBody {
                        code: "internal".into(),
                        message: "internal".into(),
                    }),
                    meta: None,
                }),
            )
                .into_response());
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

/// Query string for `GET /api/v1/master/user-department-teams/autocomplete`.
///
/// All three fields are optional — the handler treats absence as
/// "no filter" and lets the SP apply its own defaults (`take = 20`,
/// capped at `100`, hard-coded active-only). The infra adapter
/// re-clamps `take` to `[1, 100]` even though the SP does the same,
/// so the trait contract is self-documenting.
#[derive(Debug, Deserialize, utoipa::ToSchema, utoipa::IntoParams)]
pub struct AutocompleteUserDepartmentTeamQuery {
    /// Scope to one parent department. Omit to search across every
    /// department (admin-web "global" picker view).
    pub user_department_guid: Option<String>,
    /// Free-text filter against team name / code and department
    /// name / code. Trim is handled by the SP.
    pub keyword: Option<String>,
    /// Max rows to return. Omit for SP default (`20`); values
    /// `<= 0` map to `20`; values `> 100` are clamped to `100`.
    pub take: Option<i32>,
}

/// `GET /api/v1/master/user-department-teams/autocomplete`
///
/// Autocomplete lookup for the admin user-form's
/// `user_department_team` picker. Returns up to `take` rows with
/// both team and parent-department columns, ordered by department
/// name → team name → team code. Wire shape is
/// [`UserDepartmentTeamAutocompleteRow`].
///
/// Authenticated only (same gate as the country dropdown — the
/// admin web console enforces the admin role on the client side;
/// adding a server-side admin gate would 401 mobile technicians
/// who legitimately need to read this lookup).
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
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::<()> {
                    success: false,
                    data: None,
                    error: Some(kokkak_common::error::ApiErrorBody {
                        code: "internal".into(),
                        message: "internal".into(),
                    }),
                    meta: None,
                }),
            )
                .into_response());
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

/// Query string for `GET /api/v1/master/user-departments/autocomplete`.
///
/// Both fields are optional — the handler treats absence as "no
/// filter" and lets the SP / infra adapter apply the documented
/// defaults (`take = 20`, capped at `100`, hard-coded active-only).
#[derive(Debug, Deserialize, utoipa::ToSchema, utoipa::IntoParams)]
pub struct AutocompleteUserDepartmentQuery {
    /// Free-text filter against `user_department_name` /
    /// `user_department_code` (prefix-LIKE). Trim is handled by the SP.
    pub keyword: Option<String>,
    /// Max rows to return. Omit for SP default (`20`); values
    /// `<= 0` map to `20`; values `> 100` are clamped to `100`.
    pub take: Option<i32>,
}

/// `GET /api/v1/master/user-departments/autocomplete`
///
/// Autocomplete lookup for the admin user-form's
/// `user_department` picker. Returns up to `take` rows ordered by
/// `user_department_name` → `user_department_code`. Wire shape is
/// the same [`MasterDropdownRow`] as the country dropdown — clients
/// pattern-match on `value` regardless of which master type is in
/// play (country vs user_department share the contract).
///
/// Authenticated only (same gate as the country dropdown — the
/// admin web console enforces the admin role on the client side;
/// adding a server-side admin gate would 401 mobile technicians
/// who legitimately need to read this lookup).
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
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::<()> {
                    success: false,
                    data: None,
                    error: Some(kokkak_common::error::ApiErrorBody {
                        code: "internal".into(),
                        message: "internal".into(),
                    }),
                    meta: None,
                }),
            )
                .into_response());
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

/// Query string for `GET /api/v1/master/positions/autocomplete`.
///
/// Both fields are optional — the handler treats absence as "no
/// filter" and lets the SP apply its own defaults (`take = 20`,
/// active-only `status = 1`). The infra adapter re-clamps `take`
/// to `[1, 100]` so the trait contract is self-documenting (same
/// pattern as the user-department autocomplete).
#[derive(Debug, Deserialize, utoipa::ToSchema, utoipa::IntoParams)]
pub struct PositionsAutocompleteQuery {
    /// Free-text filter against `master_position_name` /
    /// `master_position_code` (prefix-LIKE). Trim is handled by the SP.
    pub keyword: Option<String>,
    /// Max rows to return. Omit for SP default (`20`); values
    /// `<= 0` map to `20`; values `> 100` are clamped to `100`.
    pub take: Option<i32>,
}

/// `GET /api/v1/master/positions/autocomplete`
///
/// Autocomplete lookup for the admin user-form's `master_position`
/// picker. Returns up to `take` rows ordered by
/// `master_position_level DESC → master_position_name ASC →
/// master_position_code ASC`. Wire shape is the richer
/// [`MasterPositionAutocompleteRow`] (carries `code`, `description`,
/// `level`, `status` alongside the `value` / `label` pair), so the
/// admin UI can render rich autocomplete results instead of a plain
/// label/value dropdown.
///
/// Authenticated only (same gate as the country dropdown — the admin
/// web console enforces the admin role client-side; adding a
/// server-side admin gate would 401 mobile technicians who
/// legitimately need to read this lookup).
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
        .autocomplete_master_positions(q.keyword.as_deref(), q.take)
        .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(error = %e, "master.autocomplete_master_positions failed");
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse::<()> {
                    success: false,
                    data: None,
                    error: Some(kokkak_common::error::ApiErrorBody {
                        code: "internal".into(),
                        message: "internal".into(),
                    }),
                    meta: None,
                }),
            )
                .into_response());
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
