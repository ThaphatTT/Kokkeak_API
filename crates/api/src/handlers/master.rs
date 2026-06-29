//! Master-data HTTP handlers (M20).
//!
//! One endpoint today, `GET /api/v1/master/countries`, shared by
//! every client (mobile / customer web / admin web). The route is
//! intentionally **authenticated-only** (no admin gate) — country
//! dropdown is consumed by every role. Add
//! `list_provinces` / `list_banks` here as their SPs land; the
//! handler pattern below scales one-for-one.
//!
//! ## Filter contract
//!
//! `?keyword=<text>&status=<0|1|2>` — both optional.
//!
//! - `keyword` is forwarded as-is (trim handled by the SP).
//! - `status=0` returns inactive countries; `status=1` active;
//!   no `status=` (default) also returns active (1). Status=3
//!   (deleted) is hard-excluded by the SP regardless.

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use kokkak_common::response::ApiResponse;
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
