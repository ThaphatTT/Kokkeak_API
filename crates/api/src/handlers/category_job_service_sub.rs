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
use kokkak_domain::traits::category_job_service_sub::SubImageForCreate;
use kokkak_domain::{LocalizedError, RepoError, StorageKey};
use kokkak_infra::image_processor::UserImageKind;
use serde::{Deserialize, Serialize};

use crate::middleware::auth::{assert_scope_admin_page, AuthnUser};
use crate::state::AppState;

#[derive(Debug, Deserialize, utoipa::IntoParams, utoipa::ToSchema)]
pub struct ListCategoryJobServiceSubQuery {
    #[serde(default)]
    pub category_job_service_guid: Option<String>,

    pub keyword: Option<String>,

    pub status: Option<i32>,

    pub include_deleted: Option<bool>,
}

fn normalize_locale_for_sp(raw: &str) -> String {
    let primary = raw.split('-').next().unwrap_or("").trim().to_lowercase();
    if matches!(primary.as_str(), "la" | "en" | "th" | "zh") {
        return primary;
    }
    if matches!(primary.as_str(), "lo") {
        return "la".to_string();
    }
    "la".to_string()
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ListCategoryJobServiceSubResponse {
    pub items: Vec<kokkak_domain::CategoryJobServiceSubRow>,

    pub total: usize,
}

#[utoipa::path(
    get,
    path = "/api/v1/category-job-service-subs",
    tag = "category-job-service-sub",
    params(ListCategoryJobServiceSubQuery),
    responses(
        (status = 200, description = "Active sub-service items under each main service", body = ListCategoryJobServiceSubResponse),
        (status = 401, description = "Not authenticated", body = crate::openapi::ApiError),
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_category_job_service_subs(
    State(state): State<AppState>,
    Query(q): Query<ListCategoryJobServiceSubQuery>,
) -> Result<Response, Response> {
    let main_guid = q.category_job_service_guid.as_deref().unwrap_or("");
    let status = q.status;
    let include_deleted = q.include_deleted.unwrap_or(false);
    let locale = normalize_locale_for_sp(&current_locale());

    let mut rows = match state
        .category_job_service_sub
        .list(
            main_guid,
            q.keyword.as_deref(),
            status,
            &locale,
            include_deleted,
        )
        .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(error = %e, "category_job_service_sub.list failed");
            return Err(sp_error_to_response(&state, e).await);
        }
    };

    for row in rows.iter_mut() {
        if row.main_img_path.is_empty() {
            continue;
        }
        row.main_img_url = crate::signed_url::signed_image_url(
            state.public_base_url.as_ref(),
            &row.main_img_path,
            state.signed_url_secret.as_ref(),
            state.signed_url_ttl_secs,
        );
    }

    let resp = ListCategoryJobServiceSubResponse {
        total: rows.len(),
        items: rows,
    };
    Ok((StatusCode::OK, ok(resp)).into_response())
}

#[utoipa::path(
    get,
    path = "/api/v1/category-job-service-subs/{sub_guid}",
    tag = "category-job-service-sub",
    params(
        ("sub_guid" = String, Path, description = "Category Job Service Sub GUID (UUID)"),
    ),
    responses(
        (status = 200, description = "Sub-service detail with images, fees, warranties", body = kokkak_domain::CategoryJobServiceSubDetailBundle),
        (status = 401, description = "Not authenticated"),
        (status = 404, description = "SUB_NOT_FOUND", body = crate::openapi::ApiError),
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_category_job_service_sub(
    State(state): State<AppState>,
    Path(sub_guid): Path<String>,
) -> Result<Response, Response> {
    let locale = normalize_locale_for_api(&current_locale());
    let bundle = match state
        .category_job_service_sub
        .detail(&sub_guid, &locale)
        .await
    {
        Ok(b) => b,
        Err(e) => {
            tracing::warn!(error = %e, sub_guid = %sub_guid, "category_job_service_sub.detail failed");
            return Err(sp_error_to_response(&state, e).await);
        }
    };
    let mut bundle = bundle;
    populate_signed_detail_image_urls(&state, &mut bundle.images);
    Ok((StatusCode::OK, ok(bundle)).into_response())
}

#[utoipa::path(
    get,
    path = "/api/v1/category-job-service-subs/{sub_guid}/images",
    tag = "category-job-service-sub",
    params(
        ("sub_guid" = String, Path, description = "Category Job Service Sub GUID (UUID)"),
    ),
    responses(
        (status = 200, description = "List of images for a sub-service"),
        (status = 401, description = "Not authenticated"),
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_category_job_service_sub_images(
    State(state): State<AppState>,
    Path(sub_guid): Path<String>,
) -> Result<Response, Response> {
    let mut images = match state.category_job_service_sub.list_images(&sub_guid).await {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(error = %e, "category_job_service_sub.list_images failed");
            return Err(sp_error_to_response(&state, e).await);
        }
    };
    populate_signed_image_urls(&state, &mut images);
    Ok((StatusCode::OK, ok(images)).into_response())
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct CreateCategoryJobServiceSubRequest {
    pub category_job_service_guid: String,

    pub category_job_service_sub_name: String,

    pub category_job_service_sub_start_price: String,

    #[serde(default)]
    pub category_job_service_sub_description: String,

    #[serde(default)]
    pub images: Vec<kokkak_domain::CategoryJobServiceSubImageInput>,
}

impl CreateCategoryJobServiceSubRequest {
    pub fn validate(&self) -> Result<(), String> {
        if self.category_job_service_guid.trim().is_empty() {
            return Err("category_job_service_guid is required".to_string());
        }
        if self.category_job_service_sub_name.trim().is_empty() {
            return Err("category_job_service_sub_name is required".to_string());
        }
        if self.category_job_service_sub_start_price.trim().is_empty() {
            return Err("category_job_service_sub_start_price is required".to_string());
        }
        for (i, img) in self.images.iter().enumerate() {
            if img.img_b64.trim().is_empty() {
                return Err(format!("images[{i}].img_b64 is required"));
            }
        }
        Ok(())
    }
}

#[utoipa::path(
    post,
    path = "/api/v1/category-job-service-subs",
    tag = "admin",
    request_body = CreateCategoryJobServiceSubRequest,
    responses(
        (status = 201, description = "Sub-service created with images", body = kokkak_domain::CategoryJobServiceSubCreateResult),
        (status = 400, description = "Validation error", body = crate::openapi::ApiError),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "PERMISSION_DENIED", body = crate::openapi::ApiError),
        (status = 404, description = "SERVICE_NOT_FOUND", body = crate::openapi::ApiError),
        (status = 422, description = "Invalid image (decode/encode/store)", body = crate::openapi::ApiError),
        (status = 500, description = "Internal error", body = crate::openapi::ApiError),
    ),
    security(("bearer_auth" = []))
)]
pub async fn create_category_job_service_sub_admin(
    State(state): State<AppState>,
    user: AuthnUser,
    Json(req): Json<CreateCategoryJobServiceSubRequest>,
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

    let start_price: rust_decimal::Decimal =
        match req.category_job_service_sub_start_price.trim().parse() {
            Ok(d) => d,
            Err(_) => {
                return Err(validation_envelope(
                    &state,
                    "category_job_service_sub_start_price must be a decimal number",
                ));
            }
        };

    let actor = user.id().to_string();
    let main_guid_for_input = req.category_job_service_guid.clone();
    let name = req.category_job_service_sub_name.clone();
    let description = req.category_job_service_sub_description.clone();
    let images = req.images.clone();

    let mut saved_files: Vec<StorageKey> = Vec::new();
    let mut image_records: Vec<SubImageForCreate> = Vec::with_capacity(images.len());

    for (idx, img) in images.iter().enumerate() {
        let bytes = match decode_base64_payload(&img.img_b64) {
            Ok(b) => b,
            Err(e) => {
                cleanup_files(&state, &saved_files).await;
                return Err(image_error_envelope(
                    &state,
                    &format!("images[{idx}].img_b64"),
                    kokkak_infra::image_processor::ImageError::Decode(e),
                ));
            }
        };
        let img_type = img.img_type.unwrap_or(1);
        let img_priority = img.img_priority.unwrap_or(idx as i32);

        let placeholder_guid = format!("pending-{idx}");
        let processed = match state
            .image
            .clone()
            .process_and_store(
                &bytes,
                &placeholder_guid,
                UserImageKind::CategoryJobServiceSubImage {
                    service_sub_guid: placeholder_guid.clone(),
                },
            )
            .await
        {
            Ok(p) => p,
            Err(e) => {
                cleanup_files(&state, &saved_files).await;
                return Err(image_error_envelope(&state, &format!("images[{idx}]"), e));
            }
        };
        saved_files.push(processed.key.clone());
        image_records.push(SubImageForCreate {
            img_type,
            img_priority,
            img_path: processed.key.as_str().to_string(),
        });
    }

    let create_input = kokkak_domain::CategoryJobServiceSubCreateInput {
        category_job_service_guid: main_guid_for_input,
        category_job_service_sub_name: name,
        category_job_service_sub_start_price: start_price,
        category_job_service_sub_description: description,
        create_by: actor.clone(),
        images: vec![],
    };

    let result = match state
        .category_job_service_sub
        .create_with_images(create_input, image_records)
        .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(error = %e, "category_job_service_sub.create_with_images failed");
            cleanup_files(&state, &saved_files).await;
            return Err(sp_error_to_response(&state, e).await);
        }
    };

    let locale = current_locale();
    let i18n_key = sp_service_sub_status_key(&result.code);
    let localized = tr(i18n_key, &locale, &[]);
    let resp = ApiResponse {
        success: result.success,
        data: Some(serde_json::json!({
            "category_job_service_sub_guid": result.category_job_service_sub_guid,
            "code": result.code,
            "message": localized,
        })),
        error: None,
        meta: None,
    };
    Ok((StatusCode::CREATED, Json(resp)).into_response())
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct UpdateCategoryJobServiceSubRequest {
    #[serde(default)]
    pub category_job_service_main_guid: Option<String>,

    #[serde(default)]
    pub category_job_service_sub_name_la: Option<String>,

    #[serde(default)]
    pub category_job_service_sub_name_en: Option<String>,

    #[serde(default)]
    pub category_job_service_sub_name_th: Option<String>,

    #[serde(default)]
    pub category_job_service_sub_name_zh: Option<String>,

    #[serde(default)]
    pub category_job_service_sub_start_price: Option<String>,

    #[serde(default)]
    pub category_job_service_sub_description_la: Option<String>,

    #[serde(default)]
    pub category_job_service_sub_description_en: Option<String>,

    #[serde(default)]
    pub category_job_service_sub_description_th: Option<String>,

    #[serde(default)]
    pub category_job_service_sub_description_zh: Option<String>,

    #[serde(default)]
    pub category_job_service_sub_status: Option<i32>,

    #[serde(default)]
    pub warranties: Vec<kokkak_domain::CategoryJobServiceSubCreateSpWarrantyInput>,

    #[serde(default)]
    pub fees: Vec<kokkak_domain::CategoryJobServiceSubCreateSpFeeInput>,

    #[serde(default)]
    pub images: Vec<UpdateCategoryJobServiceSubImageRequest>,

    #[serde(default)]
    pub deleted_image_guids: Vec<String>,

    #[serde(default = "default_replace_images")]
    pub replace_images: bool,
}

fn default_replace_images() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize, utoipa::ToSchema)]
pub struct UpdateCategoryJobServiceSubImageRequest {
    #[serde(default, alias = "img_path")]
    pub img_b64: Option<String>,

    #[serde(default)]
    pub img_type: Option<i32>,

    #[serde(default)]
    pub img_type_language: Option<i32>,

    #[serde(default)]
    pub img_priority: Option<i32>,

    #[serde(default)]
    pub img_status: Option<i32>,
}

#[utoipa::path(
    put,
    path = "/api/v1/category-job-service-subs/{sub_guid}",
    tag = "admin",
    params(
        ("sub_guid" = String, Path, description = "Category Job Service Sub GUID (UUID)"),
    ),
    request_body = UpdateCategoryJobServiceSubRequest,
    responses(
        (status = 200, description = "Sub-service updated, images replaced", body = kokkak_domain::CategoryJobServiceSubUpdateResult),
        (status = 400, description = "Validation error", body = crate::openapi::ApiError),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "PERMISSION_DENIED", body = crate::openapi::ApiError),
        (status = 404, description = "SUB_NOT_FOUND", body = crate::openapi::ApiError),
        (status = 422, description = "Invalid image (decode/encode/store)", body = crate::openapi::ApiError),
        (status = 500, description = "Internal error", body = crate::openapi::ApiError),
    ),
    security(("bearer_auth" = []))
)]
pub async fn update_category_job_service_sub_admin(
    State(state): State<AppState>,
    user: AuthnUser,
    Path(sub_guid): Path<String>,
    Json(req): Json<UpdateCategoryJobServiceSubRequest>,
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

    let start_price: Option<rust_decimal::Decimal> = match req.category_job_service_sub_start_price
    {
        Some(ref sp) => match sp.trim().parse() {
            Ok(d) => Some(d),
            Err(_) => {
                return Err(validation_envelope(
                    &state,
                    "category_job_service_sub_start_price must be a decimal number",
                ));
            }
        },
        None => None,
    };

    let actor = user.id().to_string();

    let mut saved_files: Vec<StorageKey> = Vec::new();
    let mut sp_images: Vec<kokkak_domain::CategoryJobServiceSubCreateSpImageInput> =
        Vec::with_capacity(req.images.len());

    for (idx, img) in req.images.iter().enumerate() {
        let img_b64_str = img.img_b64.as_deref().unwrap_or("");

        if img_b64_str.starts_with("http://") || img_b64_str.starts_with("https://") {
            continue;
        }

        if img_b64_str.starts_with("/files/") {
            sp_images.push(kokkak_domain::CategoryJobServiceSubCreateSpImageInput {
                img_path: img_b64_str.trim_start_matches("/files/").to_string(),
                img_type: Some(img.img_type.unwrap_or(1)),
                img_type_language: img.img_type_language,
                priority: Some(img.img_priority.unwrap_or(idx as i32)),
                img_status: Some(img.img_status.unwrap_or(1)),
            });
            continue;
        }

        let bytes = match decode_base64_payload(img_b64_str) {
            Ok(b) => b,
            Err(e) => {
                cleanup_files(&state, &saved_files).await;
                return Err(image_error_envelope(
                    &state,
                    &format!("images[{idx}]"),
                    kokkak_infra::image_processor::ImageError::Decode(e),
                ));
            }
        };

        let placeholder_guid = format!("pending-{idx}");
        let processed = match state
            .image
            .clone()
            .process_and_store(
                &bytes,
                &placeholder_guid,
                UserImageKind::CategoryJobServiceSubImage {
                    service_sub_guid: placeholder_guid.clone(),
                },
            )
            .await
        {
            Ok(p) => p,
            Err(e) => {
                cleanup_files(&state, &saved_files).await;
                return Err(image_error_envelope(&state, &format!("images[{idx}]"), e));
            }
        };
        saved_files.push(processed.key.clone());

        sp_images.push(kokkak_domain::CategoryJobServiceSubCreateSpImageInput {
            img_path: processed.key.as_str().to_string(),
            img_type: Some(img.img_type.unwrap_or(1)),
            img_type_language: img.img_type_language,
            priority: Some(img.img_priority.unwrap_or(idx as i32)),
            img_status: Some(img.img_status.unwrap_or(1)),
        });
    }

    for guid in &req.deleted_image_guids {
        let clean_guid = guid.strip_prefix("/files/").unwrap_or(guid);
        let del_input = kokkak_domain::CategoryJobServiceSubImageDeleteInput {
            category_job_service_sub_img_guid: clean_guid.to_string(),
            update_by: actor.clone(),
        };
        if let Err(e) = state.category_job_service_sub.delete_image(del_input).await {
            tracing::warn!(error = %e, guid = %guid, "deleted_image_guids soft-delete failed (non-fatal)");
        }
    }

    let sp_input = kokkak_domain::CategoryJobServiceSubUpdateSpInput {
        category_job_service_sub_guid: sub_guid.clone(),
        category_job_service_main_guid: req.category_job_service_main_guid,
        category_job_service_sub_name_la: req.category_job_service_sub_name_la,
        category_job_service_sub_name_en: req.category_job_service_sub_name_en,
        category_job_service_sub_name_th: req.category_job_service_sub_name_th,
        category_job_service_sub_name_zh: req.category_job_service_sub_name_zh,
        category_job_service_sub_start_price: start_price,
        category_job_service_sub_description_la: req.category_job_service_sub_description_la,
        category_job_service_sub_description_en: req.category_job_service_sub_description_en,
        category_job_service_sub_description_th: req.category_job_service_sub_description_th,
        category_job_service_sub_description_zh: req.category_job_service_sub_description_zh,
        category_job_service_sub_status: req.category_job_service_sub_status,
        warranties: req.warranties,
        fees: req.fees,
        images: sp_images,
        replace_images: req.replace_images,
        update_by: actor.clone(),
    };

    let result = match state
        .category_job_service_sub
        .update_via_sp(&sp_input)
        .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(error = %e, "category_job_service_sub.update_via_sp failed");
            cleanup_files(&state, &saved_files).await;
            return Err(sp_error_to_response(&state, e).await);
        }
    };

    let locale = current_locale();
    let i18n_key = sp_service_sub_status_key(&result.code);
    let localized = tr(i18n_key, &locale, &[]);
    let resp = ApiResponse {
        success: result.success,
        data: Some(serde_json::json!({
            "category_job_service_sub_guid": result.category_job_service_sub_guid,
            "code": result.code,
            "message": localized,
            "warranty_count": result.warranty_count,
            "fee_count": result.fee_count,
            "image_count": result.image_count,
        })),
        error: None,
        meta: None,
    };
    Ok((StatusCode::OK, Json(resp)).into_response())
}

#[utoipa::path(
    delete,
    path = "/api/v1/category-job-service-subs/{sub_guid}",
    tag = "admin",
    params(
        ("sub_guid" = String, Path, description = "Category Job Service Sub GUID (UUID)"),
    ),
    responses(
        (status = 200, description = "Sub-service soft-deleted (cascades to images/fees/warranties)", body = kokkak_domain::CategoryJobServiceSubDeleteResult),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "PERMISSION_DENIED", body = crate::openapi::ApiError),
        (status = 404, description = "SUB_NOT_FOUND", body = crate::openapi::ApiError),
    ),
    security(("bearer_auth" = []))
)]
pub async fn delete_category_job_service_sub_admin(
    State(state): State<AppState>,
    user: AuthnUser,
    Path(sub_guid): Path<String>,
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
        .category_job_service_sub
        .delete(&sub_guid, &user.id().to_string())
        .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(error = %e, "category_job_service_sub.delete failed");
            return Err(sp_error_to_response(&state, e).await);
        }
    };

    let locale = current_locale();
    let i18n_key = sp_service_sub_status_key(&result.code);
    let localized = tr(i18n_key, &locale, &[]);
    let resp = ApiResponse {
        success: result.success,
        data: Some(serde_json::json!({
            "category_job_service_sub_guid": result.category_job_service_sub_guid,
            "code": result.code,
            "message": localized,
        })),
        error: None,
        meta: None,
    };
    Ok((StatusCode::OK, Json(resp)).into_response())
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct CreateCategoryJobServiceSubSpRequest {
    #[serde(default)]
    pub category_job_service_sub_guid: Option<String>,

    pub category_job_service_main_guid: String,

    #[serde(default)]
    pub category_job_service_sub_name_la: Option<String>,

    #[serde(default)]
    pub category_job_service_sub_name_en: Option<String>,

    #[serde(default)]
    pub category_job_service_sub_name_th: Option<String>,

    #[serde(default)]
    pub category_job_service_sub_name_zh: Option<String>,

    pub category_job_service_sub_start_price: String,

    #[serde(default)]
    pub category_job_service_sub_description_la: Option<String>,

    #[serde(default)]
    pub category_job_service_sub_description_en: Option<String>,

    #[serde(default)]
    pub category_job_service_sub_description_th: Option<String>,

    #[serde(default)]
    pub category_job_service_sub_description_zh: Option<String>,

    #[serde(default = "default_sp_status")]
    pub category_job_service_sub_status: i32,

    #[serde(default)]
    pub warranties: Vec<kokkak_domain::CategoryJobServiceSubCreateSpWarrantyInput>,

    #[serde(default)]
    pub fees: Vec<kokkak_domain::CategoryJobServiceSubCreateSpFeeInput>,

    #[serde(default)]
    pub images: Vec<kokkak_domain::CategoryJobServiceSubCreateSpImageInput>,
}

fn default_sp_status() -> i32 {
    1
}

impl CreateCategoryJobServiceSubSpRequest {
    pub fn validate(&self) -> Result<(), String> {
        if self.category_job_service_main_guid.trim().is_empty() {
            return Err("category_job_service_main_guid is required".to_string());
        }
        let has_any_name = self
            .category_job_service_sub_name_la
            .as_deref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false)
            || self
                .category_job_service_sub_name_en
                .as_deref()
                .map(|s| !s.trim().is_empty())
                .unwrap_or(false)
            || self
                .category_job_service_sub_name_th
                .as_deref()
                .map(|s| !s.trim().is_empty())
                .unwrap_or(false)
            || self
                .category_job_service_sub_name_zh
                .as_deref()
                .map(|s| !s.trim().is_empty())
                .unwrap_or(false);
        if !has_any_name {
            return Err(
                "at least one name (_name_la/_name_en/_name_th/_name_zh) is required".to_string(),
            );
        }
        if self.category_job_service_sub_start_price.trim().is_empty() {
            return Err("category_job_service_sub_start_price is required".to_string());
        }
        if let Ok(price) = self
            .category_job_service_sub_start_price
            .trim()
            .parse::<rust_decimal::Decimal>()
        {
            if price < rust_decimal::Decimal::ZERO {
                return Err("category_job_service_sub_start_price cannot be negative".to_string());
            }
        }
        for (i, w) in self.warranties.iter().enumerate() {
            if w.guid.trim().is_empty() {
                return Err(format!("warranties[{i}].guid is required"));
            }
            if w.sort_order < 1 {
                return Err(format!("warranties[{i}].sort_order must be >= 1"));
            }
        }
        for (i, f) in self.fees.iter().enumerate() {
            if f.guid.trim().is_empty() {
                return Err(format!("fees[{i}].guid is required"));
            }
            if f.sort_order < 1 {
                return Err(format!("fees[{i}].sort_order must be >= 1"));
            }
        }
        for (i, img) in self.images.iter().enumerate() {
            if img.img_path.trim().is_empty() {
                return Err(format!("images[{i}].img_path is required"));
            }
        }
        Ok(())
    }
}

#[utoipa::path(
    post,
    path = "/api/v1/category-job-service-subs/sp-insert",
    tag = "admin",
    request_body = CreateCategoryJobServiceSubSpRequest,
    responses(
        (status = 201, description = "Sub-service created via SP", body = kokkak_domain::CategoryJobServiceSubCreateSpResult),
        (status = 400, description = "Validation error", body = crate::openapi::ApiError),
        (status = 401, description = "Not authenticated"),
        (status = 403, description = "PERMISSION_DENIED", body = crate::openapi::ApiError),
        (status = 500, description = "Internal error", body = crate::openapi::ApiError),
    ),
    security(("bearer_auth" = []))
)]
pub async fn create_category_job_service_sub_sp_admin(
    State(state): State<AppState>,
    user: AuthnUser,
    Json(req): Json<CreateCategoryJobServiceSubSpRequest>,
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

    let start_price: rust_decimal::Decimal =
        match req.category_job_service_sub_start_price.trim().parse() {
            Ok(d) => d,
            Err(_) => {
                return Err(validation_envelope(
                    &state,
                    "category_job_service_sub_start_price must be a decimal number",
                ));
            }
        };

    let actor = user.id().to_string();
    let input = kokkak_domain::CategoryJobServiceSubCreateSpInput {
        category_job_service_sub_guid: req.category_job_service_sub_guid,
        category_job_service_main_guid: req.category_job_service_main_guid,
        category_job_service_sub_name_la: req.category_job_service_sub_name_la,
        category_job_service_sub_name_en: req.category_job_service_sub_name_en,
        category_job_service_sub_name_th: req.category_job_service_sub_name_th,
        category_job_service_sub_name_zh: req.category_job_service_sub_name_zh,
        category_job_service_sub_start_price: start_price,
        category_job_service_sub_description_la: req.category_job_service_sub_description_la,
        category_job_service_sub_description_en: req.category_job_service_sub_description_en,
        category_job_service_sub_description_th: req.category_job_service_sub_description_th,
        category_job_service_sub_description_zh: req.category_job_service_sub_description_zh,
        category_job_service_sub_status: req.category_job_service_sub_status,
        warranties: req.warranties,
        fees: req.fees,
        images: req.images,
        create_by: actor,
    };

    let result = match state.category_job_service_sub.create_via_sp(input).await {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(error = %e, "category_job_service_sub.create_via_sp failed");
            return Err(sp_error_to_response(&state, e).await);
        }
    };

    let locale = current_locale();
    let i18n_key = sp_service_sub_status_key(&result.code);
    let localized = tr(i18n_key, &locale, &[]);
    let resp = ApiResponse {
        success: result.success,
        data: Some(serde_json::json!({
            "category_job_service_sub_guid": result.category_job_service_sub_guid,
            "code": result.code,
            "message": localized,
            "warranty_count": result.warranty_count,
            "fee_count": result.fee_count,
            "image_count": result.image_count,
        })),
        error: None,
        meta: None,
    };
    Ok((StatusCode::CREATED, Json(resp)).into_response())
}

fn sp_service_sub_status_key(sp_code: &str) -> &'static str {
    match sp_code {
        "INSERT_SUCCESS" | "CREATE_SUCCESS" | "CREATED" => {
            "err_category_job_service_sub.create_success"
        }
        "UPDATE_SUCCESS" | "UPDATED" => "err_category_job_service_sub.update_success",
        "DELETE_SUCCESS" | "DELETED" => "err_category_job_service_sub.delete_success",
        "NOT_FOUND" => "err_category_job_service_sub.not_found",
        "INVALID_STATUS" => "err_category_job_service_sub.invalid_status",
        "NAME_REQUIRED" => "err_category_job_service_sub.name_required",
        "MAIN_NOT_FOUND" => "err_category_job_service_sub.main_not_found",
        code if code.starts_with("INSERT_ERROR_") => "err_category_job_service_sub.backend",
        _ => "err_category_job_service_sub.backend",
    }
}

async fn cleanup_files(state: &AppState, files: &[StorageKey]) {
    for f in files {
        if let Err(e) = state.storage.delete(f).await {
            tracing::warn!(
                error = %e,
                key = %f,
                "rollback: storage delete failed (non-fatal — may leak file)"
            );
        }
    }
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
    items: &mut [kokkak_domain::CategoryJobServiceSubImageRow],
) {
    for row in items.iter_mut() {
        if row.category_job_service_sub_img_path.is_empty() {
            continue;
        }
        row.category_job_service_sub_img_url = crate::signed_url::signed_image_url(
            state.public_base_url.as_ref(),
            &row.category_job_service_sub_img_path,
            state.signed_url_secret.as_ref(),
            state.signed_url_ttl_secs,
        );
    }
}

fn populate_signed_detail_image_urls(
    state: &AppState,
    items: &mut [kokkak_domain::CategoryJobServiceSubDetailImageRow],
) {
    for row in items.iter_mut() {
        if row.category_job_service_sub_img_img_path.is_empty() {
            continue;
        }
        row.category_job_service_sub_img_url = crate::signed_url::signed_image_url(
            state.public_base_url.as_ref(),
            &row.category_job_service_sub_img_img_path,
            state.signed_url_secret.as_ref(),
            state.signed_url_ttl_secs,
        );
    }
}

fn normalize_locale_for_api(raw: &str) -> String {
    let primary = raw.split('-').next().unwrap_or("").trim().to_lowercase();
    if matches!(primary.as_str(), "la" | "en" | "th" | "zh") {
        return primary;
    }
    if primary == "lo" {
        return "la".to_string();
    }
    "la".to_string()
}

fn validation_envelope(state: &AppState, msg: &str) -> Response {
    let locale = current_locale();
    let _ = state;
    let envelope: ApiResponse<()> = ApiResponse {
        success: false,
        data: None,
        error: Some(kokkak_common::error::ApiErrorBody {
            code: "validation".into(),
            message: tr("err_category_job_service_sub.validation", &locale, &[msg]),
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
        "category_job_service_sub image upload failed"
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
        "category_job_service_sub repo error"
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
