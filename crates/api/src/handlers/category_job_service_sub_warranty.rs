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

use crate::middleware::auth::{assert_scope, AuthnUser};
use crate::state::AppState;

#[derive(Debug, Deserialize, utoipa::IntoParams, utoipa::ToSchema)]
pub struct ListCategoryJobServiceSubWarrantyQuery {
    #[serde(default)]
    pub category_job_service_sub_warranty_guid: Option<String>,

    #[serde(default)]
    pub keyword: Option<String>,

    #[serde(default)]
    pub status: Option<i32>,

    #[serde(default)]
    pub locale: Option<String>,

    #[serde(default)]
    pub page: Option<u32>,

    #[serde(default)]
    pub page_size: Option<u32>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ListCategoryJobServiceSubWarrantyMeta {
    pub total_count: i64,

    pub page: u32,

    pub page_size: u32,

    pub total_page: u32,

    pub has_next: bool,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ListCategoryJobServiceSubWarrantyResponse {
    pub items: Vec<kokkak_domain::CategoryJobServiceSubWarrantyDetailRow>,

    pub meta: ListCategoryJobServiceSubWarrantyMeta,
}

#[utoipa::path(
    get,
    path = "/api/v1/category-job-service-sub-warranties",
    tag = "category-job-service-sub-warranty",
    params(ListCategoryJobServiceSubWarrantyQuery),
    responses(
        (status = 200, description = "Paginated list of sub-service warranties", body = ListCategoryJobServiceSubWarrantyResponse),
        (status = 401, description = "Not authenticated", body = crate::openapi::ApiError),
        (status = 403, description = "Permission denied", body = crate::openapi::ApiError),
        (status = 422, description = "Validation error", body = crate::openapi::ApiError),
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_category_job_service_sub_warranties(
    State(state): State<AppState>,
    user: AuthnUser,
    Query(q): Query<ListCategoryJobServiceSubWarrantyQuery>,
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

    if let Some(locale) = q.locale.as_deref() {
        let normalized = locale.to_ascii_lowercase();
        if !matches!(normalized.as_str(), "la" | "lo" | "en" | "th" | "zh") {
            return Err(validation_envelope(
                &state,
                "locale must be one of: th, en, la, lo, zh",
            ));
        }
    }
    if let Some(s) = q.status {
        if !matches!(s, 0 | 1) {
            return Err(validation_envelope(&state, "status must be 0 or 1"));
        }
    }

    let locale = q.locale.clone().unwrap_or_else(current_locale);

    let input = kokkak_domain::CategoryJobServiceSubWarrantyListInput {
        category_job_service_sub_warranty_guid: q
            .category_job_service_sub_warranty_guid
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        keyword: q
            .keyword
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        status: q.status,
        locale: Some(locale),
        page: q.page,
        page_size: q.page_size,
    };

    let page_data = match state.category_job_service_sub_warranty.list(input).await {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!(error = %e, "category_job_service_sub_warranty.list failed");
            return Err(repo_error_to_response(&state, e).await);
        }
    };

    let total_page = page_data.total_page;
    let has_next = page_data.page < total_page && total_page > 0;

    let resp = ListCategoryJobServiceSubWarrantyResponse {
        items: page_data.items,
        meta: ListCategoryJobServiceSubWarrantyMeta {
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
pub struct CreateCategoryJobServiceSubWarrantyRequest {
    #[serde(default)]
    pub category_job_service_sub_warranty_guid: Option<String>,

    #[serde(default)]
    pub category_job_service_sub_warranty_description_la: Option<String>,

    #[serde(default)]
    pub category_job_service_sub_warranty_description_en: Option<String>,

    #[serde(default)]
    pub category_job_service_sub_warranty_description_th: Option<String>,

    #[serde(default)]
    pub category_job_service_sub_warranty_description_zh: Option<String>,

    #[serde(default)]
    pub category_job_service_sub_warranty_warranty_amount_day: i32,

    #[serde(default)]
    pub category_job_service_sub_warranty_status: i32,

    #[serde(default)]
    pub category_job_service_sub_warranty_icon: Option<String>,
}

impl CreateCategoryJobServiceSubWarrantyRequest {
    pub fn validate(&self) -> Result<(), String> {
        if !matches!(self.category_job_service_sub_warranty_status, 0 | 1) {
            return Err("category_job_service_sub_warranty_status must be 0 or 1".to_string());
        }
        if self.category_job_service_sub_warranty_warranty_amount_day < 0 {
            return Err(
                "category_job_service_sub_warranty_warranty_amount_day must be >= 0".to_string(),
            );
        }
        if let Some(guid) = self.category_job_service_sub_warranty_guid.as_deref() {
            if guid.trim().is_empty() {
                return Err(
                    "category_job_service_sub_warranty_guid must not be empty if provided"
                        .to_string(),
                );
            }
        }
        Ok(())
    }
}

#[utoipa::path(
    post,
    path = "/api/v1/category-job-service-sub-warranties",
    tag = "category-job-service-sub-warranty",
    request_body = CreateCategoryJobServiceSubWarrantyRequest,
    responses(
        (status = 201, description = "Sub-service warranty created", body = kokkak_domain::CategoryJobServiceSubWarrantyCreateResult),
        (status = 400, description = "Validation error", body = crate::openapi::ApiError),
        (status = 401, description = "Not authenticated", body = crate::openapi::ApiError),
        (status = 403, description = "Missing SERVICE_CREATE permission", body = crate::openapi::ApiError),
        (status = 409, description = "Duplicate GUID", body = crate::openapi::ApiError),
        (status = 500, description = "Internal error", body = crate::openapi::ApiError),
    ),
    security(("bearer_auth" = []))
)]
pub async fn create_category_job_service_sub_warranty_admin(
    State(state): State<AppState>,
    user: AuthnUser,
    Json(req): Json<CreateCategoryJobServiceSubWarrantyRequest>,
) -> Result<Response, Response> {
    let locale = current_locale();
    assert_scope(&user, "admin_page", tr("err_auth.forbidden", &locale, &[]))?;

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

    let actor = user.id().to_string();
    let input = kokkak_domain::CategoryJobServiceSubWarrantyCreateInput {
        category_job_service_sub_warranty_guid: req
            .category_job_service_sub_warranty_guid
            .as_ref()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        category_job_service_sub_warranty_description_la: req
            .category_job_service_sub_warranty_description_la
            .clone(),
        category_job_service_sub_warranty_description_en: req
            .category_job_service_sub_warranty_description_en
            .clone(),
        category_job_service_sub_warranty_description_th: req
            .category_job_service_sub_warranty_description_th
            .clone(),
        category_job_service_sub_warranty_description_zh: req
            .category_job_service_sub_warranty_description_zh
            .clone(),
        category_job_service_sub_warranty_warranty_amount_day: req
            .category_job_service_sub_warranty_warranty_amount_day,
        category_job_service_sub_warranty_status: req.category_job_service_sub_warranty_status,
        category_job_service_sub_warranty_icon: req.category_job_service_sub_warranty_icon.clone(),
        create_by: actor,
    };

    let result = match state.category_job_service_sub_warranty.create(input).await {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(error = %e, "category_job_service_sub_warranty.create failed");
            return Err(repo_error_to_response(&state, e).await);
        }
    };

    if result.success {
        return Ok((StatusCode::CREATED, created(result)).into_response());
    }

    let (status, _code_str) = match result.code.as_str() {
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
pub struct UpdateCategoryJobServiceSubWarrantyRequest {
    #[serde(default)]
    pub category_job_service_sub_warranty_description_la: Option<String>,

    #[serde(default)]
    pub category_job_service_sub_warranty_description_en: Option<String>,

    #[serde(default)]
    pub category_job_service_sub_warranty_description_th: Option<String>,

    #[serde(default)]
    pub category_job_service_sub_warranty_description_zh: Option<String>,

    #[serde(default)]
    pub category_job_service_sub_warranty_warranty_amount_day: Option<i32>,

    #[serde(default)]
    pub category_job_service_sub_warranty_status: Option<i32>,

    #[serde(default)]
    pub category_job_service_sub_warranty_icon: Option<String>,
}

impl UpdateCategoryJobServiceSubWarrantyRequest {
    pub fn validate(&self) -> Result<(), String> {
        if let Some(s) = self.category_job_service_sub_warranty_status {
            if !matches!(s, 0 | 1) {
                return Err("category_job_service_sub_warranty_status must be 0 or 1".to_string());
            }
        }
        if let Some(d) = self.category_job_service_sub_warranty_warranty_amount_day {
            if d < 0 {
                return Err(
                    "category_job_service_sub_warranty_warranty_amount_day must be >= 0"
                        .to_string(),
                );
            }
        }
        if let Some(icon) = self.category_job_service_sub_warranty_icon.as_deref() {
            if icon.chars().count() > 255 {
                return Err(
                    "category_job_service_sub_warranty_icon must be <= 255 characters".to_string(),
                );
            }
        }
        Ok(())
    }
}

#[utoipa::path(
    put,
    path = "/api/v1/category-job-service-sub-warranties/{guid}",
    tag = "category-job-service-sub-warranty",
    params(
        ("guid" = String, Path, description = "Category Job Service Sub Warranty GUID (UUID)"),
    ),
    request_body = UpdateCategoryJobServiceSubWarrantyRequest,
    responses(
        (status = 200, description = "Sub-service warranty updated", body = kokkak_domain::CategoryJobServiceSubWarrantyUpdateResult),
        (status = 400, description = "Validation error", body = crate::openapi::ApiError),
        (status = 401, description = "Not authenticated", body = crate::openapi::ApiError),
        (status = 403, description = "Missing SERVICE_UPDATE permission", body = crate::openapi::ApiError),
        (status = 404, description = "Warranty not found", body = crate::openapi::ApiError),
        (status = 500, description = "Internal error", body = crate::openapi::ApiError),
    ),
    security(("bearer_auth" = []))
)]
pub async fn update_category_job_service_sub_warranty_admin(
    State(state): State<AppState>,
    user: AuthnUser,
    Path(guid): Path<String>,
    Json(req): Json<UpdateCategoryJobServiceSubWarrantyRequest>,
) -> Result<Response, Response> {
    let locale = current_locale();
    assert_scope(&user, "admin_page", tr("err_auth.forbidden", &locale, &[]))?;

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

    let actor = user.id().to_string();
    let input = kokkak_domain::CategoryJobServiceSubWarrantyUpdateInput {
        category_job_service_sub_warranty_guid: guid.trim().to_string(),
        category_job_service_sub_warranty_description_la: req
            .category_job_service_sub_warranty_description_la
            .as_ref()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        category_job_service_sub_warranty_description_en: req
            .category_job_service_sub_warranty_description_en
            .as_ref()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        category_job_service_sub_warranty_description_th: req
            .category_job_service_sub_warranty_description_th
            .as_ref()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        category_job_service_sub_warranty_description_zh: req
            .category_job_service_sub_warranty_description_zh
            .as_ref()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        category_job_service_sub_warranty_warranty_amount_day: req
            .category_job_service_sub_warranty_warranty_amount_day,
        category_job_service_sub_warranty_status: req.category_job_service_sub_warranty_status,
        category_job_service_sub_warranty_icon: req
            .category_job_service_sub_warranty_icon
            .as_ref()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        update_by: actor,
    };

    let result = match state.category_job_service_sub_warranty.update(input).await {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(error = %e, "category_job_service_sub_warranty.update failed");
            return Err(repo_error_to_response(&state, e).await);
        }
    };

    if result.success {
        return Ok((StatusCode::OK, ok(result)).into_response());
    }

    let status = match result.code.as_str() {
        "NOT_FOUND" | "GUID_REQUIRED" => StatusCode::NOT_FOUND,
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

#[utoipa::path(
    delete,
    path = "/api/v1/category-job-service-sub-warranties/{guid}",
    tag = "category-job-service-sub-warranty",
    params(
        ("guid" = String, Path, description = "Category Job Service Sub Warranty GUID (UUID)"),
    ),
    responses(
        (status = 200, description = "Sub-service warranty deleted", body = kokkak_domain::CategoryJobServiceSubWarrantyDeleteResult),
        (status = 401, description = "Not authenticated", body = crate::openapi::ApiError),
        (status = 403, description = "Missing SERVICE_DELETE permission", body = crate::openapi::ApiError),
        (status = 404, description = "Warranty not found", body = crate::openapi::ApiError),
        (status = 500, description = "Internal error", body = crate::openapi::ApiError),
    ),
    security(("bearer_auth" = []))
)]
pub async fn delete_category_job_service_sub_warranty_admin(
    State(state): State<AppState>,
    user: AuthnUser,
    Path(guid): Path<String>,
) -> Result<Response, Response> {
    let locale = current_locale();
    assert_scope(&user, "admin_page", tr("err_auth.forbidden", &locale, &[]))?;

    if !user
        .has_permission(
            kokkak_domain::Permission::ServiceDelete,
            &state.permission_checker,
        )
        .await
    {
        return Err(permission_denied(
            &state,
            kokkak_domain::Permission::ServiceDelete.code(),
        ));
    }

    let guid = guid.trim().to_string();
    if guid.is_empty() {
        return Err(validation_envelope(
            &state,
            "category_job_service_sub_warranty_guid is required",
        ));
    }

    let actor = user.id().to_string();
    let input = kokkak_domain::CategoryJobServiceSubWarrantyDeleteInput {
        category_job_service_sub_warranty_guid: guid,
        update_by: actor,
    };

    let result = match state.category_job_service_sub_warranty.delete(input).await {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(error = %e, "category_job_service_sub_warranty.delete failed");
            return Err(repo_error_to_response(&state, e).await);
        }
    };

    if result.success {
        return Ok((StatusCode::OK, ok(result)).into_response());
    }

    let status = match result.code.as_str() {
        "NOT_FOUND" | "GUID_REQUIRED" => StatusCode::NOT_FOUND,
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
