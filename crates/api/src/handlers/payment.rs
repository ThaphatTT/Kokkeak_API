//! Payment HTTP handlers (M9 + M11 i18n).
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
use kokkak_common::i18n::{current_locale, tr, tr_with_repo};
use kokkak_common::response::{paginated, ApiResponse, PageMeta};
use kokkak_domain::{
    Commission, LocalizedError, Payment, PaymentError, Payout, PayoutStatus, Role,
};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::middleware::auth::{assert_role, AuthnUser};
use crate::state::AppState;

#[derive(Debug, Deserialize, utoipa::ToSchema)]
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
#[utoipa::path(
    post,
    path = "/api/v1/payments",
    tag = "payments",
    request_body = CreatePaymentRequest,
    responses(
        (status = 201, description = "Payment intent created", body = kokkak_domain::Payment),
        (status = 400, description = "Idempotency-Key required", body = crate::openapi::ApiError),
        (status = 401, description = "Not authenticated", body = crate::openapi::ApiError),
        (status = 403, description = "Not a customer", body = crate::openapi::ApiError),
        (status = 422, description = "Validation error", body = crate::openapi::ApiError),
    ),
    security(("bearer_auth" = [])),
    params(
        ("Idempotency-Key" = String, Header, description = "Unique per-request token. Mobile retries MUST send the same key to dedupe the payment."),
    )
)]
pub async fn create_payment(
    State(state): State<AppState>,
    user: AuthnUser,
    Json(req): Json<CreatePaymentRequest>,
) -> Result<Response, Response> {
    let locale = current_locale();
    let role_msg = tr_with_repo(
        &*state.translation,
        &locale,
        "err_auth.role_required",
        &[Role::Customer.as_str()],
    )
    .await;
    assert_role(&user, Role::Customer, role_msg)?;
    let amount = match req.amount {
        Some(s) => match s.parse::<Decimal>() {
            Ok(d) => Some(d),
            Err(_) => {
                let msg = tr("err_payment.bad_amount", &locale, &[]);
                return Ok(bad_request(msg));
            }
        },
        None => None,
    };
    let input = CreatePaymentInput {
        order_id: req.order_id,
        customer_id: user.id(),
        amount,
    };
    let p = match state.payments.create_payment(input).await {
        Ok(v) => v,
        Err(e) => return Err(payment_err_to_response(e, &state).await),
    };
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

#[derive(Debug, Deserialize, utoipa::ToSchema)]
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
#[utoipa::path(
    post,
    path = "/api/v1/payments/{id}/confirm",
    tag = "payments",
    params(
        ("id" = Uuid, Path, description = "Payment id"),
    ),
    request_body = ConfirmPaymentRequest,
    responses(
        (status = 200, description = "Payment confirmed", body = kokkak_domain::Payment),
        (status = 401, description = "Not authenticated", body = crate::openapi::ApiError),
        (status = 404, description = "Payment not found", body = crate::openapi::ApiError),
        (status = 422, description = "Validation error", body = crate::openapi::ApiError),
    ),
    security(("bearer_auth" = []))
)]
pub async fn confirm_payment(
    State(state): State<AppState>,
    user: AuthnUser,
    Path(id): Path<Uuid>,
    Json(req): Json<ConfirmPaymentRequest>,
) -> Result<Response, Response> {
    let locale = current_locale();
    let p = match state.payments.find_payment(id).await {
        Ok(v) => v,
        Err(e) => return Err(payment_err_to_response(e, &state).await),
    };
    let Some(p) = p else {
        let msg = tr("err_payment.not_found_msg", &locale, &[]);
        return Ok(not_found(msg));
    };
    if p.customer_id != user.id() && !user.has_role(Role::Admin) {
        let msg = tr("err_payment.not_yours", &locale, &[]);
        return Ok(forbidden(msg));
    }
    let result = match state
        .payments
        .confirm_payment(ConfirmPaymentInput {
            payment_id: id,
            gateway_ref: req.gateway_ref,
        })
        .await
    {
        Ok(v) => v,
        Err(e) => return Err(payment_err_to_response(e, &state).await),
    };
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

#[derive(Debug, Deserialize, utoipa::ToSchema, utoipa::IntoParams)]
pub struct ListMyQuery {
    pub limit: Option<u32>,
}

/// GET /api/v1/payments/me — list my payments.
#[utoipa::path(
    get,
    path = "/api/v1/payments/me",
    tag = "payments",
    params(ListMyQuery),
    responses(
        (status = 200, description = "Current user's payments", body = Vec<kokkak_domain::Payment>),
        (status = 401, description = "Not authenticated", body = crate::openapi::ApiError),
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_my_payments(
    State(state): State<AppState>,
    user: AuthnUser,
    Query(q): Query<ListMyQuery>,
) -> Result<Response, Response> {
    let limit = q.limit.unwrap_or(20);
    let payments = match state.payments.list_payments_for(user.id(), limit).await {
        Ok(v) => v,
        Err(e) => return Err(payment_err_to_response(e, &state).await),
    };
    let items: Vec<PaymentDto> = payments.into_iter().map(PaymentDto::from).collect();
    let meta = PageMeta {
        limit: limit as usize,
        has_next: items.len() as u32 == limit,
        next_cursor: None,
    };
    Ok((StatusCode::OK, paginated(items, meta)).into_response())
}

/// GET /api/v1/payments/:id — fetch one payment.
#[utoipa::path(
    get,
    path = "/api/v1/payments/{id}",
    tag = "payments",
    params(
        ("id" = Uuid, Path, description = "Payment id"),
    ),
    responses(
        (status = 200, description = "Payment found", body = kokkak_domain::Payment),
        (status = 401, description = "Not authenticated", body = crate::openapi::ApiError),
        (status = 404, description = "Payment not found", body = crate::openapi::ApiError),
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_payment(
    State(state): State<AppState>,
    user: AuthnUser,
    Path(id): Path<Uuid>,
) -> Result<Response, Response> {
    let locale = current_locale();
    let p = match state.payments.find_payment(id).await {
        Ok(v) => v,
        Err(e) => return Err(payment_err_to_response(e, &state).await),
    };
    let Some(p) = p else {
        let msg = tr("err_payment.not_found_msg", &locale, &[]);
        return Ok(not_found(msg));
    };
    if p.customer_id != user.id() && !user.has_role(Role::Admin) {
        let msg = tr("err_payment.not_yours", &locale, &[]);
        return Ok(forbidden(msg));
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

#[derive(Debug, Deserialize, utoipa::ToSchema, utoipa::IntoParams)]
pub struct ListPayoutsQuery {
    pub technician_id: Option<Uuid>,
    pub status: Option<String>,
    pub limit: Option<u32>,
}

/// GET /api/v1/admin/payouts — admin: list payouts.
#[utoipa::path(
    get,
    path = "/api/v1/admin/payouts",
    tag = "admin",
    params(ListPayoutsQuery),
    responses(
        (status = 200, description = "Payouts (admin view)", body = Vec<kokkak_domain::Payout>),
        (status = 401, description = "Not authenticated", body = crate::openapi::ApiError),
        (status = 403, description = "Not an admin", body = crate::openapi::ApiError),
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_payouts_admin(
    State(state): State<AppState>,
    user: AuthnUser,
    Query(q): Query<ListPayoutsQuery>,
) -> Result<Response, Response> {
    let locale = current_locale();
    if !user.has_role(Role::Admin) && !user.has_role(Role::SuperAdmin) {
        let msg = tr("err_auth.admin_required", &locale, &[]);
        return Ok(forbidden(msg));
    }
    let limit = q.limit.unwrap_or(50);
    let status = match q.status.as_deref() {
        Some("pending") => Some(PayoutStatus::Pending),
        Some("queued") => Some(PayoutStatus::Queued),
        Some("paid") => Some(PayoutStatus::Paid),
        Some("failed") => Some(PayoutStatus::Failed),
        Some(other) => {
            let args = [other];
            let msg = tr("err_payment.unknown_payout_status", &locale, &args);
            return Ok(bad_request(msg));
        }
        None => None,
    };
    let payouts = match state
        .payments
        .list_payouts(q.technician_id, status, limit)
        .await
    {
        Ok(v) => v,
        Err(e) => return Err(payment_err_to_response(e, &state).await),
    };
    let items: Vec<PayoutDto> = payouts.into_iter().map(PayoutDto::from).collect();
    let meta = PageMeta {
        limit: limit as usize,
        has_next: items.len() as u32 == limit,
        next_cursor: None,
    };
    Ok((StatusCode::OK, paginated(items, meta)).into_response())
}

/// POST /api/v1/admin/payouts/:id/pay — admin: mark a payout paid.
#[utoipa::path(
    post,
    path = "/api/v1/admin/payouts/{id}/pay",
    tag = "admin",
    params(
        ("id" = Uuid, Path, description = "Payout id"),
    ),
    responses(
        (status = 200, description = "Payout marked paid", body = kokkak_domain::Payout),
        (status = 401, description = "Not authenticated", body = crate::openapi::ApiError),
        (status = 403, description = "Not an admin", body = crate::openapi::ApiError),
        (status = 404, description = "Payout not found", body = crate::openapi::ApiError),
    ),
    security(("bearer_auth" = []))
)]
pub async fn mark_payout_paid_admin(
    State(state): State<AppState>,
    user: AuthnUser,
    Path(id): Path<Uuid>,
) -> Result<Response, Response> {
    let locale = current_locale();
    if !user.has_role(Role::Admin) && !user.has_role(Role::SuperAdmin) {
        let msg = tr("err_auth.admin_required", &locale, &[]);
        return Ok(forbidden(msg));
    }
    let p = match state.payments.mark_payout_paid(id).await {
        Ok(v) => v,
        Err(e) => return Err(payment_err_to_response(e, &state).await),
    };
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

fn bad_request(message: String) -> Response {
    let envelope: ApiResponse<()> = ApiResponse {
        success: false,
        data: None,
        error: Some(kokkak_common::error::ApiErrorBody {
            code: "bad_request".into(),
            message,
        }),
        meta: None,
    };
    (StatusCode::BAD_REQUEST, Json(envelope)).into_response()
}

fn not_found(message: String) -> Response {
    let envelope: ApiResponse<()> = ApiResponse {
        success: false,
        data: None,
        error: Some(kokkak_common::error::ApiErrorBody {
            code: "not_found".into(),
            message,
        }),
        meta: None,
    };
    (StatusCode::NOT_FOUND, Json(envelope)).into_response()
}

fn forbidden(message: String) -> Response {
    let envelope: ApiResponse<()> = ApiResponse {
        success: false,
        data: None,
        error: Some(kokkak_common::error::ApiErrorBody {
            code: "forbidden".into(),
            message,
        }),
        meta: None,
    };
    (StatusCode::FORBIDDEN, Json(envelope)).into_response()
}

async fn payment_err_to_response(e: PaymentError, state: &AppState) -> Response {
    use PaymentError::*;
    let (status, code) = match &e {
        NotFound(_) => (StatusCode::NOT_FOUND, "not_found"),
        InvalidAmount(_) => (StatusCode::UNPROCESSABLE_ENTITY, "validation"),
        OrderNotPayable(_) => (StatusCode::CONFLICT, "conflict"),
        Backend(_) => (StatusCode::INTERNAL_SERVER_ERROR, "internal"),
    };
    let locale = current_locale();
    let args: Vec<String> = e.l10n_args();
    let args_ref: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let message = tr_with_repo(&*state.translation, &locale, e.l10n_key(), &args_ref).await;
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

/// Borrow the `PaymentService` type for adapter crates that
/// only have the api lib.
pub type ApiPaymentService = PaymentService;
