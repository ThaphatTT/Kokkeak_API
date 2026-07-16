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
use kokkak_common::response::{ok, ApiResponse};
use kokkak_domain::{LocalizedError, RepoError};
use kokkak_infra::image_processor::UserImageKind;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::middleware::auth::{assert_scope_admin_page, AuthnUser};
use crate::state::AppState;

#[derive(Debug, Deserialize, utoipa::IntoParams, utoipa::ToSchema)]
pub struct ListCategoryJobMainQuery {
    pub keyword: Option<String>,

    pub status: Option<i32>,

    pub locale: Option<String>,

    pub page: Option<u32>,

    pub page_size: Option<u32>,
}

#[derive(Debug, Deserialize, utoipa::IntoParams, utoipa::ToSchema)]
pub struct AutocompleteCategoryJobMainQuery {
    pub keyword: Option<String>,

    pub status: Option<i32>,

    pub locale: Option<String>,

    pub take: Option<i32>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ListCategoryJobMainMeta {
    pub total_count: i64,

    pub page: u32,

    pub page_size: u32,

    pub total_page: u32,

    pub has_next: bool,

    #[serde(default)]
    pub active: i64,

    #[serde(default)]
    pub close: i64,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ListCategoryJobMainResponse {
    pub items: Vec<kokkak_domain::CategoryJobMainRow>,

    pub meta: ListCategoryJobMainMeta,
}

#[utoipa::path(
    get,
    path = "/api/v1/category-job-mains",
    tag = "category-job-main",
    params(ListCategoryJobMainQuery),
    responses(
        (status = 200, description = "Active category job mains (mobile/web landing page) — paginated, localized", body = ListCategoryJobMainResponse),
        (status = 401, description = "Not authenticated", body = crate::openapi::ApiError),
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_category_job_mains(
    State(state): State<AppState>,
    Query(q): Query<ListCategoryJobMainQuery>,
) -> Result<Response, Response> {
    let page = q.page.unwrap_or(1).max(1);
    let page_size = q.page_size.unwrap_or(20).clamp(1, 100);
    let status = q.status.filter(|v| matches!(v, 0 | 1));

    let locale = q.locale.clone().or_else(|| Some(current_locale()));

    let input = kokkak_domain::CategoryJobMainListInput {
        keyword: q.keyword.clone(),
        status,
        locale,
        page,
        page_size,
    };

    let page_data = match state.category_job_main.list(input).await {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!(error = %e, "category_job_main.list failed");
            return Err(repo_error_to_response(e, &state).await);
        }
    };

    let mut items = page_data.items;
    populate_signed_image_urls(&state, &mut items);

    let total_page = page_data.total_page;
    let has_next = page_data.page < total_page && total_page > 0;

    let resp = ListCategoryJobMainResponse {
        items,
        meta: ListCategoryJobMainMeta {
            total_count: page_data.total_count,
            page: page_data.page,
            page_size: page_data.page_size,
            total_page,
            has_next,
            active: page_data.active,
            close: page_data.close,
        },
    };
    Ok((StatusCode::OK, ok(resp)).into_response())
}

#[utoipa::path(
        get,
        path = "/api/v1/category-job-mains/autocomplete",
        tag = "category-job-main",
        params(AutocompleteCategoryJobMainQuery),
        responses(
            (status = 200, description = "Category-job-main autocomplete rows (top-N by priority then recency)", body = Vec<kokkak_domain::CategoryJobMainAutocompleteRow>),
            (status = 401, description = "Not authenticated", body = crate::openapi::ApiError),
            (status = 422, description = "Validation error (invalid status / locale / take)", body = crate::openapi::ApiError),
        ),
        security(("bearer_auth" = []))
    )]
pub async fn autocomplete_category_job_mains(
    State(state): State<AppState>,
    Query(q): Query<AutocompleteCategoryJobMainQuery>,
) -> Result<Response, Response> {
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
    if let Some(t) = q.take {
        if !(1..=100).contains(&t) {
            return Err(validation_envelope(
                &state,
                "take must be between 1 and 100",
            ));
        }
    }

    let locale = q.locale.clone().or_else(|| Some(current_locale()));

    let input = kokkak_domain::CategoryJobMainAutocompleteInput {
        keyword: q.keyword.clone(),
        status: q.status,
        locale,
        take: q.take,
    };

    let rows = match state.category_job_main.autocomplete(input).await {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(error = %e, "category_job_main.autocomplete failed");
            return Err(repo_error_to_response(e, &state).await);
        }
    };

    Ok((
        StatusCode::OK,
        Json(ApiResponse {
            success: true,
            data: Some(rows),
            error: None,
            meta: None,
        }),
    )
        .into_response())
}

#[utoipa::path(
    get,
    path = "/api/v1/category-job-mains/{guid}",
    tag = "category-job-main",
    params(
        ("guid" = String, Path, description = "Category Job Main GUID (UUID)"),
    ),
    responses(
        (status = 200, description = "Single category job main by GUID"),
        (status = 401, description = "Not authenticated"),
        (status = 404, description = "Category not found"),
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_category_job_main(
    State(state): State<AppState>,
    Path(guid): Path<String>,
) -> Result<Response, Response> {
    if guid.trim().is_empty() {
        return Err(validation_envelope(&state, "guid is required"));
    }

    let detail = match state.category_job_main.detail(&guid).await {
        Ok(Some(d)) => d,
        Ok(None) => {
            let locale = current_locale();
            let msg = tr("err_category_job_main.not_found", &locale, &[guid.as_str()]);
            let envelope: ApiResponse<()> = ApiResponse {
                success: false,
                data: None,
                error: Some(kokkak_common::error::ApiErrorBody {
                    code: "not_found".into(),
                    message: msg,
                }),
                meta: None,
            };
            return Err((StatusCode::NOT_FOUND, Json(envelope)).into_response());
        }
        Err(e) => {
            tracing::warn!(error = %e, "category_job_main.detail failed");
            return Err(repo_error_to_response(e, &state).await);
        }
    };

    let mut detail = detail;
    populate_signed_detail_image_url(&state, &mut detail);
    Ok((StatusCode::OK, ok(detail)).into_response())
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct CreateCategoryJobMainRequest {
    #[serde(default)]
    pub category_job_main_name_la: Option<String>,

    #[serde(default)]
    pub category_job_main_name_en: Option<String>,

    #[serde(default)]
    pub category_job_main_name_th: Option<String>,

    #[serde(default)]
    pub category_job_main_name_zh: Option<String>,

    #[serde(default)]
    pub category_job_main_icon_style: Option<String>,

    #[serde(default)]
    pub category_job_main_icon_line: Option<String>,

    #[serde(default)]
    pub category_job_main_img_path: Option<String>,

    #[serde(default)]
    pub category_job_main_img_b64: Option<String>,

    #[serde(default)]
    pub category_job_main_priority: Option<i32>,
}

impl CreateCategoryJobMainRequest {
    pub fn validate(&self) -> Result<(), String> {
        let any_name = [
            self.category_job_main_name_la.as_deref(),
            self.category_job_main_name_en.as_deref(),
            self.category_job_main_name_th.as_deref(),
            self.category_job_main_name_zh.as_deref(),
        ]
        .iter()
        .flatten()
        .any(|s| !s.trim().is_empty());
        if !any_name {
            return Err(
                "at least one of name_la, name_en, name_th, name_zh is required".to_string(),
            );
        }
        Ok(())
    }
}

#[utoipa::path(
    post,
    path = "/api/v1/category-job-mains",
    tag = "admin",
    request_body = CreateCategoryJobMainRequest,
    responses(
        (status = 201, description = "Category created", body = kokkak_domain::CategoryJobMainCreateResult),
        (status = 400, description = "Validation error", body = crate::openapi::ApiError),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "PERMISSION_DENIED", body = crate::openapi::ApiError),
        (status = 409, description = "CATEGORY_NAME_DUPLICATE", body = crate::openapi::ApiError),
        (status = 422, description = "Invalid image (decode/encode/store)", body = crate::openapi::ApiError),
        (status = 500, description = "Internal error", body = crate::openapi::ApiError),
    ),
    security(("bearer_auth" = []))
)]
pub async fn create_category_job_main_admin(
    State(state): State<AppState>,
    user: AuthnUser,
    Json(req): Json<CreateCategoryJobMainRequest>,
) -> Result<Response, Response> {
    let locale = current_locale();
    assert_scope_admin_page(&user, tr("err_auth.forbidden", &locale, &[]))?;

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

    let category_guid = Uuid::now_v7().to_string();

    let img_path = match resolve_b64_category_icon(
        state.image.clone(),
        &category_guid,
        req.category_job_main_img_b64.as_deref(),
    )
    .await
    {
        Ok(v) => v.or(req
            .category_job_main_img_path
            .map(|p| p.strip_prefix("/files/").unwrap_or(&p).to_string())),
        Err(e) => return Err(image_error_envelope(&state, "category_job_main_img_b64", e)),
    };

    let input = kokkak_domain::CategoryJobMainCreateInput {
        category_job_main_name_la: req.category_job_main_name_la,
        category_job_main_name_en: req.category_job_main_name_en,
        category_job_main_name_th: req.category_job_main_name_th,
        category_job_main_name_zh: req.category_job_main_name_zh,
        category_job_main_icon_style: req.category_job_main_icon_style,
        category_job_main_icon_line: req.category_job_main_icon_line,
        category_job_main_img_path: img_path,
        category_job_main_priority: req.category_job_main_priority,
        create_by: user.id().to_string(),
    };

    let result = match state.category_job_main.create(input).await {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(error = %e, "category_job_main.create failed");
            return Err(sp_error_to_response(&state, e).await);
        }
    };

    let locale = current_locale();
    let i18n_key = sp_main_status_key(&result.code);
    let localized = tr(i18n_key, &locale, &[]);
    let resp = ApiResponse {
        success: result.success,
        data: Some(serde_json::json!({
            "category_job_main_guid": result.category_job_main_guid,
            "code": result.code,
            "message": localized,
        })),
        error: None,
        meta: None,
    };
    Ok((StatusCode::CREATED, Json(resp)).into_response())
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct UpdateCategoryJobMainRequest {
    #[serde(default)]
    pub category_job_main_name_la: Option<String>,

    #[serde(default)]
    pub category_job_main_name_en: Option<String>,

    #[serde(default)]
    pub category_job_main_name_th: Option<String>,

    #[serde(default)]
    pub category_job_main_name_zh: Option<String>,

    #[serde(default)]
    pub category_job_main_icon_style: Option<String>,

    #[serde(default)]
    pub category_job_main_icon_line: Option<String>,

    #[serde(default)]
    pub category_job_main_img_path: Option<String>,

    #[serde(default)]
    pub category_job_main_img_b64: Option<String>,

    pub category_job_main_status: i32,

    pub category_job_main_priority: i32,
}

impl UpdateCategoryJobMainRequest {
    pub fn validate(&self) -> Result<(), String> {
        if !matches!(self.category_job_main_status, 0 | 1) {
            return Err("category_job_main_status must be 0 or 1".to_string());
        }
        Ok(())
    }
}

#[utoipa::path(
    put,
    path = "/api/v1/category-job-mains/{guid}",
    tag = "admin",
    params(
        ("guid" = String, Path, description = "Category Job Main GUID (UUID)"),
    ),
    request_body = UpdateCategoryJobMainRequest,
    responses(
        (status = 200, description = "Category updated", body = kokkak_domain::CategoryJobMainUpdateResult),
        (status = 400, description = "Validation error", body = crate::openapi::ApiError),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "PERMISSION_DENIED", body = crate::openapi::ApiError),
        (status = 404, description = "CATEGORY_NOT_FOUND", body = crate::openapi::ApiError),
        (status = 422, description = "Invalid image (decode/encode/store)", body = crate::openapi::ApiError),
        (status = 500, description = "Internal error", body = crate::openapi::ApiError),
    ),
    security(("bearer_auth" = []))
)]
pub async fn update_category_job_main_admin(
    State(state): State<AppState>,
    user: AuthnUser,
    Path(guid): Path<String>,
    Json(req): Json<UpdateCategoryJobMainRequest>,
) -> Result<Response, Response> {
    let locale = current_locale();
    assert_scope_admin_page(&user, tr("err_auth.forbidden", &locale, &[]))?;

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

    let img_path = match resolve_b64_category_icon(
        state.image.clone(),
        &guid,
        req.category_job_main_img_b64.as_deref(),
    )
    .await
    {
        Ok(v) => v.or(req
            .category_job_main_img_path
            .map(|p| p.strip_prefix("/files/").unwrap_or(&p).to_string())),
        Err(e) => return Err(image_error_envelope(&state, "category_job_main_img_b64", e)),
    };

    let input = kokkak_domain::CategoryJobMainUpdateInput {
        category_job_main_guid: guid,
        category_job_main_name_la: req.category_job_main_name_la,
        category_job_main_name_en: req.category_job_main_name_en,
        category_job_main_name_th: req.category_job_main_name_th,
        category_job_main_name_zh: req.category_job_main_name_zh,
        category_job_main_icon_style: req.category_job_main_icon_style,
        category_job_main_icon_line: req.category_job_main_icon_line,
        category_job_main_img_path: img_path,
        category_job_main_status: req.category_job_main_status,
        category_job_main_priority: req.category_job_main_priority,
        update_by: user.id().to_string(),
    };

    let result = match state.category_job_main.update(input).await {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(error = %e, "category_job_main.update failed");
            return Err(sp_error_to_response(&state, e).await);
        }
    };

    let locale = current_locale();
    let i18n_key = sp_main_status_key(&result.code);
    let localized = tr(i18n_key, &locale, &[]);
    let resp = ApiResponse {
        success: result.success,
        data: Some(serde_json::json!({
            "category_job_main_guid": result.category_job_main_guid,
            "code": result.code,
            "message": localized,
        })),
        error: None,
        meta: None,
    };
    Ok((StatusCode::OK, Json(resp)).into_response())
}

#[utoipa::path(
    delete,
    path = "/api/v1/category-job-mains/{guid}",
    tag = "admin",
    params(
        ("guid" = String, Path, description = "Category Job Main GUID (UUID)"),
    ),
    responses(
        (status = 200, description = "Category soft-deleted (cascade)", body = kokkak_domain::CategoryJobMainDeleteResult),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "PERMISSION_DENIED", body = crate::openapi::ApiError),
        (status = 404, description = "CATEGORY_NOT_FOUND", body = crate::openapi::ApiError),
    ),
    security(("bearer_auth" = []))
)]
pub async fn delete_category_job_main_admin(
    State(state): State<AppState>,
    user: AuthnUser,
    Path(guid): Path<String>,
) -> Result<Response, Response> {
    let locale = current_locale();
    assert_scope_admin_page(&user, tr("err_auth.forbidden", &locale, &[]))?;

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

    let result = match state
        .category_job_main
        .delete(&guid, &user.id().to_string())
        .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(error = %e, "category_job_main.delete failed");
            return Err(sp_error_to_response(&state, e).await);
        }
    };

    let locale = current_locale();
    let i18n_key = sp_main_status_key(&result.code);
    let localized = tr(i18n_key, &locale, &[]);
    let resp = ApiResponse {
        success: result.success,
        data: Some(serde_json::json!({
            "category_job_main_guid": result.category_job_main_guid,
            "code": result.code,
            "message": localized,
        })),
        error: None,
        meta: None,
    };
    Ok((StatusCode::OK, Json(resp)).into_response())
}

fn sp_main_status_key(sp_code: &str) -> &'static str {
    match sp_code {
        "INSERT_SUCCESS" | "CREATE_SUCCESS" | "CREATED" => "err_category_job_main.create_success",
        "UPDATE_SUCCESS" | "UPDATED" => "err_category_job_main.update_success",
        "DELETE_SUCCESS" | "DELETED" => "err_category_job_main.delete_success",
        "NOT_FOUND" => "err_category_job_main.not_found",
        "INVALID_STATUS" => "err_category_job_main.validation",
        _ => "err_category_job_main.backend",
    }
}

async fn resolve_b64_category_icon(
    processor: Arc<kokkak_infra::image_processor::ImageProcessor>,
    category_guid: &str,
    b64: Option<&str>,
) -> Result<Option<String>, kokkak_infra::image_processor::ImageError> {
    let Some(s) = b64 else { return Ok(None) };
    let bytes =
        decode_base64_payload(s).map_err(kokkak_infra::image_processor::ImageError::Decode)?;
    let result = processor
        .process_and_store(
            &bytes,
            category_guid,
            UserImageKind::CategoryJobMainIcon {
                category_guid: category_guid.to_string(),
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

fn populate_signed_image_urls(state: &AppState, items: &mut [kokkak_domain::CategoryJobMainRow]) {
    for row in items.iter_mut() {
        if row.category_job_main_img_path.is_empty() {
            continue;
        }
        row.category_job_main_img_url = crate::signed_url::signed_image_url(
            state.public_base_url.as_ref(),
            &row.category_job_main_img_path,
            state.signed_url_secret.as_ref(),
            state.signed_url_ttl_secs,
        );
    }
}

fn populate_signed_detail_image_url(
    state: &AppState,
    row: &mut kokkak_domain::CategoryJobMainDetailRow,
) {
    if row.category_job_main_img_path.is_empty() {
        return;
    }
    row.category_job_main_img_url = crate::signed_url::signed_image_url(
        state.public_base_url.as_ref(),
        &row.category_job_main_img_path,
        state.signed_url_secret.as_ref(),
        state.signed_url_ttl_secs,
    );
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

fn image_error_envelope(
    state: &AppState,
    field: &str,
    err: kokkak_infra::image_processor::ImageError,
) -> Response {
    let locale = current_locale();
    tracing::warn!(
        field = field,
        error = %err,
        "category_job_main image upload failed"
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

fn validation_envelope(state: &AppState, msg: &str) -> Response {
    let locale = current_locale();
    let _ = state;
    let envelope: ApiResponse<()> = ApiResponse {
        success: false,
        data: None,
        error: Some(kokkak_common::error::ApiErrorBody {
            code: "validation".into(),
            message: tr("err_category_job_main.validation", &locale, &[msg]),
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
        "category_job_main repo error"
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
