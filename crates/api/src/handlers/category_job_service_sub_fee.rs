use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use kokkak_common::i18n::{current_locale, tr, tr_with_repo};
use kokkak_common::response::{created, ok, ApiResponse};
use kokkak_domain::LocalizedError;
use serde::{Deserialize, Serialize};

use crate::middleware::auth::AuthnUser;
use crate::state::AppState;

#[derive(Debug, Deserialize, utoipa::IntoParams, utoipa::ToSchema)]
pub struct ListCategoryJobServiceSubFeeQuery {
    #[serde(default)]
    pub category_job_service_sub_fee_guid: Option<String>,

    #[serde(default)]
    pub keyword: Option<String>,

    #[serde(default)]
    pub status: Option<i32>,

    #[serde(default)]
    pub locale: Option<String>,

    #[serde(default)]
    pub include_deleted: Option<bool>,

    #[serde(default)]
    pub page: Option<u32>,

    #[serde(default)]
    pub page_size: Option<u32>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ListCategoryJobServiceSubFeeMeta {
    pub total_count: i64,

    pub page: u32,

    pub page_size: u32,

    pub total_page: u32,

    pub has_next: bool,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ListCategoryJobServiceSubFeeResponse {
    pub items: Vec<kokkak_domain::CategoryJobServiceSubFeeAdminRow>,

    pub meta: ListCategoryJobServiceSubFeeMeta,
}

#[utoipa::path(
    get,
    path = "/api/v1/admin/category-job-service-sub-fees",
    tag = "category-job-service-sub-fee",
    params(ListCategoryJobServiceSubFeeQuery),
    responses(
        (status = 200, description = "Paginated, localized list of sub-service fees for the web admin", body = ListCategoryJobServiceSubFeeResponse),
        (status = 401, description = "Not authenticated", body = crate::openapi::ApiError),
        (status = 403, description = "Missing SERVICE_VIEW permission", body = crate::openapi::ApiError),
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_category_job_service_sub_fees_admin(
    State(state): State<AppState>,
    user: AuthnUser,
    Query(q): Query<ListCategoryJobServiceSubFeeQuery>,
) -> Result<Response, Response> {
    if !user
        .has_permission(
            kokkak_domain::Permission::PageServiceView,
            &state.permission_checker,
        )
        .await
    {
        return Err(permission_denied(&state, "SERVICE_VIEW"));
    }

    let locale = q.locale.clone().unwrap_or_else(current_locale);
    let input = kokkak_domain::CategoryJobServiceSubFeeListInput {
        category_job_service_sub_fee_guid: q
            .category_job_service_sub_fee_guid
            .as_ref()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        keyword: q
            .keyword
            .as_ref()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        status: q.status,
        locale: Some(locale),
        include_deleted: q.include_deleted,
        page: q.page,
        page_size: q.page_size,
    };

    let page_data = match state.category_job_service_sub_fee.list(input).await {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!(error = %e, "category_job_service_sub_fee.list failed");
            return Err(repo_error_to_response(&state, e).await);
        }
    };

    let total_page = page_data.total_page;
    let has_next = page_data.page < total_page && total_page > 0;

    let resp = ListCategoryJobServiceSubFeeResponse {
        items: page_data.items,
        meta: ListCategoryJobServiceSubFeeMeta {
            total_count: page_data.total_count,
            page: page_data.page,
            page_size: page_data.page_size,
            total_page,
            has_next,
        },
    };
    Ok((StatusCode::OK, ok(resp)).into_response())
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct CreateCategoryJobServiceSubFeeRequest {
    #[serde(default)]
    pub category_job_service_sub_fee_guid: Option<String>,

    #[serde(default)]
    pub category_job_service_sub_fee_header_la: Option<String>,

    #[serde(default)]
    pub category_job_service_sub_fee_description_la: Option<String>,

    #[serde(default)]
    pub category_job_service_sub_fee_header_en: Option<String>,

    #[serde(default)]
    pub category_job_service_sub_fee_description_en: Option<String>,

    #[serde(default)]
    pub category_job_service_sub_fee_header_th: Option<String>,

    #[serde(default)]
    pub category_job_service_sub_fee_description_th: Option<String>,

    #[serde(default)]
    pub category_job_service_sub_fee_header_zh: Option<String>,

    #[serde(default)]
    pub category_job_service_sub_fee_description_zh: Option<String>,

    pub category_job_service_sub_fee_price: String,

    pub category_job_service_sub_fee_status: i32,

    #[serde(default)]
    pub category_job_service_sub_fee_icon: Option<String>,
}

impl CreateCategoryJobServiceSubFeeRequest {
    pub fn validate(&self) -> Result<(), String> {
        if self.category_job_service_sub_fee_price.trim().is_empty() {
            return Err("category_job_service_sub_fee_price is required".to_string());
        }
        if !matches!(self.category_job_service_sub_fee_status, 0 | 1) {
            return Err("category_job_service_sub_fee_status must be 0 or 1".to_string());
        }
        if let Some(guid) = self.category_job_service_sub_fee_guid.as_deref() {
            if guid.trim().is_empty() {
                return Err(
                    "category_job_service_sub_fee_guid must not be empty if provided".to_string(),
                );
            }
        }
        Ok(())
    }
}

#[utoipa::path(
    post,
    path = "/api/v1/admin/category-job-service-sub-fees",
    tag = "category-job-service-sub-fee",
    request_body = CreateCategoryJobServiceSubFeeRequest,
    responses(
        (status = 201, description = "Sub-service fee created", body = kokkak_domain::CategoryJobServiceSubFeeCreateResult),
        (status = 400, description = "Validation error", body = crate::openapi::ApiError),
        (status = 401, description = "Not authenticated", body = crate::openapi::ApiError),
        (status = 403, description = "Missing SERVICE_CREATE permission", body = crate::openapi::ApiError),
        (status = 409, description = "Duplicate GUID", body = crate::openapi::ApiError),
        (status = 500, description = "Internal error", body = crate::openapi::ApiError),
    ),
    security(("bearer_auth" = []))
)]
pub async fn create_category_job_service_sub_fee_admin(
    State(state): State<AppState>,
    user: AuthnUser,
    Json(req): Json<CreateCategoryJobServiceSubFeeRequest>,
) -> Result<Response, Response> {
    if !user
        .has_permission(
            kokkak_domain::Permission::ServiceCreate,
            &state.permission_checker,
        )
        .await
    {
        return Err(permission_denied(
            &state,
            kokkak_domain::Permission::ServiceCreate.code(),
        ));
    }
    if let Err(msg) = req.validate() {
        return Err(validation_envelope(&state, &msg));
    }

    let price: rust_decimal::Decimal = match req.category_job_service_sub_fee_price.trim().parse() {
        Ok(d) => d,
        Err(_) => {
            return Err(validation_envelope(
                &state,
                "category_job_service_sub_fee_price must be a decimal number",
            ));
        }
    };

    let actor = user.id().to_string();
    let input = kokkak_domain::CategoryJobServiceSubFeeCreateInput {
        category_job_service_sub_fee_guid: req
            .category_job_service_sub_fee_guid
            .as_ref()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        category_job_service_sub_fee_header_la: req.category_job_service_sub_fee_header_la.clone(),
        category_job_service_sub_fee_description_la: req
            .category_job_service_sub_fee_description_la
            .clone(),
        category_job_service_sub_fee_header_en: req.category_job_service_sub_fee_header_en.clone(),
        category_job_service_sub_fee_description_en: req
            .category_job_service_sub_fee_description_en
            .clone(),
        category_job_service_sub_fee_header_th: req.category_job_service_sub_fee_header_th.clone(),
        category_job_service_sub_fee_description_th: req
            .category_job_service_sub_fee_description_th
            .clone(),
        category_job_service_sub_fee_header_zh: req.category_job_service_sub_fee_header_zh.clone(),
        category_job_service_sub_fee_description_zh: req
            .category_job_service_sub_fee_description_zh
            .clone(),
        category_job_service_sub_fee_price: price,
        category_job_service_sub_fee_status: req.category_job_service_sub_fee_status,
        category_job_service_sub_fee_icon: req.category_job_service_sub_fee_icon.clone(),
        create_by: actor,
    };

    let result = match state.category_job_service_sub_fee.create(input).await {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(error = %e, "category_job_service_sub_fee.create failed");
            return Err(repo_error_to_response(&state, e).await);
        }
    };

    if result.success {
        return Ok((StatusCode::CREATED, created(result)).into_response());
    }

    let (status, code_str) = match result.code.as_str() {
        "DUPLICATE_GUID" => (StatusCode::CONFLICT, "conflict"),
        _ => (StatusCode::INTERNAL_SERVER_ERROR, "internal"),
    };
    let envelope: ApiResponse<()> = ApiResponse {
        success: false,
        data: None,
        error: Some(kokkak_common::error::ApiErrorBody {
            code: result.code.clone(),
            message: result.message.clone(),
        }),
        meta: None,
    };
    Ok((status, Json(envelope)).into_response())
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct UpdateCategoryJobServiceSubFeeRequest {
    #[serde(default)]
    pub category_job_service_sub_fee_header_la: Option<String>,

    #[serde(default)]
    pub category_job_service_sub_fee_description_la: Option<String>,

    #[serde(default)]
    pub category_job_service_sub_fee_header_en: Option<String>,

    #[serde(default)]
    pub category_job_service_sub_fee_description_en: Option<String>,

    #[serde(default)]
    pub category_job_service_sub_fee_header_th: Option<String>,

    #[serde(default)]
    pub category_job_service_sub_fee_description_th: Option<String>,

    #[serde(default)]
    pub category_job_service_sub_fee_header_zh: Option<String>,

    #[serde(default)]
    pub category_job_service_sub_fee_description_zh: Option<String>,

    #[serde(default)]
    pub category_job_service_sub_fee_price: Option<String>,

    #[serde(default)]
    pub category_job_service_sub_fee_status: Option<i32>,

    #[serde(default)]
    pub category_job_service_sub_fee_icon: Option<String>,
}

impl UpdateCategoryJobServiceSubFeeRequest {
    pub fn validate(&self) -> Result<(), String> {
        if let Some(s) = self.category_job_service_sub_fee_status {
            if !matches!(s, 0 | 1) {
                return Err("category_job_service_sub_fee_status must be 0 or 1".to_string());
            }
        }
        for (name, val) in [
            (
                "category_job_service_sub_fee_header_la",
                &self.category_job_service_sub_fee_header_la,
            ),
            (
                "category_job_service_sub_fee_header_en",
                &self.category_job_service_sub_fee_header_en,
            ),
            (
                "category_job_service_sub_fee_header_th",
                &self.category_job_service_sub_fee_header_th,
            ),
            (
                "category_job_service_sub_fee_header_zh",
                &self.category_job_service_sub_fee_header_zh,
            ),
            (
                "category_job_service_sub_fee_icon",
                &self.category_job_service_sub_fee_icon,
            ),
        ] {
            if let Some(v) = val {
                if v.chars().count() > 255 {
                    return Err(format!("{name} must be <= 255 characters"));
                }
            }
        }
        Ok(())
    }
}

#[utoipa::path(
        put,
        path = "/api/v1/admin/category-job-service-sub-fees/{guid}",
        tag = "category-job-service-sub-fee",
        params(
            ("guid" = String, Path, description = "Category Job Service Sub Fee GUID (UUID)"),
        ),
        request_body = UpdateCategoryJobServiceSubFeeRequest,
        responses(
            (status = 200, description = "Sub-service fee updated", body = kokkak_domain::CategoryJobServiceSubFeeUpdateResult),
            (status = 400, description = "Validation error", body = crate::openapi::ApiError),
            (status = 401, description = "Not authenticated", body = crate::openapi::ApiError),
            (status = 403, description = "Missing SERVICE_UPDATE permission", body = crate::openapi::ApiError),
            (status = 404, description = "Fee not found", body = crate::openapi::ApiError),
            (status = 500, description = "Internal error", body = crate::openapi::ApiError),
        ),
        security(("bearer_auth" = []))
    )]
pub async fn update_category_job_service_sub_fee_admin(
    State(state): State<AppState>,
    user: AuthnUser,
    Path(guid): Path<String>,
    Json(req): Json<UpdateCategoryJobServiceSubFeeRequest>,
) -> Result<Response, Response> {
    if !user
        .has_permission(
            kokkak_domain::Permission::ServiceUpdate,
            &state.permission_checker,
        )
        .await
    {
        return Err(permission_denied(
            &state,
            kokkak_domain::Permission::ServiceUpdate.code(),
        ));
    }
    if let Err(msg) = req.validate() {
        return Err(validation_envelope(&state, &msg));
    }

    let price: Option<rust_decimal::Decimal> = match req
        .category_job_service_sub_fee_price
        .as_deref()
        .map(str::trim)
    {
        Some(s) if !s.is_empty() => match s.parse() {
            Ok(d) => Some(d),
            Err(_) => {
                return Err(validation_envelope(
                    &state,
                    "category_job_service_sub_fee_price must be a decimal number",
                ));
            }
        },
        _ => None,
    };

    let actor = user.id().to_string();
    let input = kokkak_domain::CategoryJobServiceSubFeeUpdateInput {
        category_job_service_sub_fee_guid: guid.trim().to_string(),
        category_job_service_sub_fee_header_la: req
            .category_job_service_sub_fee_header_la
            .as_ref()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        category_job_service_sub_fee_description_la: req
            .category_job_service_sub_fee_description_la
            .as_ref()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        category_job_service_sub_fee_header_en: req
            .category_job_service_sub_fee_header_en
            .as_ref()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        category_job_service_sub_fee_description_en: req
            .category_job_service_sub_fee_description_en
            .as_ref()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        category_job_service_sub_fee_header_th: req
            .category_job_service_sub_fee_header_th
            .as_ref()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        category_job_service_sub_fee_description_th: req
            .category_job_service_sub_fee_description_th
            .as_ref()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        category_job_service_sub_fee_header_zh: req
            .category_job_service_sub_fee_header_zh
            .as_ref()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        category_job_service_sub_fee_description_zh: req
            .category_job_service_sub_fee_description_zh
            .as_ref()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        category_job_service_sub_fee_price: price,
        category_job_service_sub_fee_status: req.category_job_service_sub_fee_status,
        category_job_service_sub_fee_icon: req
            .category_job_service_sub_fee_icon
            .as_ref()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        update_by: actor,
    };

    let result = match state.category_job_service_sub_fee.update(input).await {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(error = %e, "category_job_service_sub_fee.update failed");
            return Err(repo_error_to_response(&state, e).await);
        }
    };

    if result.success {
        return Ok((StatusCode::OK, ok(result)).into_response());
    }

    let status = match result.code.as_str() {
        "NOT_FOUND" | "GUID_REQUIRED" => StatusCode::NOT_FOUND,
        "INVALID_STATUS" | "INVALID_PRICE" | "PRICE_OUT_OF_RANGE" | "HEADER_TOO_LONG" => {
            StatusCode::UNPROCESSABLE_ENTITY
        }
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    let envelope: ApiResponse<()> = ApiResponse {
        success: false,
        data: None,
        error: Some(kokkak_common::error::ApiErrorBody {
            code: result.code.clone(),
            message: result.message.clone(),
        }),
        meta: None,
    };
    Ok((status, Json(envelope)).into_response())
}

async fn repo_error_to_response(state: &AppState, err: kokkak_domain::RepoError) -> Response {
    use kokkak_domain::RepoError::*;
    let (status, code) = match &err {
        NotFound(_) => (StatusCode::NOT_FOUND, "not_found"),
        Conflict(_) => (StatusCode::CONFLICT, "conflict"),
        Backend(_) => (StatusCode::INTERNAL_SERVER_ERROR, "internal"),
    };
    let locale = current_locale();
    let message = tr_with_repo(&*state.translation, &locale, err.l10n_key(), &[]).await;
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

fn permission_denied(state: &AppState, code: &str) -> Response {
    let locale = current_locale();
    let _ = state;
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

fn validation_envelope(state: &AppState, msg: &str) -> Response {
    let locale = current_locale();
    let _ = state;
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
