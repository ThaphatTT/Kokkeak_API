use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use kokkak_common::i18n::{current_locale, tr};
use kokkak_common::response::{created, ApiResponse};
use kokkak_domain::Permission;
use serde::{Deserialize, Serialize};

use crate::middleware::auth::{assert_scope_admin_page, AuthnUser};
use crate::state::AppState;

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct CreateOrderServiceRequest {
    pub submission_action: Option<i32>,
    pub customer: CustomerInput,
    pub order: OrderInput,
    #[serde(default)]
    pub participants: Vec<ParticipantInput>,
    pub addresses: Vec<AddressInput>,
    pub bodies: Vec<BodyInput>,
    #[serde(default)]
    pub proposal_candidates: Vec<CandidateInput>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct CustomerInput {
    pub owner_user_guid: String,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct OrderInput {
    pub sourcing_mode: i32,
    pub approval_policy: i32,
    pub minimum_approval_count: Option<i32>,
    pub currency: String,
    pub preferred_payment_method: Option<String>,
    pub note: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct ParticipantInput {
    pub user_guid: String,
    pub is_approver: Option<bool>,
    pub is_payer: Option<bool>,
    pub is_viewer: Option<bool>,
    pub invitation_status: Option<i32>,
    pub can_view: Option<bool>,
    pub can_approve_proposal: Option<bool>,
    pub can_cancel: Option<bool>,
    pub can_pay: Option<bool>,
    pub can_confirm_completion: Option<bool>,
    pub can_request_refund: Option<bool>,
    pub can_review: Option<bool>,
    pub can_open_dispute: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct AddressInput {
    pub client_key: String,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub province_guid: Option<String>,
    pub district_guid: Option<String>,
    pub sub_district_guid: Option<String>,
    pub village_guid: Option<String>,
    pub province_name: Option<String>,
    pub district_name: Option<String>,
    pub sub_district_name: Option<String>,
    pub village_name: Option<String>,
    pub postal_code: Option<String>,
    pub address_detail: Option<String>,
    pub is_default: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct BodyInput {
    pub client_key: String,
    pub category_job_service_sub_guid: String,
    pub requested_by_user_guid: Option<String>,
    pub address_client_key: String,
    pub sourcing_mode: Option<i32>,
    pub body_type: Option<i32>,
    pub priority: Option<i32>,
    pub description: Option<String>,
    pub preferred_start_at: Option<String>,
    pub preferred_end_at: Option<String>,
    pub proposal_close_at: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, utoipa::ToSchema)]
pub struct CandidateInput {
    pub client_key: String,
    pub bidder_key: String,
    pub bidder_type: i32,
    pub bidder_owner_user_guid: Option<String>,
    pub bidder_name: Option<String>,
    pub invitation_expired_at: Option<String>,
    pub body_client_keys: Vec<String>,
}

impl CreateOrderServiceRequest {
    pub fn validate(&self) -> Result<(), String> {
        if self.addresses.is_empty() {
            return Err("addresses must have at least 1 item".to_string());
        }
        if self.bodies.is_empty() {
            return Err("bodies must have at least 1 item".to_string());
        }

        let addr_keys: std::collections::HashSet<&str> = self
            .addresses
            .iter()
            .map(|a| a.client_key.as_str())
            .collect();
        for body in &self.bodies {
            if !addr_keys.contains(body.address_client_key.as_str()) {
                return Err(format!(
                    "body.address_client_key '{}' not found in addresses",
                    body.address_client_key
                ));
            }
        }

        if let Some(sa) = self.submission_action {
            if sa != 1 {
                return Err("submission_action must be 1".to_string());
            }
        }

        if !matches!(self.order.sourcing_mode, 1 | 2 | 3) {
            return Err("order.sourcing_mode must be 1, 2, or 3".to_string());
        }

        Ok(())
    }
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct CreateOrderServiceResponse {
    pub order_service_header_guid: String,
    pub order_no: String,
    pub workflow_status: i32,
    pub workflow_status_text: String,
    pub participant_count: i32,
    pub address_count: i32,
    pub body_count: i32,
    pub invitation_count: i32,
}

#[utoipa::path(
    post,
    path = "/api/v1/order-services",
    tag = "order-services",
    request_body = CreateOrderServiceRequest,
    responses(
        (status = 201, description = "Order service created", body = CreateOrderServiceResponse),
        (status = 400, description = "Validation error", body = crate::openapi::ApiError),
        (status = 401, description = "Not authenticated", body = crate::openapi::ApiError),
        (status = 403, description = "Permission denied", body = crate::openapi::ApiError),
        (status = 409, description = "Idempotency conflict", body = crate::openapi::ApiError),
    ),
    security(("bearer_auth" = [])),
    params(
        ("Idempotency-Key" = String, Header, description = "Unique per-request token"),
        ("X-Correlation-ID" = String, Header, description = "Correlation ID for tracing"),
    )
)]
pub async fn create_order_service_admin(
    State(state): State<AppState>,
    user: AuthnUser,
    headers: HeaderMap,
    Json(req): Json<CreateOrderServiceRequest>,
) -> Result<Response, Response> {
    let locale = current_locale();
    assert_scope_admin_page(&user, tr("err_auth.forbidden", &locale, &[]))?;

    if !user
        .has_permission(Permission::JobsCreate, &state.permission_checker)
        .await
    {
        return Err(permission_denied(&state, "JOBS_CREATE"));
    }

    if let Err(msg) = req.validate() {
        return Err(validation_envelope(&state, &msg));
    }

    let idempotency_key = headers
        .get("idempotency-key")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    if idempotency_key.is_empty() {
        return Err(validation_envelope(
            &state,
            "Idempotency-Key header is required",
        ));
    }

    let correlation_id = headers
        .get("x-correlation-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let payload_json = match serde_json::to_string(&req) {
        Ok(j) => j,
        Err(e) => {
            tracing::error!(error = %e, "failed to serialize request");
            return Err(internal_error(&state));
        }
    };

    let actor_guid = user.id().to_string();

    let result = match state
        .admin_order_service
        .create_full(
            &actor_guid,
            &idempotency_key,
            correlation_id.as_deref(),
            &payload_json,
        )
        .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(error = %e, "admin_order_service.create_full failed");
            return Err(map_repo_error(&state, e));
        }
    };

    if !result.success {
        let msg = result.message.clone();
        tracing::warn!(message = %msg, "SP_ADMIN_ORDER_SERVICE_CREATE_FULL returned failure");
        return Err(validation_envelope(&state, &msg));
    }

    let data = result.data.ok_or_else(|| {
        tracing::error!("SP success but missing data");
        internal_error(&state)
    })?;

    let resp = CreateOrderServiceResponse {
        order_service_header_guid: data.order_service_header_guid,
        order_no: data.order_no,
        workflow_status: data.workflow_status,
        workflow_status_text: data.workflow_status_text,
        participant_count: data.participant_count,
        address_count: data.address_count,
        body_count: data.body_count,
        invitation_count: data.invitation_count,
    };

    Ok((StatusCode::CREATED, created(resp)).into_response())
}

fn permission_denied(_state: &AppState, code: &str) -> Response {
    let locale = current_locale();
    let envelope: ApiResponse<()> = ApiResponse {
        success: false,
        data: None,
        error: Some(kokkak_common::error::ApiErrorBody {
            code: "permission_denied".into(),
            message: tr("err_auth.permission_denied", &locale, &[code]),
        }),
        meta: None,
    };
    (StatusCode::FORBIDDEN, Json(envelope)).into_response()
}

fn validation_envelope(_state: &AppState, msg: &str) -> Response {
    let locale = current_locale();
    let envelope: ApiResponse<()> = ApiResponse {
        success: false,
        data: None,
        error: Some(kokkak_common::error::ApiErrorBody {
            code: "validation".into(),
            message: tr("err_auth.validation", &locale, &[msg]),
        }),
        meta: None,
    };
    (StatusCode::UNPROCESSABLE_ENTITY, Json(envelope)).into_response()
}

fn internal_error(_state: &AppState) -> Response {
    let locale = current_locale();
    let envelope: ApiResponse<()> = ApiResponse {
        success: false,
        data: None,
        error: Some(kokkak_common::error::ApiErrorBody {
            code: "internal".into(),
            message: tr("err.internal", &locale, &[]),
        }),
        meta: None,
    };
    (StatusCode::INTERNAL_SERVER_ERROR, Json(envelope)).into_response()
}

fn map_repo_error(_state: &AppState, err: kokkak_domain::RepoError) -> Response {
    let locale = current_locale();
    match err {
        kokkak_domain::RepoError::NotFound(msg) => {
            let envelope: ApiResponse<()> = ApiResponse {
                success: false,
                data: None,
                error: Some(kokkak_common::error::ApiErrorBody {
                    code: "not_found".into(),
                    message: tr("err_repo.not_found", &locale, &[&msg]),
                }),
                meta: None,
            };
            (StatusCode::NOT_FOUND, Json(envelope)).into_response()
        }
        kokkak_domain::RepoError::Conflict(msg) => {
            let envelope: ApiResponse<()> = ApiResponse {
                success: false,
                data: None,
                error: Some(kokkak_common::error::ApiErrorBody {
                    code: "conflict".into(),
                    message: tr("err_repo.conflict", &locale, &[&msg]),
                }),
                meta: None,
            };
            (StatusCode::CONFLICT, Json(envelope)).into_response()
        }
        kokkak_domain::RepoError::Backend(msg) => {
            tracing::error!(error = %msg, "database error");
            let envelope: ApiResponse<()> = ApiResponse {
                success: false,
                data: None,
                error: Some(kokkak_common::error::ApiErrorBody {
                    code: "internal".into(),
                    message: tr("err.internal", &locale, &[]),
                }),
                meta: None,
            };
            (StatusCode::INTERNAL_SERVER_ERROR, Json(envelope)).into_response()
        }
    }
}
