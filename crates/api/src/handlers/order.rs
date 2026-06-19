//! Order HTTP handlers (M3 + M6 + M11 i18n).
//!
//! - GET /api/v1/orders/me  (customer: list my orders)
//! - GET /api/v1/orders/assigned  (technician: list my assigned orders)
//! - POST /api/v1/orders  (customer: create a new order)

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use kokkak_common::i18n::{current_locale, tr, tr_with_repo};
use kokkak_common::response::{paginated, ApiResponse, PageMeta};
use kokkak_domain::{LocalizedError, RepoError, Role};
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
    let locale = current_locale();
    let role_msg = tr_with_repo(
        &*state.translation,
        &locale,
        "err_auth.role_required",
        &[Role::Customer.as_str()],
    )
    .await;
    if let Err(r) = assert_role(&user, Role::Customer, role_msg) {
        return Err(r);
    }
    let limit = q.limit.unwrap_or(20);
    let page = match state
        .orders
        .list_for_customer(user.id(), q.after, limit)
        .await
    {
        Ok(p) => p,
        Err(e) => return Err(repo_error_to_response(e, &state).await),
    };
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
    let locale = current_locale();
    let role_msg = tr_with_repo(
        &*state.translation,
        &locale,
        "err_auth.role_required",
        &[Role::Customer.as_str()],
    )
    .await;
    if let Err(r) = assert_role(&user, Role::Customer, role_msg) {
        return Err(r);
    }
    let total: rust_decimal::Decimal = match req.total.parse() {
        Ok(d) => d,
        Err(_) => {
            // Localized "invalid total" message — uses the
            // file-based `tr` because the error is a client
            // validation problem, not a backend failure.
            let msg = tr("err_order.invalid_total", &locale, &[]);
            let envelope: ApiResponse<()> = ApiResponse {
                success: false,
                data: None,
                error: Some(kokkak_common::error::ApiErrorBody {
                    code: "validation".into(),
                    message: msg,
                }),
                meta: None,
            };
            return Err((StatusCode::UNPROCESSABLE_ENTITY, Json(envelope)).into_response());
        }
    };
    let input = kokkak_application::order::CreateOrderInput {
        service_code: req.service_code,
        customer_id: user.id(),
        description: req.description,
        address: req.address,
        total,
        order_lat: req.order_lat,
        order_lon: req.order_lon,
    };
    let order = match state.orders.create_order(input).await {
        Ok(o) => o,
        Err(e) => return Err(repo_error_to_response(e, &state).await),
    };
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
    let locale = current_locale();
    let role_msg = tr_with_repo(
        &*state.translation,
        &locale,
        "err_auth.role_required",
        &[Role::Technician.as_str()],
    )
    .await;
    if let Err(r) = assert_role(&user, Role::Technician, role_msg) {
        return Err(r);
    }
    let limit = q.limit.unwrap_or(20);
    let page = match state
        .orders
        .list_for_technician(user.id(), q.after, limit)
        .await
    {
        Ok(p) => p,
        Err(e) => return Err(repo_error_to_response(e, &state).await),
    };
    let has_next = page.next_cursor.is_some();
    let items: Vec<OrderItem> = page.items.into_iter().map(OrderItem::from).collect();
    let meta = PageMeta {
        limit: limit as usize,
        has_next,
        next_cursor: page.next_cursor,
    };
    Ok((StatusCode::OK, paginated(items, meta)).into_response())
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
