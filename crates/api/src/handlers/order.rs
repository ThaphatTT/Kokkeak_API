//! Order HTTP handlers (M3).
//!
//! - GET /api/v1/orders/me  (customer: list my orders)
//! - GET /api/v1/orders/assigned  (technician: list my assigned orders)

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use kokkak_common::response::{paginated, ApiResponse, PageMeta};
use kokkak_domain::Role;
use serde::{Deserialize, Serialize};

use crate::middleware::auth::{assert_role, AuthnUser};
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    pub after: Option<String>,
    pub limit: Option<u32>,
}

#[derive(Debug, Serialize)]
pub struct OrderItem {
    pub id: uuid::Uuid,
    pub service_code: String,
    pub customer_id: uuid::Uuid,
    pub technician_id: Option<uuid::Uuid>,
    pub description: String,
    pub address: String,
    pub total: String,
    pub status: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl From<kokkak_domain::Order> for OrderItem {
    fn from(o: kokkak_domain::Order) -> Self {
        Self {
            id: o.id,
            service_code: o.service_code,
            customer_id: o.customer_id,
            technician_id: o.technician_id,
            description: o.description,
            address: o.address,
            total: o.total.to_string(),
            status: o.status.as_str().to_string(),
            created_at: o.created_at,
        }
    }
}

/// GET /api/v1/orders/me  — list orders for the current customer.
pub async fn list_my_orders(
    State(state): State<AppState>,
    user: AuthnUser,
    Query(q): Query<ListQuery>,
) -> Result<Response, Response> {
    assert_role(&user, Role::Customer).map_err(|r| r)?;
    let limit = q.limit.unwrap_or(20);
    let page = state
        .orders
        .list_for_customer(user.id(), q.after, limit)
        .await
        .map_err(repo_error_to_response)?;
    let has_next = page.next_cursor.is_some();
    let items: Vec<OrderItem> = page.items.into_iter().map(OrderItem::from).collect();
    let meta = PageMeta {
        limit: limit as usize,
        has_next,
        next_cursor: page.next_cursor,
    };
    Ok((StatusCode::OK, paginated(items, meta)).into_response())
}

#[derive(Debug, serde::Deserialize)]
pub struct CreateOrderRequest {
    pub service_code: String,
    pub description: String,
    pub address: String,
    pub total: String,
    pub order_lat: Option<f64>,
    pub order_lon: Option<f64>,
}

/// POST /api/v1/orders  — create a new order (M6).
///
/// Only customers can create orders. The persisted order is then
/// published on `order.dispatch` for the worker to fan out.
pub async fn create_order(
    State(state): State<AppState>,
    user: AuthnUser,
    Json(req): Json<CreateOrderRequest>,
) -> Result<Response, Response> {
    assert_role(&user, Role::Customer).map_err(|r| r)?;
    let total: rust_decimal::Decimal = req.total.parse().map_err(|_| {
        repo_error_to_response(kokkak_domain::RepoError::Backend("invalid total".into()))
    })?;
    let input = kokkak_application::order::CreateOrderInput {
        service_code: req.service_code,
        customer_id: user.id(),
        description: req.description,
        address: req.address,
        total,
        order_lat: req.order_lat,
        order_lon: req.order_lon,
    };
    let order = state
        .orders
        .create_order(input)
        .await
        .map_err(repo_error_to_response)?;
    Ok((
        StatusCode::CREATED,
        Json(ApiResponse {
            success: true,
            data: Some(OrderItem::from(order)),
            error: None,
            meta: None,
        }),
    )
        .into_response())
}

/// GET /api/v1/orders/assigned  — list orders assigned to the current technician.
pub async fn list_assigned_orders(
    State(state): State<AppState>,
    user: AuthnUser,
    Query(q): Query<ListQuery>,
) -> Result<Response, Response> {
    assert_role(&user, Role::Technician).map_err(|r| r)?;
    let limit = q.limit.unwrap_or(20);
    let page = state
        .orders
        .list_for_technician(user.id(), q.after, limit)
        .await
        .map_err(repo_error_to_response)?;
    let has_next = page.next_cursor.is_some();
    let items: Vec<OrderItem> = page.items.into_iter().map(OrderItem::from).collect();
    let meta = PageMeta {
        limit: limit as usize,
        has_next,
        next_cursor: page.next_cursor,
    };
    Ok((StatusCode::OK, paginated(items, meta)).into_response())
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
