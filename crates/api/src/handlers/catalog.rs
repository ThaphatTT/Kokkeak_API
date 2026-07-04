

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use kokkak_common::i18n::{current_locale, tr_with_repo};
use kokkak_common::response::{paginated, ApiResponse, PageMeta};
use kokkak_domain::{LocalizedError, RepoError};
use serde::{Deserialize, Serialize};

use crate::state::AppState;

#[derive(Debug, Deserialize, utoipa::ToSchema, utoipa::IntoParams)]
pub struct ListQuery {

    pub after: Option<String>,

    pub limit: Option<u32>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ServiceItem {
    pub id: uuid::Uuid,
    pub code: String,
    pub default_price: Option<rust_decimal::Decimal>,
    pub warranty_days: i32,
    pub sort_order: i32,
}

impl From<kokkak_domain::ServiceCategory> for ServiceItem {
    fn from(s: kokkak_domain::ServiceCategory) -> Self {
        Self {
            id: s.id,
            code: s.code,
            default_price: s.default_price,
            warranty_days: s.warranty_days,
            sort_order: s.sort_order,
        }
    }
}

#[utoipa::path(
    get,
    path = "/api/v1/catalog/services",
    tag = "catalog",
    params(ListQuery),
    responses(
        (status = 200, description = "Active service categories", body = Vec<ServiceItem>),
    )
)]
pub async fn list_services(
    State(state): State<AppState>,
    Query(q): Query<ListQuery>,
) -> Result<Response, Response> {
    let limit = q.limit.unwrap_or(20);
    let page = match state.catalog.list_active(q.after, limit).await {
        Ok(p) => p,
        Err(e) => return Err(repo_error_to_response(e, &state).await),
    };
    let has_next = page.next_cursor.is_some();
    let items: Vec<ServiceItem> = page.items.into_iter().map(ServiceItem::from).collect();
    let resp_data = items;
    let meta = PageMeta {
        limit: limit as usize,
        has_next,
        next_cursor: page.next_cursor,
    };
    Ok((StatusCode::OK, paginated(resp_data, meta)).into_response())
}

async fn repo_error_to_response(err: RepoError, state: &AppState) -> Response {
    use RepoError::*;
    let (status, code) = match &err {
        NotFound(_) => (StatusCode::NOT_FOUND, "not_found"),
        Conflict(_) => (StatusCode::CONFLICT, "conflict"),
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
