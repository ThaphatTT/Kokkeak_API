//! Catalog HTTP handlers (M3).
//!
//! - GET /api/v1/catalog/services

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use kokkak_common::response::{paginated, ApiResponse, PageMeta};
use serde::{Deserialize, Serialize};

use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    /// Cursor returned by the previous page (omit for first page).
    pub after: Option<String>,
    /// Page size (default 20, max 200).
    pub limit: Option<u32>,
}

#[derive(Debug, Serialize)]
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

/// GET /api/v1/catalog/services
pub async fn list_services(
    State(state): State<AppState>,
    Query(q): Query<ListQuery>,
) -> Result<Response, Response> {
    let limit = q.limit.unwrap_or(20);
    let page = state
        .catalog
        .list_active(q.after, limit)
        .await
        .map_err(repo_error_to_response)?;
    let has_next = page.next_cursor.is_some();
    let items: Vec<ServiceItem> = page.items.into_iter().map(ServiceItem::from).collect();
    let resp_data = items;
    let meta = PageMeta {
        limit: limit as usize,
        has_next,
        next_cursor: page.next_cursor,
    };
    Ok((
        StatusCode::OK,
        paginated(resp_data, meta),
    )
        .into_response())
}

fn repo_error_to_response(err: kokkak_domain::RepoError) -> Response {
    use kokkak_domain::RepoError::*;
    let (status, code) = match &err {
        NotFound(_) => (StatusCode::NOT_FOUND, "not_found"),
        Conflict(_) => (StatusCode::CONFLICT, "conflict"),
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
