use std::sync::Arc;

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use kokkak_common::i18n::{current_locale, tr, tr_with_repo};
use kokkak_common::response::{created, ok, ApiResponse};
use kokkak_domain::{LocalizedError, RepoError};
use kokkak_infra::image_processor::UserImageKind;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::middleware::auth::AuthnUser;
use crate::state::AppState;

#[derive(Debug, Deserialize, utoipa::IntoParams, utoipa::ToSchema)]
pub struct ListCategoryJobServiceMainQuery {
    #[serde(default)]
    pub category_job_main_guid: Option<String>,

    pub keyword: Option<String>,

    pub include_inactive: Option<bool>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ListCategoryJobServiceMainResponse {
    pub items: Vec<kokkak_domain::CategoryJobServiceMainRow>,

    pub total: usize,
}

#[utoipa::path(
    get,
    path = "/api/v1/category-job-services",
    tag = "category-job-service-main",
    params(ListCategoryJobServiceMainQuery),
    responses(
        (status = 200, description = "Active service items under each main category (mobile/web landing page)", body = ListCategoryJobServiceMainResponse),
        (status = 401, description = "Not authenticated", body = crate::openapi::ApiError),
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_category_job_service_mains(
    State(state): State<AppState>,
    Query(q): Query<ListCategoryJobServiceMainQuery>,
) -> Result<Response, Response> {
    let include_inactive = q.include_inactive.unwrap_or(false);
    let main_guid = q.category_job_main_guid.as_deref().unwrap_or("");
    let rows = match state
        .category_job_service_main
        .list(main_guid, q.keyword.as_deref(), include_inactive)
        .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(error = %e, "category_job_service_main.list failed");
            return Err(sp_error_to_response(&state, e).await);
        }
    };
    let mut items = rows;
    populate_signed_image_urls(&state, &mut items);
    let resp = ListCategoryJobServiceMainResponse {
        total: items.len(),
        items,
    };
    Ok((StatusCode::OK, ok(resp)).into_response())
}

#[derive(Debug, Deserialize, utoipa::IntoParams, utoipa::ToSchema)]
pub struct GetCategoryJobServiceMainPath {
    pub service_guid: String,
}

#[utoipa::path(
    get,
    path = "/api/v1/category-job-services/{service_guid}",
    tag = "category-job-service-main",
    params(
        ("service_guid" = String, Path, description = "Category Job Service GUID (UUID)"),
    ),
    responses(
        (status = 200, description = "Single service item by GUID"),
        (status = 401, description = "Not authenticated"),
        (status = 404, description = "Service not found"),
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_category_job_service_main(
    State(state): State<AppState>,
    Path(service_guid): Path<String>,
) -> Result<Response, Response> {
    let rows = match state.category_job_service_main.list("", None, true).await {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(error = %e, "category_job_service_main.list failed");
            return Err(sp_error_to_response(&state, e).await);
        }
    };
    let mut found: Vec<kokkak_domain::CategoryJobServiceMainRow> = rows
        .into_iter()
        .filter(|r| r.category_job_service_guid == service_guid)
        .collect();
    match found.len() {
        0 => {
            let locale = current_locale();
            let msg = tr(
                "err_category_job_service_main.not_found",
                &locale,
                &[service_guid.as_str()],
            );
            let envelope: ApiResponse<()> = ApiResponse {
                success: false,
                data: None,
                error: Some(kokkak_common::error::ApiErrorBody {
                    code: "not_found".into(),
                    message: msg,
                }),
                meta: None,
            };
            Err((StatusCode::NOT_FOUND, Json(envelope)).into_response())
        }
        _ => {
            populate_signed_image_urls(&state, &mut found);
            Ok((StatusCode::OK, ok(found.remove(0))).into_response())
        }
    }
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct CreateCategoryJobServiceMainRequest {
    pub category_job_main_guid: String,

    pub category_job_service_name: String,

    #[serde(default)]
    pub category_job_service_icon_style: Option<String>,

    #[serde(default)]
    pub category_job_service_icon_line: Option<String>,

    #[serde(default)]
    pub category_job_service_img_path: Option<String>,

    #[serde(default)]
    pub category_job_service_img_b64: Option<String>,
}

impl CreateCategoryJobServiceMainRequest {
    pub fn validate(&self) -> Result<(), String> {
        if self.category_job_main_guid.trim().is_empty() {
            return Err("category_job_main_guid is required".to_string());
        }
        if self.category_job_service_name.trim().is_empty() {
            return Err("category_job_service_name is required".to_string());
        }
        Ok(())
    }
}

#[utoipa::path(
    post,
    path = "/api/v1/admin/category-job-services",
    tag = "admin",
    request_body = CreateCategoryJobServiceMainRequest,
    responses(
        (status = 201, description = "Service created", body = kokkak_domain::CategoryJobServiceMainCreateResult),
        (status = 400, description = "Validation error", body = crate::openapi::ApiError),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "PERMISSION_DENIED", body = crate::openapi::ApiError),
        (status = 409, description = "CATEGORY_NOT_FOUND", body = crate::openapi::ApiError),
        (status = 422, description = "Invalid image (decode/encode/store)", body = crate::openapi::ApiError),
        (status = 500, description = "Internal error", body = crate::openapi::ApiError),
    ),
    security(("bearer_auth" = []))
)]
pub async fn create_category_job_service_main_admin(
    State(state): State<AppState>,
    user: AuthnUser,
    Json(req): Json<CreateCategoryJobServiceMainRequest>,
) -> Result<Response, Response> {
    if !user
        .has_permission(
            kokkak_domain::Permission::CategoryJobServiceMainCreate,
            &state.permission_checker,
        )
        .await
    {
        return Err(permission_denied(
            &state,
            kokkak_domain::Permission::CategoryJobServiceMainCreate.code(),
        ));
    }

    if let Err(msg) = req.validate() {
        return Err(validation_envelope(&state, &msg));
    }

    let service_guid = Uuid::now_v7().to_string();

    let img_path = match resolve_b64_service_icon(
        state.image.clone(),
        &service_guid,
        req.category_job_service_img_b64.as_deref(),
    )
    .await
    {
        Ok(v) => v.or(req.category_job_service_img_path),
        Err(e) => {
            return Err(image_error_envelope(
                &state,
                "category_job_service_img_b64",
                e,
            ))
        }
    };

    let input = kokkak_domain::CategoryJobServiceMainCreateInput {
        category_job_main_guid: req.category_job_main_guid,
        category_job_service_name: req.category_job_service_name,
        category_job_service_icon_style: req.category_job_service_icon_style,
        category_job_service_icon_line: req.category_job_service_icon_line,
        category_job_service_img_path: img_path,
        create_by: user.id().to_string(),
    };

    let result = match state.category_job_service_main.create(input).await {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(error = %e, "category_job_service_main.create failed");
            return Err(sp_error_to_response(&state, e).await);
        }
    };

    Ok((StatusCode::CREATED, created(result)).into_response())
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct UpdateCategoryJobServiceMainRequest {
    pub category_job_main_guid: String,

    pub category_job_service_name: String,

    #[serde(default)]
    pub category_job_service_icon_style: Option<String>,

    #[serde(default)]
    pub category_job_service_icon_line: Option<String>,

    #[serde(default)]
    pub category_job_service_img_path: Option<String>,

    #[serde(default)]
    pub category_job_service_img_b64: Option<String>,

    pub category_job_service_status: i32,
}

impl UpdateCategoryJobServiceMainRequest {
    pub fn validate(&self) -> Result<(), String> {
        if self.category_job_main_guid.trim().is_empty() {
            return Err("category_job_main_guid is required".to_string());
        }
        if self.category_job_service_name.trim().is_empty() {
            return Err("category_job_service_name is required".to_string());
        }
        if !matches!(self.category_job_service_status, 0 | 1) {
            return Err("category_job_service_status must be 0 or 1".to_string());
        }
        Ok(())
    }
}

#[utoipa::path(
    put,
    path = "/api/v1/admin/category-job-services/{service_guid}",
    tag = "admin",
    params(
        ("service_guid" = String, Path, description = "Category Job Service GUID (UUID)"),
    ),
    request_body = UpdateCategoryJobServiceMainRequest,
    responses(
        (status = 200, description = "Service updated", body = kokkak_domain::CategoryJobServiceMainUpdateResult),
        (status = 400, description = "Validation error", body = crate::openapi::ApiError),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "PERMISSION_DENIED", body = crate::openapi::ApiError),
        (status = 404, description = "SERVICE_NOT_FOUND", body = crate::openapi::ApiError),
        (status = 422, description = "Invalid image (decode/encode/store)", body = crate::openapi::ApiError),
        (status = 500, description = "Internal error", body = crate::openapi::ApiError),
    ),
    security(("bearer_auth" = []))
)]
pub async fn update_category_job_service_main_admin(
    State(state): State<AppState>,
    user: AuthnUser,
    Path(service_guid): Path<String>,
    Json(req): Json<UpdateCategoryJobServiceMainRequest>,
) -> Result<Response, Response> {
    if !user
        .has_permission(
            kokkak_domain::Permission::CategoryJobServiceMainUpdate,
            &state.permission_checker,
        )
        .await
    {
        return Err(permission_denied(
            &state,
            kokkak_domain::Permission::CategoryJobServiceMainUpdate.code(),
        ));
    }

    if let Err(msg) = req.validate() {
        return Err(validation_envelope(&state, &msg));
    }

    let img_path = match resolve_b64_service_icon(
        state.image.clone(),
        &service_guid,
        req.category_job_service_img_b64.as_deref(),
    )
    .await
    {
        Ok(v) => v.or(req.category_job_service_img_path),
        Err(e) => {
            return Err(image_error_envelope(
                &state,
                "category_job_service_img_b64",
                e,
            ))
        }
    };

    let input = kokkak_domain::CategoryJobServiceMainUpdateInput {
        category_job_service_guid: service_guid,
        category_job_main_guid: req.category_job_main_guid,
        category_job_service_name: req.category_job_service_name,
        category_job_service_icon_style: req.category_job_service_icon_style,
        category_job_service_icon_line: req.category_job_service_icon_line,
        category_job_service_img_path: img_path,
        category_job_service_status: req.category_job_service_status,
        update_by: user.id().to_string(),
    };

    let result = match state.category_job_service_main.update(input).await {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(error = %e, "category_job_service_main.update failed");
            return Err(sp_error_to_response(&state, e).await);
        }
    };

    Ok((StatusCode::OK, ok(result)).into_response())
}

#[utoipa::path(
    delete,
    path = "/api/v1/admin/category-job-services/{service_guid}",
    tag = "admin",
    params(
        ("service_guid" = String, Path, description = "Category Job Service GUID (UUID)"),
    ),
    responses(
        (status = 200, description = "Service soft-deleted (cascade)", body = kokkak_domain::CategoryJobServiceMainDeleteResult),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "PERMISSION_DENIED", body = crate::openapi::ApiError),
        (status = 404, description = "SERVICE_NOT_FOUND", body = crate::openapi::ApiError),
    ),
    security(("bearer_auth" = []))
)]
pub async fn delete_category_job_service_main_admin(
    State(state): State<AppState>,
    user: AuthnUser,
    Path(service_guid): Path<String>,
) -> Result<Response, Response> {
    if !user
        .has_permission(
            kokkak_domain::Permission::CategoryJobServiceMainDelete,
            &state.permission_checker,
        )
        .await
    {
        return Err(permission_denied(
            &state,
            kokkak_domain::Permission::CategoryJobServiceMainDelete.code(),
        ));
    }

    let result = match state
        .category_job_service_main
        .delete(&service_guid, &user.id().to_string())
        .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(error = %e, "category_job_service_main.delete failed");
            return Err(sp_error_to_response(&state, e).await);
        }
    };

    Ok((StatusCode::OK, ok(result)).into_response())
}

async fn resolve_b64_service_icon(
    processor: Arc<kokkak_infra::image_processor::ImageProcessor>,
    service_guid: &str,
    b64: Option<&str>,
) -> Result<Option<String>, kokkak_infra::image_processor::ImageError> {
    let Some(s) = b64 else { return Ok(None) };
    let bytes =
        decode_base64_payload(s).map_err(kokkak_infra::image_processor::ImageError::Decode)?;
    let result = processor
        .process_and_store(
            &bytes,
            service_guid,
            UserImageKind::CategoryJobServiceMainIcon {
                service_guid: service_guid.to_string(),
            },
        )
        .await?;
    Ok(Some(result.key.as_str().to_string()))
}

fn decode_base64_payload(s: &str) -> Result<Vec<u8>, String> {
    let payload = if let Some(idx) = s.find("base64,") {
        &s[idx + "base64,".len()..]
    } else {
        s
    };
    let cleaned: String = payload.chars().filter(|c| !c.is_whitespace()).collect();
    STANDARD
        .decode(cleaned.as_bytes())
        .map_err(|e| e.to_string())
}

fn populate_signed_image_urls(
    state: &AppState,
    items: &mut [kokkak_domain::CategoryJobServiceMainRow],
) {
    for row in items.iter_mut() {
        if row.category_job_service_img_path.is_empty() {
            continue;
        }
        row.category_job_service_img_url = crate::signed_url::signed_image_url(
            state.public_base_url.as_ref(),
            &row.category_job_service_img_path,
            state.signed_url_secret.as_ref(),
            state.signed_url_ttl_secs,
        );
    }
}

fn validation_envelope(state: &AppState, msg: &str) -> Response {
    let locale = current_locale();
    let _ = state;
    let envelope: ApiResponse<()> = ApiResponse {
        success: false,
        data: None,
        error: Some(kokkak_common::error::ApiErrorBody {
            code: "validation".into(),
            message: tr("err_category_job_service_main.validation", &locale, &[msg]),
        }),
        meta: None,
    };
    (StatusCode::BAD_REQUEST, Json(envelope)).into_response()
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

fn image_error_envelope(
    state: &AppState,
    field: &str,
    err: kokkak_infra::image_processor::ImageError,
) -> Response {
    let locale = current_locale();
    tracing::warn!(
        field = field,
        error = %err,
        "category_job_service_main image upload failed"
    );
    let i18n_key = "err_admin_user.image_invalid";
    let localized = tr(i18n_key, &locale, &[field]);
    let _ = state;
    let envelope: ApiResponse<()> = ApiResponse {
        success: false,
        data: None,
        error: Some(kokkak_common::error::ApiErrorBody {
            code: "validation".into(),
            message: localized,
        }),
        meta: None,
    };
    (StatusCode::UNPROCESSABLE_ENTITY, Json(envelope)).into_response()
}

async fn sp_error_to_response(state: &AppState, err: RepoError) -> Response {
    use RepoError::*;
    let (status, code) = match &err {
        NotFound(_) => (StatusCode::NOT_FOUND, "not_found"),
        Conflict(_) => (StatusCode::CONFLICT, "conflict"),
        Backend(_) => (StatusCode::INTERNAL_SERVER_ERROR, "internal"),
    };
    let locale = current_locale();
    let message = tr_with_repo(&*state.translation, &locale, err.l10n_key(), &[]).await;
    tracing::warn!(
        repo_error = %err,
        localized_code = code,
        "category_job_service_main repo error"
    );
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
