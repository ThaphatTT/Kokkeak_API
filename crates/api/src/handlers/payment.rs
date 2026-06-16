//! Payment HTTP handlers (M9).
//!
//! - `POST /api/v1/payments` — open a payment intent.
//! - `POST /api/v1/payments/:id/confirm` — capture + commission + payout.
//! - `GET  /api/v1/payments/me` — list my payments.
//! - `GET  /api/v1/payments/:id` — fetch one payment.
//! - `GET  /api/v1/admin/payouts` — admin: list payouts.
//! - `POST /api/v1/admin/payouts/:id/pay` — admin: mark a payout paid.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use kokkak_application::{ConfirmPaymentInput, CreatePaymentInput, PaymentService};
use kokkak_common::response::{paginated, ApiResponse, PageMeta};
use kokkak_domain::{Commission, Payment, PaymentError, Payout, PayoutStatus};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::middleware::auth::{assert_role, AuthnUser};
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct CreatePaymentRequest {
    pub order_id: Uuid,
    /// Optional override (otherwise the order's total is used).
    pub amount: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PaymentDto {
    pub id: Uuid,
    pub order_id: Uuid,
    pub customer_id: Uuid,
    pub amount: String,
    pub status: String,
    pub currency: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl From<Payment> for PaymentDto {
    fn from(p: Payment) -> Self {
        Self {
            id: p.id,
            order_id: p.order_id,
            customer_id: p.customer_id,
            amount: p.amount.to_string(),
            status: p.status.as_str().to_string(),
            currency: p.currency,
            created_at: p.created_at,
            updated_at: p.updated_at,
        }
    }
}

/// POST /api/v1/payments — open a payment intent.
pub async fn create_payment(
    State(state): State<AppState>,
    user: AuthnUser,
    Json(req): Json<CreatePaymentRequest>,
) -> Result<Response, Response> {
    assert_role(&user, kokkak_domain::Role::Customer).map_err(|r| r)?;
    let amount = match req.amount {
        Some(s) => match s.parse::<Decimal>() {
            Ok(d) => Some(d),
            Err(_) => {
                return Ok(bad_request("amount must be a decimal string"));
            }
        },
        None => None,
    };
    let input = CreatePaymentInput {
        order_id: req.order_id,
        customer_id: user.id(),
        amount,
    };
    let p = state
        .payments
        .create_payment(input)
        .await
        .map_err(payment_err_to_response)?;
    Ok((
        StatusCode::CREATED,
        Json(ApiResponse {
            success: true,
            data: Some(PaymentDto::from(p)),
            error: None,
            meta: None,
        }),
    )
        .into_response())
}

#[derive(Debug, Deserialize)]
pub struct ConfirmPaymentRequest {
    pub gateway_ref: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ConfirmPaymentDto {
    pub payment: PaymentDto,
    pub commission: CommissionDto,
    pub payout: PayoutDto,
}

#[derive(Debug, Serialize)]
pub struct CommissionDto {
    pub id: Uuid,
    pub order_id: Uuid,
    pub technician_id: Uuid,
    pub gross: String,
    pub amount: String,
    pub rate: String,
    pub net_to_tech: String,
    pub computed_at: chrono::DateTime<chrono::Utc>,
}

impl From<Commission> for CommissionDto {
    fn from(c: Commission) -> Self {
        Self {
            id: c.id,
            order_id: c.order_id,
            technician_id: c.technician_id,
            gross: c.gross.to_string(),
            amount: c.amount.to_string(),
            rate: c.rate.to_string(),
            net_to_tech: c.net_to_tech.to_string(),
            computed_at: c.computed_at,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct PayoutDto {
    pub id: Uuid,
    pub technician_id: Uuid,
    pub order_id: Uuid,
    pub amount: String,
    pub status: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl From<Payout> for PayoutDto {
    fn from(p: Payout) -> Self {
        Self {
            id: p.id,
            technician_id: p.technician_id,
            order_id: p.order_id,
            amount: p.amount.to_string(),
            status: p.status.as_str().to_string(),
            created_at: p.created_at,
            updated_at: p.updated_at,
        }
    }
}

/// POST /api/v1/payments/:id/confirm — capture + commission + payout.
pub async fn confirm_payment(
    State(state): State<AppState>,
    user: AuthnUser,
    Path(id): Path<Uuid>,
    Json(req): Json<ConfirmPaymentRequest>,
) -> Result<Response, Response> {
    let p = state
        .payments
        .find_payment(id)
        .await
        .map_err(payment_err_to_response)?;
    let Some(p) = p else {
        return Ok(not_found("payment not found"));
    };
    if p.customer_id != user.id() && !user.has_role(kokkak_domain::Role::Admin) {
        return Ok(forbidden("not your payment"));
    }
    let result = state
        .payments
        .confirm_payment(ConfirmPaymentInput {
            payment_id: id,
            gateway_ref: req.gateway_ref,
        })
        .await
        .map_err(payment_err_to_response)?;
    Ok((
        StatusCode::OK,
        Json(ApiResponse {
            success: true,
            data: Some(ConfirmPaymentDto {
                payment: PaymentDto::from(result.payment),
                commission: CommissionDto::from(result.commission),
                payout: PayoutDto::from(result.payout),
            }),
            error: None,
            meta: None,
        }),
    )
        .into_response())
}

#[derive(Debug, Deserialize)]
pub struct ListMyQuery {
    pub limit: Option<u32>,
}

/// GET /api/v1/payments/me — list my payments.
pub async fn list_my_payments(
    State(state): State<AppState>,
    user: AuthnUser,
    Query(q): Query<ListMyQuery>,
) -> Result<Response, Response> {
    let limit = q.limit.unwrap_or(20);
    let payments = state
        .payments
        .list_payments_for(user.id(), limit)
        .await
        .map_err(payment_err_to_response)?;
    let items: Vec<PaymentDto> = payments.into_iter().map(PaymentDto::from).collect();
    let meta = PageMeta {
        limit: limit as usize,
        has_next: items.len() as u32 == limit,
        next_cursor: None,
    };
    Ok((StatusCode::OK, paginated(items, meta)).into_response())
}

/// GET /api/v1/payments/:id — fetch one payment.
pub async fn get_payment(
    State(state): State<AppState>,
    user: AuthnUser,
    Path(id): Path<Uuid>,
) -> Result<Response, Response> {
    let p = state
        .payments
        .find_payment(id)
        .await
        .map_err(payment_err_to_response)?;
    let Some(p) = p else {
        return Ok(not_found("payment not found"));
    };
    if p.customer_id != user.id() && !user.has_role(kokkak_domain::Role::Admin) {
        return Ok(forbidden("not your payment"));
    }
    Ok((
        StatusCode::OK,
        Json(ApiResponse {
            success: true,
            data: Some(PaymentDto::from(p)),
            error: None,
            meta: None,
        }),
    )
        .into_response())
}

#[derive(Debug, Deserialize)]
pub struct ListPayoutsQuery {
    pub technician_id: Option<Uuid>,
    pub status: Option<String>,
    pub limit: Option<u32>,
}

/// GET /api/v1/admin/payouts — admin: list payouts.
pub async fn list_payouts_admin(
    State(state): State<AppState>,
    user: AuthnUser,
    Query(q): Query<ListPayoutsQuery>,
) -> Result<Response, Response> {
    if !user.has_role(kokkak_domain::Role::Admin)
        && !user.has_role(kokkak_domain::Role::SuperAdmin)
    {
        return Ok(forbidden("admin required"));
    }
    let limit = q.limit.unwrap_or(50);
    let status = match q.status.as_deref() {
        Some("pending") => Some(PayoutStatus::Pending),
        Some("queued") => Some(PayoutStatus::Queued),
        Some("paid") => Some(PayoutStatus::Paid),
        Some("failed") => Some(PayoutStatus::Failed),
        Some(other) => {
            return Ok(bad_request(&format!("unknown payout status: {other}")));
        }
        None => None,
    };
    let payouts = state
        .payments
        .list_payouts(q.technician_id, status, limit)
        .await
        .map_err(payment_err_to_response)?;
    let items: Vec<PayoutDto> = payouts.into_iter().map(PayoutDto::from).collect();
    let meta = PageMeta {
        limit: limit as usize,
        has_next: items.len() as u32 == limit,
        next_cursor: None,
    };
    Ok((StatusCode::OK, paginated(items, meta)).into_response())
}

/// POST /api/v1/admin/payouts/:id/pay — admin: mark a payout paid.
pub async fn mark_payout_paid_admin(
    State(state): State<AppState>,
    user: AuthnUser,
    Path(id): Path<Uuid>,
) -> Result<Response, Response> {
    if !user.has_role(kokkak_domain::Role::Admin)
        && !user.has_role(kokkak_domain::Role::SuperAdmin)
    {
        return Ok(forbidden("admin required"));
    }
    let p = state
        .payments
        .mark_payout_paid(id)
        .await
        .map_err(payment_err_to_response)?;
    Ok((
        StatusCode::OK,
        Json(ApiResponse {
            success: true,
            data: Some(PayoutDto::from(p)),
            error: None,
            meta: None,
        }),
    )
        .into_response())
}

fn bad_request(message: &str) -> Response {
    let envelope: ApiResponse<()> = ApiResponse {
        success: false,
        data: None,
        error: Some(kokkak_common::error::ApiErrorBody {
            code: "bad_request".into(),
            message: message.into(),
        }),
        meta: None,
    };
    (StatusCode::BAD_REQUEST, Json(envelope)).into_response()
}

fn not_found(message: &str) -> Response {
    let envelope: ApiResponse<()> = ApiResponse {
        success: false,
        data: None,
        error: Some(kokkak_common::error::ApiErrorBody {
            code: "not_found".into(),
            message: message.into(),
        }),
        meta: None,
    };
    (StatusCode::NOT_FOUND, Json(envelope)).into_response()
}

fn forbidden(message: &str) -> Response {
    let envelope: ApiResponse<()> = ApiResponse {
        success: false,
        data: None,
        error: Some(kokkak_common::error::ApiErrorBody {
            code: "forbidden".into(),
            message: message.into(),
        }),
        meta: None,
    };
    (StatusCode::FORBIDDEN, Json(envelope)).into_response()
}

fn payment_err_to_response(e: PaymentError) -> Response {
    use PaymentError::*;
    let (status, code) = match &e {
        NotFound(_) => (StatusCode::NOT_FOUND, "not_found"),
        InvalidAmount(_) => (StatusCode::UNPROCESSABLE_ENTITY, "validation"),
        OrderNotPayable(_) => (StatusCode::CONFLICT, "conflict"),
        Backend(_) => (StatusCode::INTERNAL_SERVER_ERROR, "internal"),
    };
    let envelope: ApiResponse<()> = ApiResponse {
        success: false,
        data: None,
        error: Some(kokkak_common::error::ApiErrorBody {
            code: code.into(),
            message: e.to_string(),
        }),
        meta: None,
    };
    (status, Json(envelope)).into_response()
}

/// Borrow the `PaymentService` type for adapter crates that
/// only have the api lib.
pub type ApiPaymentService = PaymentService;
