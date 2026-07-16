use std::sync::Arc;

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Utc};
use kokkak_application::admin_user::{
    AdminInsertUserFullInput, AdminUpdateUserFullInput, AdminUserListPagingInput,
};
use kokkak_application::auth::{PasswordHasherPort, RegisterInput};
use kokkak_application::user_role::{PermissionUpdateInput, UpdatePermissionsInput};
use kokkak_common::error::AppError;
use kokkak_common::error_codes::ErrorCode;
use kokkak_common::i18n::{current_locale, tr};
use kokkak_common::response::{created, ok, paginated, ApiResponse, PageMeta};
#[allow(unused_imports)]
use kokkak_domain::{
    AdminDeleteUserError, AdminInsertUserError, AdminInsertUserResult, AdminUpdateUserError,
    AdminUpdateUserResult, AdminUserDetail, Permission, PermissionUpdateRow, PermissionUserGroup,
    RepoError, Role, UserRoleWithPermissions,
};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::{Validate, ValidationError};

use crate::error::{ApiError, IntoLocalizedResponse};
use crate::extractors::ValidatedJson;
use crate::handlers::auth::AuthResponse;
use crate::middleware::auth::{assert_scope_admin_page, AuthnUser};
use crate::state::AppState;
use kokkak_infra::image_processor::UserImageKind;

#[derive(Debug, Deserialize, Validate, utoipa::ToSchema)]
pub struct CreateUserRequest {
    #[validate(length(min = 3, max = 64, message = "username must be 3-64 characters"))]
    pub username: String,
    #[validate(length(min = 8, max = 128, message = "password must be 8-128 characters"))]
    pub password: String,
    #[validate(length(min = 1, max = 100, message = "first_name must be 1-100 characters"))]
    pub first_name: String,
    #[validate(length(min = 1, max = 100, message = "last_name must be 1-100 characters"))]
    pub last_name: String,

    #[validate(length(min = 1, max = 20, message = "role must be 1-20 characters"))]
    pub role: String,
}

#[utoipa::path(
    post,
    path = "/api/v1/users",
    tag = "admin",
    request_body = CreateUserRequest,
    responses(
        (status = 201, description = "User created (admin-created)", body = kokkak_domain::PublicUser),
        (status = 401, description = "Not authenticated", body = crate::openapi::ApiError),
        (status = 403, description = "Not an admin", body = crate::openapi::ApiError),
        (status = 409, description = "Username already taken", body = crate::openapi::ApiError),
        (status = 422, description = "Validation error", body = crate::openapi::ApiError),
    ),
    security(("bearer_auth" = []))
)]
pub async fn create_user_admin(
    State(state): State<AppState>,
    user: AuthnUser,
    ValidatedJson(req): ValidatedJson<CreateUserRequest>,
) -> Result<Response, Response> {
    let locale = current_locale();
    assert_scope_admin_page(&user, tr("err_auth.forbidden", &locale, &[]))?;

    if !user
        .has_permission(Permission::UsersCreate, &state.permission_checker)
        .await
    {
        let code_str = Permission::UsersCreate.code();
        let localized = tr("err_auth.permission_denied", &locale, &[code_str]);
        return Err(ApiError::from(AppError::Localized {
            status: StatusCode::FORBIDDEN,
            code: ErrorCode::PERMISSION_DENIED,
            message: localized,
        })
        .into_response());
    }

    let role = match Role::from_code(&req.role) {
        Some(r) => r,
        None => {
            return Err(ApiError::from(AppError::RoleNotAllowed(req.role))
                .into_localized_response(&state)
                .await);
        }
    };

    let input = RegisterInput {
        username: req.username,
        password: req.password,
        first_name: req.first_name,
        last_name: req.last_name,
        role,
    };
    let outcome = match state.auth.register(input).await {
        Ok(o) => o,
        Err(e) => return Err(ApiError::from(e).into_localized_response(&state).await),
    };
    Ok((StatusCode::CREATED, created(AuthResponse::from(outcome))).into_response())
}

#[derive(Debug, Default, Deserialize, utoipa::ToSchema, utoipa::IntoParams)]
pub struct ListUsersQuery {
    pub page: Option<u32>,

    pub page_size: Option<u32>,

    #[serde(default)]
    pub keyword: Option<String>,

    pub user_status: Option<i32>,

    pub user_is_customer: Option<bool>,

    pub user_is_employee: Option<bool>,

    pub user_is_freelance: Option<bool>,

    pub department_guid: Option<String>,

    pub department_team_guid: Option<String>,

    pub position_guid: Option<String>,
}

#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct ListUsersResponse {
    #[serde(flatten)]
    pub page: kokkak_domain::admin_user::AdminUserListPagingPage,
}

#[utoipa::path(
        get,
        path = "/api/v1/users",
        tag = "admin",
        params(ListUsersQuery),
        responses(
            (status = 200, description = "Page of users", body = ListUsersResponse),
            (status = 401, description = "Not authenticated", body = crate::openapi::ApiError),
            (status = 403, description = "Not an admin", body = crate::openapi::ApiError),
        ),
        security(("bearer_auth" = []))
    )]
pub async fn list_users_admin(
    State(state): State<AppState>,
    user: AuthnUser,
    Query(q): Query<ListUsersQuery>,
) -> Result<Response, Response> {
    let locale = current_locale();
    assert_scope_admin_page(&user, tr("err_auth.forbidden", &locale, &[]))?;

    if !user
        .has_permission(Permission::PageUsersView, &state.permission_checker)
        .await
    {
        let code_str = Permission::PageUsersView.code();
        let localized = tr("err_auth.permission_denied", &locale, &[code_str]);
        return Err(ApiError::from(AppError::Localized {
            status: StatusCode::FORBIDDEN,
            code: ErrorCode::PERMISSION_DENIED,
            message: localized,
        })
        .into_response());
    }

    let input = AdminUserListPagingInput {
        keyword: q.keyword.unwrap_or_default(),
        user_status: q.user_status,
        user_is_customer: q.user_is_customer,
        user_is_employee: q.user_is_employee,
        user_is_freelance: q.user_is_freelance,
        department_guid: q.department_guid,
        department_team_guid: q.department_team_guid,
        position_guid: q.position_guid,
        page: q.page.unwrap_or(1),
        page_size: q.page_size.unwrap_or(20),
    };

    let page = match state.admin_users.list_users_paging(user.id(), input).await {
        Ok(p) => p,
        Err(e) => return Err(ApiError::from(e).into_localized_response(&state).await),
    };

    let total_pages = if page.page_size > 0 {
        (page.total_count + page.page_size as i64 - 1) / page.page_size as i64
    } else {
        0
    };
    let has_next = (page.page as i64) < total_pages;
    let next_cursor = if has_next {
        Some((page.page + 1).to_string())
    } else {
        None
    };

    let meta = PageMeta {
        limit: page.page_size as usize,
        has_next,
        next_cursor,
    };
    Ok((StatusCode::OK, paginated(ListUsersResponse { page }, meta)).into_response())
}

#[utoipa::path(
    get,
    path = "/api/v1/users/{guid}/permissions",
    tag = "admin",
    params(
        ("guid" = Uuid, Path, description = "User GUID (36-char UUID)"),
    ),
    responses(
        (status = 200, description = "Grouped permission detail for the user", body = PermissionUserGroup),
        (status = 401, description = "Not authenticated", body = crate::openapi::ApiError),
        (status = 403, description = "Not an admin", body = crate::openapi::ApiError),
        (status = 404, description = "User not found", body = crate::openapi::ApiError),
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_user_permissions_admin(
    State(state): State<AppState>,
    user: AuthnUser,
    Path(guid): Path<Uuid>,
) -> Result<Response, Response> {
    let locale = current_locale();
    assert_scope_admin_page(&user, tr("err_auth.forbidden", &locale, &[]))?;

    if !user
        .has_permission(Permission::PagePermissionsView, &state.permission_checker)
        .await
    {
        let code_str = Permission::PagePermissionsView.code();
        let localized = tr("err_auth.permission_denied", &locale, &[code_str]);
        return Err(ApiError::from(AppError::Localized {
            status: StatusCode::FORBIDDEN,
            code: ErrorCode::PERMISSION_DENIED,
            message: localized,
        })
        .into_response());
    }

    let group = match state
        .permission
        .get_permission_user_group(guid, user.id())
        .await
    {
        Ok(g) => g,
        Err(RepoError::NotFound(_)) => {
            let locale = current_locale();
            let localized = tr("err_auth.user_not_found", &locale, &[&guid.to_string()]);
            return Err(ApiError::from(
                AppError::NotFound(guid.to_string()).with_message(localized),
            )
            .into_response());
        }
        Err(e) => return Err(ApiError::from(e).into_localized_response(&state).await),
    };

    Ok((
        StatusCode::OK,
        Json(ApiResponse::<PermissionUserGroup> {
            success: true,
            data: Some(group),
            error: None,
            meta: None,
        }),
    )
        .into_response())
}

#[utoipa::path(
    get,
    path = "/api/v1/users/{guid}/detail",
    tag = "admin",
    params(
        ("guid" = Uuid, Path, description = "User GUID (36-char UUID)"),
    ),
    responses(
        (status = 200, description = "Full user detail (profile + login + company + roles + position + salary + schedule + bank + attachments)", body = AdminUserDetail),
        (status = 401, description = "Not authenticated", body = crate::openapi::ApiError),
        (status = 403, description = "Not an admin", body = crate::openapi::ApiError),
        (status = 404, description = "User not found", body = crate::openapi::ApiError),
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_user_detail_full_admin(
    State(state): State<AppState>,
    user: AuthnUser,
    Path(guid): Path<Uuid>,
) -> Result<Response, Response> {
    let locale = current_locale();
    assert_scope_admin_page(&user, tr("err_auth.forbidden", &locale, &[]))?;

    if !user
        .has_permission(Permission::PageUsersView, &state.permission_checker)
        .await
    {
        let code_str = Permission::PageUsersView.code();
        let localized = tr("err_auth.permission_denied", &locale, &[code_str]);
        return Err(ApiError::from(AppError::Localized {
            status: StatusCode::FORBIDDEN,
            code: ErrorCode::PERMISSION_DENIED,
            message: localized,
        })
        .into_response());
    }

    let mut detail = match state
        .admin_users
        .get_user_detail_full(user.id(), guid)
        .await
    {
        Ok(Some(d)) => d,
        Ok(None) => {
            let locale = current_locale();
            let localized = tr("err_auth.user_not_found", &locale, &[&guid.to_string()]);
            return Err(ApiError::from(
                AppError::NotFound(guid.to_string()).with_message(localized),
            )
            .into_response());
        }
        Err(e) => return Err(ApiError::from(e).into_localized_response(&state).await),
    };

    populate_image_urls(
        &mut detail,
        &state.public_base_url,
        &state.signed_url_secret,
        state.signed_url_ttl_secs,
    );

    Ok((StatusCode::OK, ok(detail)).into_response())
}

fn populate_image_urls(
    detail: &mut kokkak_domain::admin_user::AdminUserDetail,
    public_base_url: &str,
    signed_url_secret: &str,
    signed_url_ttl_secs: u32,
) {
    let compose = |path: &str| {
        crate::signed_url::signed_image_url(
            public_base_url,
            path,
            signed_url_secret,
            signed_url_ttl_secs,
        )
    };

    if let Some(img) = detail.profile_image.as_mut() {
        img.profile_img_url = compose(&img.profile_img_path);
    }
    if let Some(bank) = detail.bank_account.as_mut() {
        bank.bank_book_img_url = compose(&bank.bank_book_img_path);
    }

    let slots: [&mut Option<kokkak_domain::admin_user::AdminUserDetailAttachment>; 4] = [
        &mut detail.id_card_front,
        &mut detail.id_card_back,
        &mut detail.proof_of_address,
        &mut detail.source_of_funds_statement,
    ];
    for slot in slots {
        if let Some(att) = slot.as_mut() {
            att.attachment_url = compose(&att.attachment_path);
        }
    }
}

#[derive(Debug, Deserialize, utoipa::ToSchema, utoipa::IntoParams)]
pub struct PermissionsQuery {
    pub mode: String,
}

#[utoipa::path(
    get,
    path = "/api/v1/permissions",
    tag = "admin",
    params(PermissionsQuery),
    responses(
        (status = 200, description = "Role × permission matrix (grouped by role)", body = Vec<UserRoleWithPermissions>),
        (status = 400, description = "Missing or empty `mode` query parameter", body = crate::openapi::ApiError),
        (status = 401, description = "Not authenticated", body = crate::openapi::ApiError),
        (status = 403, description = "Not an admin", body = crate::openapi::ApiError),
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_permissions(
    State(state): State<AppState>,
    user: AuthnUser,
    Query(q): Query<PermissionsQuery>,
) -> Result<Response, Response> {
    let locale = current_locale();
    assert_scope_admin_page(&user, tr("err_auth.forbidden", &locale, &[]))?;

    if !user
        .has_permission(Permission::PagePermissionsView, &state.permission_checker)
        .await
    {
        let code_str = Permission::PagePermissionsView.code();
        let localized = tr("err_auth.permission_denied", &locale, &[code_str]);
        return Err(ApiError::from(AppError::Localized {
            status: StatusCode::FORBIDDEN,
            code: ErrorCode::PERMISSION_DENIED,
            message: localized,
        })
        .into_response());
    }

    let mode = q.mode.trim();
    if mode.is_empty() {
        let locale = current_locale();
        let msg = tr("err_permission.mode_required", &locale, &[]);
        return Err(bad_request_envelope(&msg, ErrorCode::BAD_REQUEST));
    }

    let groups: Vec<UserRoleWithPermissions> =
        match state.user_roles.list_permissions(mode, user.id()).await {
            Ok(r) => r,
            Err(e) => return Err(ApiError::from(e).into_localized_response(&state).await),
        };

    Ok((
        StatusCode::OK,
        Json(ApiResponse {
            success: true,
            data: Some(groups),
            error: None,
            meta: None,
        }),
    )
        .into_response())
}

fn bad_request_envelope(message: &str, code: &'static str) -> Response {
    let envelope: ApiResponse<()> = ApiResponse {
        success: false,
        data: None,
        error: Some(kokkak_common::error::ApiErrorBody {
            code: code.into(),
            message: message.to_string(),
        }),
        meta: None,
    };
    (StatusCode::BAD_REQUEST, Json(envelope)).into_response()
}

const MAX_BULK_PERMISSION_UPDATES: u64 = 500;

#[derive(Debug, Deserialize, Serialize, Validate, utoipa::ToSchema)]
pub struct PermissionUpdateItem {
    #[validate(length(min = 36, max = 36, message = "user_role_guid must be a 36-char GUID"))]
    pub user_role_guid: String,

    #[validate(length(
        min = 36,
        max = 36,
        message = "user_permission_guid must be a 36-char GUID"
    ))]
    pub user_permission_guid: String,

    #[validate(custom(
        function = "validate_status",
        message = "user_role_permission_status must be 0 or 1"
    ))]
    pub user_role_permission_status: i32,
}

fn validate_status(status: i32) -> Result<(), ValidationError> {
    if status == 0 || status == 1 {
        Ok(())
    } else {
        Err(ValidationError::new("invalid_status"))
    }
}

fn is_valid_guid(s: &str) -> bool {
    Uuid::parse_str(s).is_ok()
}

#[derive(Debug, Deserialize, Validate, utoipa::ToSchema)]
pub struct UpdatePermissionsRequest {
    #[validate(length(
        min = 1,
        max = MAX_BULK_PERMISSION_UPDATES,
        message = "updates must have 1-500 items"
    ))]
    #[validate(nested)]
    pub updates: Vec<PermissionUpdateItem>,

    #[validate(length(max = 36, message = "update_by must be at most 36 chars"))]
    pub update_by: Option<String>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct PermissionUpdateResultItem {
    pub user_role_guid: String,

    pub user_permission_guid: String,

    pub success: bool,

    pub code: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_role_permission_guid: Option<String>,

    pub user_role_permission_status: i32,

    pub message: String,
}

impl From<PermissionUpdateRow> for PermissionUpdateResultItem {
    fn from(r: PermissionUpdateRow) -> Self {
        Self {
            user_role_guid: r.user_role_guid,
            user_permission_guid: r.user_permission_guid,
            success: r.success,
            code: r.code,
            user_role_permission_guid: r.user_role_permission_guid,
            user_role_permission_status: r.user_role_permission_status,
            message: r.message,
        }
    }
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct UpdatePermissionsResponse {
    pub total: usize,

    pub updated: usize,

    pub created: usize,

    pub no_change: usize,

    pub failed: usize,

    pub results: Vec<PermissionUpdateResultItem>,
}

#[utoipa::path(
    post,
    path = "/api/v1/permissions",
    tag = "admin",
    request_body = UpdatePermissionsRequest,
    responses(
        (status = 200, description = "Per-item results (always 200; per-item `success` field carries the outcome)", body = UpdatePermissionsResponse),
        (status = 400, description = "Malformed JSON body", body = crate::openapi::ApiError),
        (status = 401, description = "Not authenticated", body = crate::openapi::ApiError),
        (status = 403, description = "Not an admin", body = crate::openapi::ApiError),
        (status = 422, description = "Validation error (empty list, bad GUID, status not in {0,1})", body = crate::openapi::ApiError),
    ),
    security(("bearer_auth" = []))
)]
pub async fn update_permissions_admin(
    State(state): State<AppState>,
    user: AuthnUser,
    ValidatedJson(req): ValidatedJson<UpdatePermissionsRequest>,
) -> Result<Response, Response> {
    let locale = current_locale();
    assert_scope_admin_page(&user, tr("err_auth.forbidden", &locale, &[]))?;

    if !user
        .has_permission(Permission::PermissionsUpdate, &state.permission_checker)
        .await
    {
        let code_str = Permission::PermissionsUpdate.code();
        let localized = tr("err_auth.permission_denied", &locale, &[code_str]);
        return Err(ApiError::from(AppError::Localized {
            status: StatusCode::FORBIDDEN,
            code: ErrorCode::PERMISSION_DENIED,
            message: localized,
        })
        .into_response());
    }

    for (i, item) in req.updates.iter().enumerate() {
        if !is_valid_guid(&item.user_role_guid) {
            let locale = current_locale();
            let msg = tr("err_permission.invalid_role_guid", &locale, &[]);
            return Err(validation_envelope(&msg, i, "user_role_guid"));
        }
        if !is_valid_guid(&item.user_permission_guid) {
            let locale = current_locale();
            let msg = tr("err_permission.invalid_permission_guid", &locale, &[]);
            return Err(validation_envelope(&msg, i, "user_permission_guid"));
        }
    }

    let update_by: Option<String> = req.update_by.or_else(|| Some(user.id().to_string()));

    let input = UpdatePermissionsInput {
        updates: req
            .updates
            .iter()
            .map(|i| PermissionUpdateInput {
                user_role_guid: i.user_role_guid.clone(),
                user_permission_guid: i.user_permission_guid.clone(),
                user_role_permission_status: i.user_role_permission_status,
            })
            .collect(),
        update_by,
    };
    let rows = match state.user_roles.update_permissions(input).await {
        Ok(r) => r,
        Err(e) => return Err(ApiError::from(e).into_localized_response(&state).await),
    };

    let mut updated = 0usize;
    let mut created = 0usize;
    let mut no_change = 0usize;
    let mut failed = 0usize;
    let locale = current_locale();
    let results: Vec<PermissionUpdateResultItem> = rows
        .into_iter()
        .map(|r| {
            match r.code.as_str() {
                PermissionUpdateRow::CODE_UPDATED => updated += 1,
                PermissionUpdateRow::CODE_CREATED => created += 1,
                PermissionUpdateRow::CODE_NO_CHANGE => no_change += 1,
                _ => failed += 1,
            }
            let i18n_key = sp_permission_update_status_key(&r.code);
            PermissionUpdateResultItem {
                user_role_guid: r.user_role_guid,
                user_permission_guid: r.user_permission_guid,
                success: r.success,
                code: r.code,
                user_role_permission_guid: r.user_role_permission_guid,
                user_role_permission_status: r.user_role_permission_status,
                message: tr(i18n_key, &locale, &[]),
            }
        })
        .collect();
    let total = results.len();

    Ok((
        StatusCode::OK,
        Json(ApiResponse {
            success: true,
            data: Some(UpdatePermissionsResponse {
                total,
                updated,
                created,
                no_change,
                failed,
                results,
            }),
            error: None,
            meta: None,
        }),
    )
        .into_response())
}

fn validation_envelope(message: &str, index: usize, field: &str) -> Response {
    let full = format!("{message} (item #{index}, field `{field}`)");
    let envelope: ApiResponse<()> = ApiResponse {
        success: false,
        data: None,
        error: Some(kokkak_common::error::ApiErrorBody {
            code: ErrorCode::VALIDATION.into(),
            message: full,
        }),
        meta: None,
    };
    (StatusCode::UNPROCESSABLE_ENTITY, Json(envelope)).into_response()
}

#[derive(Debug, Deserialize, Serialize, Validate, utoipa::ToSchema, Default, Clone)]
pub struct DayScheduleDto {
    pub is_working: bool,

    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(max = 8, message = "start_time must be `HH:MM:SS`"))]
    pub start_time: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(max = 8, message = "end_time must be `HH:MM:SS`"))]
    pub end_time: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Validate, utoipa::ToSchema, Default, Clone)]
pub struct WeeklyScheduleDto {
    #[validate(nested)]
    pub monday: DayScheduleDto,

    #[validate(nested)]
    pub tuesday: DayScheduleDto,

    #[validate(nested)]
    pub wednesday: DayScheduleDto,

    #[validate(nested)]
    pub thursday: DayScheduleDto,

    #[validate(nested)]
    pub friday: DayScheduleDto,

    #[validate(nested)]
    pub saturday: DayScheduleDto,

    #[validate(nested)]
    pub sunday: DayScheduleDto,
}

#[derive(Debug, Deserialize, Validate, utoipa::ToSchema)]
pub struct AdminInsertUserRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(max = 36, message = "user_guid must be a 36-char UUID"))]
    pub user_guid: Option<String>,

    #[validate(length(min = 1, max = 100, message = "first_name must be 1-100 characters"))]
    pub first_name: String,

    #[validate(length(min = 1, max = 100, message = "last_name must be 1-100 characters"))]
    pub last_name: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(max = 100, message = "id_card must be at most 100 characters"))]
    pub id_card: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(max = 50, message = "tel must be at most 50 characters"))]
    pub tel: Option<String>,

    #[validate(length(min = 1, max = 255, message = "email must be 1-255 characters"))]
    pub email: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(max = 50, message = "gender must be at most 50 characters"))]
    pub gender: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(max = 36, message = "country_guid must be a 36-char UUID"))]
    pub country_guid: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(max = 255, message = "province must be at most 255 characters"))]
    pub province: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(max = 255, message = "district must be at most 255 characters"))]
    pub district: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(max = 255, message = "sub_district must be at most 255 characters"))]
    pub sub_district: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(max = 255, message = "village must be at most 255 characters"))]
    pub village: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(max = 50, message = "post must be at most 50 characters"))]
    pub post: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(max = 4000, message = "description must be at most 4000 characters"))]
    pub description: Option<String>,

    pub is_foreign: bool,
    pub is_customer_company: bool,
    pub is_customer: bool,
    pub is_admin: bool,
    pub is_employee: bool,
    pub is_freelance: bool,

    #[validate(custom(function = "validate_user_status", message = "status must be 0 or 1"))]
    pub status: i32,

    #[validate(length(min = 3, max = 255, message = "username must be 3-255 characters"))]
    pub username: String,

    #[validate(length(min = 8, max = 128, message = "password must be 8-128 characters"))]
    pub password: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(max = 500, message = "profile_img_path must be at most 500 characters"))]
    pub profile_img_path: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_img_b64: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(max = 36, message = "company_guid must be a 36-char UUID"))]
    pub company_guid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(max = 255, message = "company_name must be at most 255 characters"))]
    pub company_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(max = 50, message = "company_tel must be at most 50 characters"))]
    pub company_tel: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub company_type: Option<i32>,
    pub company_status: i32,

    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(max = 36, message = "department_guid must be a 36-char UUID"))]
    pub department_guid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(max = 36, message = "department_team_guid must be a 36-char UUID"))]
    pub department_team_guid: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(max = 36, message = "position_guid must be a 36-char UUID"))]
    pub position_guid: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub position_start_at: Option<DateTime<Utc>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub salary_amount: Option<Decimal>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(max = 10, message = "salary_currency must be at most 10 characters"))]
    pub salary_currency: Option<String>,

    #[validate(nested)]
    pub schedule: WeeklyScheduleDto,

    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(max = 255, message = "bank_name must be at most 255 characters"))]
    pub bank_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(max = 50, message = "bank_code must be at most 50 characters"))]
    pub bank_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(max = 100, message = "bank_account_no must be at most 100 characters"))]
    pub bank_account_no: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(
        max = 255,
        message = "bank_account_name must be at most 255 characters"
    ))]
    pub bank_account_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(
        max = 500,
        message = "bank_book_img_path must be at most 500 characters"
    ))]
    pub bank_book_img_path: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub bank_book_img_b64: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(
        max = 500,
        message = "id_card_front_path must be at most 500 characters"
    ))]
    pub id_card_front_path: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub id_card_front_b64: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(
        max = 500,
        message = "id_card_back_path must be at most 500 characters"
    ))]
    pub id_card_back_path: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub id_card_back_b64: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(
        max = 500,
        message = "proof_of_address_path must be at most 500 characters"
    ))]
    pub proof_of_address_path: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub proof_of_address_b64: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(
        max = 500,
        message = "source_of_funds_statement_path must be at most 500 characters"
    ))]
    pub source_of_funds_statement_path: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_of_funds_statement_b64: Option<String>,
}

#[derive(Debug, Deserialize, Validate, utoipa::ToSchema)]
pub struct AdminUpdateUserRequest {
    #[validate(length(min = 1, max = 100, message = "first_name must be 1-100 characters"))]
    pub first_name: String,

    #[validate(length(min = 1, max = 100, message = "last_name must be 1-100 characters"))]
    pub last_name: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(max = 100, message = "id_card must be at most 100 characters"))]
    pub id_card: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(max = 50, message = "tel must be at most 50 characters"))]
    pub tel: Option<String>,

    #[validate(length(min = 1, max = 255, message = "email must be 1-255 characters"))]
    pub email: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(max = 50, message = "gender must be at most 50 characters"))]
    pub gender: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(max = 36, message = "country_guid must be a 36-char UUID"))]
    pub country_guid: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(max = 255, message = "province must be at most 255 characters"))]
    pub province: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(max = 255, message = "district must be at most 255 characters"))]
    pub district: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(max = 255, message = "sub_district must be at most 255 characters"))]
    pub sub_district: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(max = 255, message = "village must be at most 255 characters"))]
    pub village: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(max = 50, message = "post must be at most 50 characters"))]
    pub post: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(max = 4000, message = "description must be at most 4000 characters"))]
    pub description: Option<String>,

    pub is_foreign: bool,
    pub is_customer_company: bool,
    pub is_customer: bool,
    pub is_admin: bool,
    pub is_employee: bool,
    pub is_freelance: bool,

    #[validate(custom(function = "validate_user_status", message = "status must be 0 or 1"))]
    pub status: i32,

    #[validate(length(min = 3, max = 255, message = "username must be 3-255 characters"))]
    pub username: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(max = 500, message = "profile_img_path must be at most 500 characters"))]
    pub profile_img_path: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_img_b64: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(max = 36, message = "company_guid must be a 36-char UUID"))]
    pub company_guid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(max = 255, message = "company_name must be at most 255 characters"))]
    pub company_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(max = 50, message = "company_tel must be at most 50 characters"))]
    pub company_tel: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub company_type: Option<i32>,
    pub company_status: i32,

    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(max = 36, message = "department_guid must be a 36-char UUID"))]
    pub department_guid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(max = 36, message = "department_team_guid must be a 36-char UUID"))]
    pub department_team_guid: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(max = 36, message = "position_guid must be a 36-char UUID"))]
    pub position_guid: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub position_start_at: Option<DateTime<Utc>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub salary_amount: Option<Decimal>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(max = 10, message = "salary_currency must be at most 10 characters"))]
    pub salary_currency: Option<String>,

    #[validate(nested)]
    pub schedule: WeeklyScheduleDto,

    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(max = 255, message = "bank_name must be at most 255 characters"))]
    pub bank_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(max = 50, message = "bank_code must be at most 50 characters"))]
    pub bank_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(max = 100, message = "bank_account_no must be at most 100 characters"))]
    pub bank_account_no: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(
        max = 255,
        message = "bank_account_name must be at most 255 characters"
    ))]
    pub bank_account_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(
        max = 500,
        message = "bank_book_img_path must be at most 500 characters"
    ))]
    pub bank_book_img_path: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub bank_book_img_b64: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(
        max = 500,
        message = "id_card_front_path must be at most 500 characters"
    ))]
    pub id_card_front_path: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub id_card_front_b64: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(
        max = 500,
        message = "id_card_back_path must be at most 500 characters"
    ))]
    pub id_card_back_path: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub id_card_back_b64: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(
        max = 500,
        message = "proof_of_address_path must be at most 500 characters"
    ))]
    pub proof_of_address_path: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub proof_of_address_b64: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[validate(length(
        max = 500,
        message = "source_of_funds_statement_path must be at most 500 characters"
    ))]
    pub source_of_funds_statement_path: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_of_funds_statement_b64: Option<String>,
}

fn validate_user_status(s: i32) -> Result<(), ValidationError> {
    if s == 0 || s == 1 {
        Ok(())
    } else {
        Err(ValidationError::new("invalid_user_status"))
    }
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct AdminInsertUserResponse {
    pub user_guid: String,

    pub user_username_guid: String,

    pub username: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub assigned_role_guid: Option<String>,
}

impl From<AdminInsertUserResult> for AdminInsertUserResponse {
    fn from(r: AdminInsertUserResult) -> Self {
        Self {
            user_guid: r.user_guid,
            user_username_guid: r.user_username_guid,
            username: r.username,
            assigned_role_guid: r.assigned_role_guid,
        }
    }
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct AdminUpdateUserResponse {
    pub user_guid: String,
}

impl From<AdminUpdateUserResult> for AdminUpdateUserResponse {
    fn from(r: AdminUpdateUserResult) -> Self {
        Self {
            user_guid: r.user_guid,
        }
    }
}

#[utoipa::path(
    post,
    path = "/api/v1/users/full",
    tag = "admin",
    request_body = AdminInsertUserRequest,
    responses(
        (status = 201, description = "User created (admin-provisioned)", body = AdminInsertUserResponse),
        (status = 400, description = "Malformed JSON body or ACTOR_REQUIRED", body = crate::openapi::ApiError),
        (status = 401, description = "Not authenticated or ACTOR_NOT_FOUND", body = crate::openapi::ApiError),
        (status = 403, description = "Not an admin (PERMISSION_DENIED)", body = crate::openapi::ApiError),
        (status = 409, description = "Username / email / id_card / user_guid collision", body = crate::openapi::ApiError),
        (status = 422, description = "Validation error (required field, role/position/company not found)", body = crate::openapi::ApiError),
        (status = 500, description = "Internal server error (ADMIN/EMPLOYEE role missing)", body = crate::openapi::ApiError),
    ),
    security(("bearer_auth" = []))
)]
pub async fn admin_insert_user_full(
    State(state): State<AppState>,
    user: AuthnUser,
    ValidatedJson(req): ValidatedJson<AdminInsertUserRequest>,
) -> Result<Response, Response> {
    let locale = current_locale();
    assert_scope_admin_page(&user, tr("err_auth.forbidden", &locale, &[]))?;

    if !user
        .has_permission(Permission::UsersCreate, &state.permission_checker)
        .await
    {
        let code_str = Permission::UsersCreate.code();
        let localized = tr("err_auth.permission_denied", &locale, &[code_str]);

        return Err(ApiError::from(AppError::Localized {
            status: StatusCode::FORBIDDEN,
            code: ErrorCode::PERMISSION_DENIED,
            message: localized,
        })
        .into_response());
    }

    let schedule = kokkak_domain::admin_user::WeeklySchedule {
        monday: dto_to_day_schedule(&req.schedule.monday),
        tuesday: dto_to_day_schedule(&req.schedule.tuesday),
        wednesday: dto_to_day_schedule(&req.schedule.wednesday),
        thursday: dto_to_day_schedule(&req.schedule.thursday),
        friday: dto_to_day_schedule(&req.schedule.friday),
        saturday: dto_to_day_schedule(&req.schedule.saturday),
        sunday: dto_to_day_schedule(&req.schedule.sunday),
    };

    let user_guid = match req.user_guid.as_deref() {
        Some(g) => g.to_string(),
        None => Uuid::now_v7().to_string(),
    };

    let profile_img_path = match resolve_b64_image(
        state.image.clone(),
        &user_guid,
        req.profile_img_b64.as_deref(),
        UserImageKind::Profile,
    )
    .await
    {
        Ok(v) => v.or(req.profile_img_path),
        Err(e) => return Err(image_error_envelope(&state, "profile_img_b64", e)),
    };
    let bank_book_img_path = match resolve_b64_image(
        state.image.clone(),
        &user_guid,
        req.bank_book_img_b64.as_deref(),
        UserImageKind::BankBook,
    )
    .await
    {
        Ok(v) => v.or(req.bank_book_img_path),
        Err(e) => return Err(image_error_envelope(&state, "bank_book_img_b64", e)),
    };
    let id_card_front_path = match resolve_b64_image(
        state.image.clone(),
        &user_guid,
        req.id_card_front_b64.as_deref(),
        UserImageKind::Attachment(kokkak_infra::storage::UserAttachment::IdCardFront),
    )
    .await
    {
        Ok(v) => v.or(req.id_card_front_path),
        Err(e) => return Err(image_error_envelope(&state, "id_card_front_b64", e)),
    };
    let id_card_back_path = match resolve_b64_image(
        state.image.clone(),
        &user_guid,
        req.id_card_back_b64.as_deref(),
        UserImageKind::Attachment(kokkak_infra::storage::UserAttachment::IdCardBack),
    )
    .await
    {
        Ok(v) => v.or(req.id_card_back_path),
        Err(e) => return Err(image_error_envelope(&state, "id_card_back_b64", e)),
    };
    let proof_of_address_path = match resolve_b64_image(
        state.image.clone(),
        &user_guid,
        req.proof_of_address_b64.as_deref(),
        UserImageKind::Attachment(kokkak_infra::storage::UserAttachment::ProofOfAddress),
    )
    .await
    {
        Ok(v) => v.or(req.proof_of_address_path),
        Err(e) => return Err(image_error_envelope(&state, "proof_of_address_b64", e)),
    };
    let source_of_funds_statement_path = match resolve_b64_image(
        state.image.clone(),
        &user_guid,
        req.source_of_funds_statement_b64.as_deref(),
        UserImageKind::Attachment(kokkak_infra::storage::UserAttachment::SourceOfFunds),
    )
    .await
    {
        Ok(v) => v.or(req.source_of_funds_statement_path),
        Err(e) => {
            return Err(image_error_envelope(
                &state,
                "source_of_funds_statement_b64",
                e,
            ));
        }
    };

    let input = AdminInsertUserFullInput {
        user_guid: Some(user_guid),
        first_name: req.first_name,
        last_name: req.last_name,
        id_card: req.id_card,
        tel: req.tel,
        email: req.email,
        gender: req.gender,
        country_guid: req.country_guid,
        province: req.province,
        district: req.district,
        sub_district: req.sub_district,
        village: req.village,
        post: req.post,
        description: req.description,
        is_foreign: req.is_foreign,
        is_customer_company: req.is_customer_company,
        is_customer: req.is_customer,
        is_admin: req.is_admin,
        is_employee: req.is_employee,
        is_freelance: req.is_freelance,
        status: req.status,
        username: req.username,
        password: req.password,
        profile_img_path,
        company_guid: req.company_guid,
        company_name: req.company_name,
        company_tel: req.company_tel,
        company_type: req.company_type,
        company_status: req.company_status,
        department_guid: req.department_guid,
        department_team_guid: req.department_team_guid,
        position_guid: req.position_guid,
        position_start_at: req.position_start_at,
        salary_amount: req.salary_amount,
        salary_currency: req.salary_currency,
        schedule,
        bank_name: req.bank_name,
        bank_code: req.bank_code,
        bank_account_no: req.bank_account_no,
        bank_account_name: req.bank_account_name,
        bank_book_img_path,
        id_card_front_path,
        id_card_back_path,
        proof_of_address_path,
        source_of_funds_statement_path,
    };

    let result = match state.admin_users.admin_insert_full(user.id(), input).await {
        Ok(r) => r,
        Err(e) => return Err(sp_error_envelope(&state, e)),
    };

    Ok((
        StatusCode::CREATED,
        created(AdminInsertUserResponse::from(result)),
    )
        .into_response())
}

#[utoipa::path(
        put,
        path = "/api/v1/users/{guid}/full",
        tag = "admin",
        params(
            ("guid" = Uuid, Path, description = "User GUID (36-char UUID)"),
        ),
        request_body = AdminUpdateUserRequest,
        responses(
            (status = 200, description = "User updated", body = AdminUpdateUserResponse),
            (status = 400, description = "Malformed JSON body or ACTOR_REQUIRED", body = crate::openapi::ApiError),
            (status = 401, description = "Not authenticated or ACTOR_NOT_FOUND", body = crate::openapi::ApiError),
            (status = 403, description = "Not an admin (PERMISSION_DENIED)", body = crate::openapi::ApiError),
            (status = 404, description = "User not found (USER_NOT_FOUND)", body = crate::openapi::ApiError),
            (status = 409, description = "Username / email / id_card collision", body = crate::openapi::ApiError),
            (status = 422, description = "Validation error (required field, role/position/company not found)", body = crate::openapi::ApiError),
            (status = 500, description = "Internal server error (ADMIN/EMPLOYEE role missing)", body = crate::openapi::ApiError),
        ),
        security(("bearer_auth" = []))
    )]
pub async fn admin_update_user_full(
    State(state): State<AppState>,
    user: AuthnUser,
    Path(guid): Path<Uuid>,
    ValidatedJson(req): ValidatedJson<AdminUpdateUserRequest>,
) -> Result<Response, Response> {
    let locale = current_locale();
    assert_scope_admin_page(&user, tr("err_auth.forbidden", &locale, &[]))?;

    if !user
        .has_permission(Permission::UsersUpdate, &state.permission_checker)
        .await
    {
        let code_str = Permission::UsersUpdate.code();
        let localized = tr("err_auth.permission_denied", &locale, &[code_str]);
        return Err(ApiError::from(AppError::Localized {
            status: StatusCode::FORBIDDEN,
            code: ErrorCode::PERMISSION_DENIED,
            message: localized,
        })
        .into_response());
    }

    let schedule = kokkak_domain::admin_user::WeeklySchedule {
        monday: dto_to_day_schedule(&req.schedule.monday),
        tuesday: dto_to_day_schedule(&req.schedule.tuesday),
        wednesday: dto_to_day_schedule(&req.schedule.wednesday),
        thursday: dto_to_day_schedule(&req.schedule.thursday),
        friday: dto_to_day_schedule(&req.schedule.friday),
        saturday: dto_to_day_schedule(&req.schedule.saturday),
        sunday: dto_to_day_schedule(&req.schedule.sunday),
    };

    let user_guid_str = guid.to_string();

    let profile_img_path = match resolve_b64_image(
        state.image.clone(),
        &user_guid_str,
        req.profile_img_b64.as_deref(),
        UserImageKind::Profile,
    )
    .await
    {
        Ok(v) => v.or(req.profile_img_path),
        Err(e) => return Err(image_error_envelope(&state, "profile_img_b64", e)),
    };
    let bank_book_img_path = match resolve_b64_image(
        state.image.clone(),
        &user_guid_str,
        req.bank_book_img_b64.as_deref(),
        UserImageKind::BankBook,
    )
    .await
    {
        Ok(v) => v.or(req.bank_book_img_path),
        Err(e) => return Err(image_error_envelope(&state, "bank_book_img_b64", e)),
    };
    let id_card_front_path = match resolve_b64_image(
        state.image.clone(),
        &user_guid_str,
        req.id_card_front_b64.as_deref(),
        UserImageKind::Attachment(kokkak_infra::storage::UserAttachment::IdCardFront),
    )
    .await
    {
        Ok(v) => v.or(req.id_card_front_path),
        Err(e) => return Err(image_error_envelope(&state, "id_card_front_b64", e)),
    };
    let id_card_back_path = match resolve_b64_image(
        state.image.clone(),
        &user_guid_str,
        req.id_card_back_b64.as_deref(),
        UserImageKind::Attachment(kokkak_infra::storage::UserAttachment::IdCardBack),
    )
    .await
    {
        Ok(v) => v.or(req.id_card_back_path),
        Err(e) => return Err(image_error_envelope(&state, "id_card_back_b64", e)),
    };
    let proof_of_address_path = match resolve_b64_image(
        state.image.clone(),
        &user_guid_str,
        req.proof_of_address_b64.as_deref(),
        UserImageKind::Attachment(kokkak_infra::storage::UserAttachment::ProofOfAddress),
    )
    .await
    {
        Ok(v) => v.or(req.proof_of_address_path),
        Err(e) => return Err(image_error_envelope(&state, "proof_of_address_b64", e)),
    };
    let source_of_funds_statement_path = match resolve_b64_image(
        state.image.clone(),
        &user_guid_str,
        req.source_of_funds_statement_b64.as_deref(),
        UserImageKind::Attachment(kokkak_infra::storage::UserAttachment::SourceOfFunds),
    )
    .await
    {
        Ok(v) => v.or(req.source_of_funds_statement_path),
        Err(e) => {
            return Err(image_error_envelope(
                &state,
                "source_of_funds_statement_b64",
                e,
            ));
        }
    };

    let input = AdminUpdateUserFullInput {
        user_guid: user_guid_str,
        first_name: req.first_name,
        last_name: req.last_name,
        id_card: req.id_card,
        tel: req.tel,
        email: req.email,
        gender: req.gender,
        country_guid: req.country_guid,
        province: req.province,
        district: req.district,
        sub_district: req.sub_district,
        village: req.village,
        post: req.post,
        description: req.description,
        is_foreign: req.is_foreign,
        is_customer_company: req.is_customer_company,
        is_customer: req.is_customer,
        is_admin: req.is_admin,
        is_employee: req.is_employee,
        is_freelance: req.is_freelance,
        status: req.status,
        username: req.username,
        profile_img_path,
        company_guid: req.company_guid,
        company_name: req.company_name,
        company_tel: req.company_tel,
        company_type: req.company_type,
        company_status: req.company_status,
        department_guid: req.department_guid,
        department_team_guid: req.department_team_guid,
        position_guid: req.position_guid,
        position_start_at: req.position_start_at,
        salary_amount: req.salary_amount,
        salary_currency: req.salary_currency,
        schedule,
        bank_name: req.bank_name,
        bank_code: req.bank_code,
        bank_account_no: req.bank_account_no,
        bank_account_name: req.bank_account_name,
        bank_book_img_path,
        id_card_front_path,
        id_card_back_path,
        proof_of_address_path,
        source_of_funds_statement_path,
    };

    let result = match state.admin_users.admin_update_full(user.id(), input).await {
        Ok(r) => r,
        Err(e) => return Err(sp_update_error_envelope(&state, e)),
    };

    if let Err(e) = state.permission_checker.invalidate_user(guid).await {
        tracing::warn!(
            user_guid = %guid,
            error = %e,
            "permission cache invalidation after user update failed (non-fatal)"
        );
    }

    Ok((StatusCode::OK, ok(AdminUpdateUserResponse::from(result))).into_response())
}

#[utoipa::path(
    delete,
    path = "/api/v1/users/{guid}",
    tag = "users",
    params(
        ("guid" = Uuid, Path, description = "User GUID to delete"),
    ),
    responses(
        (status = 200, description = "User deleted", body = AdminDeleteUserResponse),
        (status = 401, description = "Not authenticated", body = crate::openapi::ApiError),
        (status = 403, description = "Permission denied", body = crate::openapi::ApiError),
        (status = 404, description = "User not found", body = crate::openapi::ApiError),
    ),
    security(("bearer_auth" = []))
)]
pub async fn delete_user_admin(
    State(state): State<AppState>,
    user: AuthnUser,
    Path(guid): Path<Uuid>,
) -> Result<Response, Response> {
    let locale = current_locale();
    assert_scope_admin_page(&user, tr("err_auth.forbidden", &locale, &[]))?;

    if !user
        .has_permission(Permission::UsersDelete, &state.permission_checker)
        .await
    {
        let code_str = Permission::UsersDelete.code();
        let localized = tr("err_auth.permission_denied", &locale, &[code_str]);
        return Err(ApiError::from(AppError::Localized {
            status: StatusCode::FORBIDDEN,
            code: ErrorCode::PERMISSION_DENIED,
            message: localized,
        })
        .into_response());
    }

    let user_guid_str = guid.to_string();

    let result = match state
        .admin_users
        .admin_delete_user(user.id(), &user_guid_str)
        .await
    {
        Ok(r) => r,
        Err(e) => return Err(sp_delete_error_envelope(&state, e)),
    };

    if let Err(e) = state.permission_checker.invalidate_user(guid).await {
        tracing::warn!(
            user_guid = %guid,
            error = %e,
            "permission cache invalidation after user delete failed (non-fatal)"
        );
    }

    let locale = current_locale();
    let i18n_key = sp_delete_status_key(&result.code);
    let localized = tr(i18n_key, &locale, &[]);
    let resp = ApiResponse {
        success: true,
        data: Some(serde_json::json!({
            "user_guid": result.user_guid,
            "code": result.code,
            "message": localized,
        })),
        error: None,
        meta: None,
    };
    Ok((StatusCode::OK, Json(resp)).into_response())
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct AdminDeleteUserResponse {
    pub user_guid: String,
    pub code: String,
    pub message: String,
}

impl From<kokkak_domain::AdminDeleteUserResult> for AdminDeleteUserResponse {
    fn from(r: kokkak_domain::AdminDeleteUserResult) -> Self {
        Self {
            user_guid: r.user_guid,
            code: r.code,
            message: r.message,
        }
    }
}

#[utoipa::path(
    post,
    path = "/api/v1/users/{guid}/suspend",
    tag = "users",
    params(
        ("guid" = Uuid, Path, description = "User GUID to suspend"),
    ),
    responses(
        (status = 200, description = "User suspended", body = AdminDeleteUserResponse),
        (status = 401, description = "Not authenticated", body = crate::openapi::ApiError),
        (status = 403, description = "Permission denied", body = crate::openapi::ApiError),
        (status = 404, description = "User not found", body = crate::openapi::ApiError),
    ),
    security(("bearer_auth" = []))
)]
pub async fn suspend_user_admin(
    State(state): State<AppState>,
    user: AuthnUser,
    Path(guid): Path<Uuid>,
) -> Result<Response, Response> {
    let locale = current_locale();
    assert_scope_admin_page(&user, tr("err_auth.forbidden", &locale, &[]))?;

    if !user
        .has_permission(Permission::UsersUpdate, &state.permission_checker)
        .await
    {
        let code_str = Permission::UsersUpdate.code();
        let localized = tr("err_auth.permission_denied", &locale, &[code_str]);
        return Err(ApiError::from(AppError::Localized {
            status: StatusCode::FORBIDDEN,
            code: ErrorCode::PERMISSION_DENIED,
            message: localized,
        })
        .into_response());
    }

    let user_guid_str = guid.to_string();

    let result = match state
        .admin_users
        .admin_suspend_user(user.id(), &user_guid_str)
        .await
    {
        Ok(r) => r,
        Err(e) => return Err(sp_suspend_error_envelope(&state, e)),
    };

    if let Err(e) = state.permission_checker.invalidate_user(guid).await {
        tracing::warn!(
            user_guid = %guid,
            error = %e,
            "permission cache invalidation after user suspend failed (non-fatal)"
        );
    }

    let locale = current_locale();
    let i18n_key = sp_suspend_status_key(&result.code);
    let localized = tr(i18n_key, &locale, &[]);
    let resp = ApiResponse {
        success: true,
        data: Some(serde_json::json!({
            "user_guid": result.user_guid,
            "code": result.code,
            "message": localized,
        })),
        error: None,
        meta: None,
    };
    Ok((StatusCode::OK, Json(resp)).into_response())
}

fn sp_suspend_error_envelope(state: &AppState, err: AdminDeleteUserError) -> Response {
    let (status, code, i18n_key) = sp_suspend_status(&err.code);
    let locale = current_locale();
    let localized = tr(i18n_key, &locale, &[]);
    tracing::warn!(
        sp_code = %err.code,
        sp_message = %err.message,
        localized_code = code,
        "SP_USER_SUSPEND rejected request"
    );
    let envelope: ApiResponse<()> = ApiResponse {
        success: false,
        data: None,
        error: Some(kokkak_common::error::ApiErrorBody {
            code: code.into(),
            message: localized,
        }),
        meta: None,
    };
    let _ = state;
    (status, Json(envelope)).into_response()
}

fn sp_suspend_status(sp_code: &str) -> (StatusCode, &'static str, &'static str) {
    match sp_code {
        "ACTOR_REQUIRED" => (
            StatusCode::BAD_REQUEST,
            ErrorCode::ACTOR_REQUIRED,
            "err_admin_user.actor_required",
        ),
        "USER_GUID_REQUIRED" => (
            StatusCode::BAD_REQUEST,
            ErrorCode::VALIDATION,
            "err_admin_user.user_guid_required",
        ),
        "ACTOR_NOT_FOUND" => (
            StatusCode::UNAUTHORIZED,
            ErrorCode::ACTOR_NOT_FOUND,
            "err_admin_user.actor_not_found",
        ),
        "PERMISSION_DENIED" => (
            StatusCode::FORBIDDEN,
            ErrorCode::PERMISSION_DENIED,
            "err_admin_user.permission_denied",
        ),
        "USER_NOT_FOUND" => (
            StatusCode::NOT_FOUND,
            ErrorCode::USER_NOT_FOUND,
            "err_admin_user.user_not_found",
        ),
        "USER_DELETED" => (
            StatusCode::CONFLICT,
            "user_deleted",
            "err_admin_user.user_deleted_cannot_suspend",
        ),
        "ALREADY_SUSPENDED" => (
            StatusCode::OK,
            "already_suspended",
            "err_admin_user.already_suspended",
        ),
        "SUSPENDED" => (StatusCode::OK, "suspended", "err_admin_user.suspended"),
        "ERROR" => (
            StatusCode::INTERNAL_SERVER_ERROR,
            ErrorCode::INTERNAL,
            "err.internal",
        ),
        _ => (
            StatusCode::INTERNAL_SERVER_ERROR,
            ErrorCode::INTERNAL,
            "err.internal",
        ),
    }
}

fn sp_suspend_status_key(sp_code: &str) -> &'static str {
    match sp_code {
        "SUSPENDED" => "err_admin_user.suspended",
        "ALREADY_SUSPENDED" => "err_admin_user.already_suspended",
        "USER_DELETED" => "err_admin_user.user_deleted_cannot_suspend",
        _ => "err.internal",
    }
}

fn sp_delete_error_envelope(state: &AppState, err: AdminDeleteUserError) -> Response {
    let (status, code, i18n_key) = sp_delete_status(&err.code);
    let locale = current_locale();
    let localized = tr(i18n_key, &locale, &[]);
    tracing::warn!(
        sp_code = %err.code,
        sp_message = %err.message,
        localized_code = code,
        "SP_USER_DELETE rejected request"
    );
    let envelope: ApiResponse<()> = ApiResponse {
        success: false,
        data: None,
        error: Some(kokkak_common::error::ApiErrorBody {
            code: code.into(),
            message: localized,
        }),
        meta: None,
    };
    let _ = state;
    (status, Json(envelope)).into_response()
}

fn sp_delete_status(sp_code: &str) -> (StatusCode, &'static str, &'static str) {
    match sp_code {
        "ACTOR_REQUIRED" => (
            StatusCode::BAD_REQUEST,
            ErrorCode::ACTOR_REQUIRED,
            "err_admin_user.actor_required",
        ),
        "USER_GUID_REQUIRED" => (
            StatusCode::BAD_REQUEST,
            ErrorCode::VALIDATION,
            "err_admin_user.user_guid_required",
        ),
        "ACTOR_NOT_FOUND" => (
            StatusCode::UNAUTHORIZED,
            ErrorCode::ACTOR_NOT_FOUND,
            "err_admin_user.actor_not_found",
        ),
        "PERMISSION_DENIED" => (
            StatusCode::FORBIDDEN,
            ErrorCode::PERMISSION_DENIED,
            "err_admin_user.permission_denied",
        ),
        "USER_NOT_FOUND" => (
            StatusCode::NOT_FOUND,
            ErrorCode::USER_NOT_FOUND,
            "err_admin_user.user_not_found",
        ),
        "ALREADY_DELETED" => (
            StatusCode::OK,
            "already_deleted",
            "err_admin_user.already_deleted",
        ),
        "DELETED" => (StatusCode::OK, "deleted", "err_admin_user.deleted"),
        "ERROR" => (
            StatusCode::INTERNAL_SERVER_ERROR,
            ErrorCode::INTERNAL,
            "err.internal",
        ),
        _ => (
            StatusCode::INTERNAL_SERVER_ERROR,
            ErrorCode::INTERNAL,
            "err.internal",
        ),
    }
}

fn sp_delete_status_key(sp_code: &str) -> &'static str {
    match sp_code {
        "DELETED" => "err_admin_user.deleted",
        "ALREADY_DELETED" => "err_admin_user.already_deleted",
        _ => "err.internal",
    }
}

fn sp_permission_update_status_key(sp_code: &str) -> &'static str {
    match sp_code {
        "UPDATED" => "err_permission.updated",
        "CREATED" => "err_permission.created",
        "NO_CHANGE" => "err_permission.no_change",
        "ROLE_NOT_FOUND" => "err_permission.role_not_found",
        _ => "err.internal",
    }
}

fn dto_to_day_schedule(d: &DayScheduleDto) -> kokkak_domain::admin_user::DaySchedule {
    let parse_hms = |s: &str| s.parse::<chrono::NaiveTime>().ok();
    kokkak_domain::admin_user::DaySchedule {
        is_working: d.is_working,
        start_time: d.start_time.as_deref().and_then(parse_hms),
        end_time: d.end_time.as_deref().and_then(parse_hms),
    }
}

async fn resolve_b64_image(
    processor: Arc<kokkak_infra::image_processor::ImageProcessor>,
    user_guid: &str,
    b64: Option<&str>,
    kind: kokkak_infra::image_processor::UserImageKind,
) -> Result<Option<String>, kokkak_infra::image_processor::ImageError> {
    let Some(s) = b64 else { return Ok(None) };
    let bytes =
        decode_base64_payload(s).map_err(kokkak_infra::image_processor::ImageError::Decode)?;
    let result = processor.process_and_store(&bytes, user_guid, kind).await?;
    Ok(Some(result.key.as_str().to_string()))
}

fn decode_base64_payload(s: &str) -> Result<Vec<u8>, String> {
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine as _;
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

fn image_error_envelope(
    state: &AppState,
    field: &str,
    err: kokkak_infra::image_processor::ImageError,
) -> Response {
    let locale = current_locale();
    tracing::warn!(
        field = field,
        error = %err,
        "image upload failed"
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

fn sp_error_envelope(state: &AppState, err: AdminInsertUserError) -> Response {
    let (status, code, i18n_key) = sp_insert_full_status(&err.code);
    let locale = current_locale();
    let localized = tr(i18n_key, &locale, &[]);

    tracing::warn!(
        sp_code = %err.code,
        sp_message = %err.message,
        localized_code = code,
        "SP_USER_INSERT_FULL rejected request"
    );
    let envelope: ApiResponse<()> = ApiResponse {
        success: false,
        data: None,
        error: Some(kokkak_common::error::ApiErrorBody {
            code: code.into(),
            message: localized,
        }),
        meta: None,
    };

    let _ = state;
    (status, Json(envelope)).into_response()
}

fn sp_insert_full_status(sp_code: &str) -> (StatusCode, &'static str, &'static str) {
    match sp_code {
        "actor_required" => (
            StatusCode::BAD_REQUEST,
            ErrorCode::ACTOR_REQUIRED,
            "err_admin_user.actor_required",
        ),
        "actor_not_found" => (
            StatusCode::UNAUTHORIZED,
            ErrorCode::ACTOR_NOT_FOUND,
            "err_admin_user.actor_not_found",
        ),
        "permission_denied" => (
            StatusCode::FORBIDDEN,
            ErrorCode::PERMISSION_DENIED,
            "err_admin_user.permission_denied",
        ),

        "first_name_required" => (
            StatusCode::UNPROCESSABLE_ENTITY,
            ErrorCode::FIRST_NAME_REQUIRED,
            "err_admin_user.first_name_required",
        ),
        "last_name_required" => (
            StatusCode::UNPROCESSABLE_ENTITY,
            ErrorCode::LAST_NAME_REQUIRED,
            "err_admin_user.last_name_required",
        ),
        "email_required" => (
            StatusCode::UNPROCESSABLE_ENTITY,
            ErrorCode::EMAIL_REQUIRED,
            "err_admin_user.email_required",
        ),
        "username_required" => (
            StatusCode::UNPROCESSABLE_ENTITY,
            ErrorCode::USERNAME_REQUIRED,
            "err_admin_user.username_required",
        ),
        "password_hash_required" => (
            StatusCode::UNPROCESSABLE_ENTITY,
            ErrorCode::PASSWORD_HASH_REQUIRED,
            "err_admin_user.password_hash_required",
        ),
        "invalid_user_status" => (
            StatusCode::UNPROCESSABLE_ENTITY,
            ErrorCode::INVALID_USER_STATUS,
            "err_admin_user.invalid_user_status",
        ),

        "user_guid_exists" => (
            StatusCode::CONFLICT,
            ErrorCode::USER_GUID_EXISTS,
            "err_admin_user.user_guid_exists",
        ),
        "username_exists" => (
            StatusCode::CONFLICT,
            ErrorCode::USERNAME_TAKEN,
            "err_admin_user.username_exists",
        ),
        "email_exists" => (
            StatusCode::CONFLICT,
            ErrorCode::EMAIL_TAKEN,
            "err_admin_user.email_exists",
        ),
        "id_card_exists" => (
            StatusCode::CONFLICT,
            ErrorCode::ID_CARD_TAKEN,
            "err_admin_user.id_card_exists",
        ),

        "country_required" => (
            StatusCode::UNPROCESSABLE_ENTITY,
            ErrorCode::COUNTRY_REQUIRED,
            "err_admin_user.country_required",
        ),
        "country_not_found" => (
            StatusCode::UNPROCESSABLE_ENTITY,
            ErrorCode::COUNTRY_NOT_FOUND,
            "err_admin_user.country_not_found",
        ),
        "company_required" => (
            StatusCode::UNPROCESSABLE_ENTITY,
            ErrorCode::COMPANY_REQUIRED,
            "err_admin_user.company_required",
        ),
        "company_not_found" => (
            StatusCode::UNPROCESSABLE_ENTITY,
            ErrorCode::COMPANY_NOT_FOUND,
            "err_admin_user.company_not_found",
        ),
        "department_not_found" => (
            StatusCode::UNPROCESSABLE_ENTITY,
            ErrorCode::DEPARTMENT_NOT_FOUND,
            "err_admin_user.department_not_found",
        ),
        "department_team_not_found" => (
            StatusCode::UNPROCESSABLE_ENTITY,
            ErrorCode::DEPARTMENT_TEAM_NOT_FOUND,
            "err_admin_user.department_team_not_found",
        ),
        "department_team_mismatch" => (
            StatusCode::UNPROCESSABLE_ENTITY,
            ErrorCode::DEPARTMENT_TEAM_MISMATCH,
            "err_admin_user.department_team_mismatch",
        ),
        "position_not_found" => (
            StatusCode::UNPROCESSABLE_ENTITY,
            ErrorCode::POSITION_NOT_FOUND,
            "err_admin_user.position_not_found",
        ),
        "invalid_salary" => (
            StatusCode::UNPROCESSABLE_ENTITY,
            ErrorCode::INVALID_SALARY,
            "err_admin_user.invalid_salary",
        ),
        "work_time_required" => (
            StatusCode::UNPROCESSABLE_ENTITY,
            ErrorCode::WORK_TIME_REQUIRED,
            "err_admin_user.work_time_required",
        ),

        "admin_role_not_found" => (
            StatusCode::INTERNAL_SERVER_ERROR,
            ErrorCode::ADMIN_ROLE_NOT_FOUND,
            "err_admin_user.admin_role_not_found",
        ),
        "employee_role_not_found" => (
            StatusCode::INTERNAL_SERVER_ERROR,
            ErrorCode::EMPLOYEE_ROLE_NOT_FOUND,
            "err_admin_user.employee_role_not_found",
        ),

        _ => (
            StatusCode::INTERNAL_SERVER_ERROR,
            ErrorCode::INTERNAL,
            "err.internal",
        ),
    }
}

fn sp_update_error_envelope(state: &AppState, err: AdminUpdateUserError) -> Response {
    let (status, code, i18n_key) = sp_update_full_status(&err.code);
    let locale = current_locale();
    let localized = tr(i18n_key, &locale, &[]);
    tracing::warn!(
        sp_code = %err.code,
        sp_message = %err.message,
        localized_code = code,
        "SP_USER_UPDATE_FULL rejected request"
    );
    let envelope: ApiResponse<()> = ApiResponse {
        success: false,
        data: None,
        error: Some(kokkak_common::error::ApiErrorBody {
            code: code.into(),
            message: localized,
        }),
        meta: None,
    };
    let _ = state;
    (status, Json(envelope)).into_response()
}

fn sp_update_full_status(sp_code: &str) -> (StatusCode, &'static str, &'static str) {
    match sp_code {
        "actor_required" => (
            StatusCode::BAD_REQUEST,
            ErrorCode::ACTOR_REQUIRED,
            "err_admin_user.actor_required",
        ),
        "actor_not_found" => (
            StatusCode::UNAUTHORIZED,
            ErrorCode::ACTOR_NOT_FOUND,
            "err_admin_user.actor_not_found",
        ),
        "permission_denied" => (
            StatusCode::FORBIDDEN,
            ErrorCode::PERMISSION_DENIED,
            "err_admin_user.permission_denied",
        ),

        "user_not_found" => (
            StatusCode::NOT_FOUND,
            ErrorCode::USER_NOT_FOUND,
            "err_admin_user.user_not_found",
        ),

        "first_name_required" => (
            StatusCode::UNPROCESSABLE_ENTITY,
            ErrorCode::FIRST_NAME_REQUIRED,
            "err_admin_user.first_name_required",
        ),
        "last_name_required" => (
            StatusCode::UNPROCESSABLE_ENTITY,
            ErrorCode::LAST_NAME_REQUIRED,
            "err_admin_user.last_name_required",
        ),
        "email_required" => (
            StatusCode::UNPROCESSABLE_ENTITY,
            ErrorCode::EMAIL_REQUIRED,
            "err_admin_user.email_required",
        ),
        "username_required" => (
            StatusCode::UNPROCESSABLE_ENTITY,
            ErrorCode::USERNAME_REQUIRED,
            "err_admin_user.username_required",
        ),
        "invalid_user_status" => (
            StatusCode::UNPROCESSABLE_ENTITY,
            ErrorCode::INVALID_USER_STATUS,
            "err_admin_user.invalid_user_status",
        ),

        "username_exists" => (
            StatusCode::CONFLICT,
            ErrorCode::USERNAME_TAKEN,
            "err_admin_user.username_exists",
        ),
        "email_exists" => (
            StatusCode::CONFLICT,
            ErrorCode::EMAIL_TAKEN,
            "err_admin_user.email_exists",
        ),
        "id_card_exists" => (
            StatusCode::CONFLICT,
            ErrorCode::ID_CARD_TAKEN,
            "err_admin_user.id_card_exists",
        ),

        "country_required" => (
            StatusCode::UNPROCESSABLE_ENTITY,
            ErrorCode::COUNTRY_REQUIRED,
            "err_admin_user.country_required",
        ),
        "country_not_found" => (
            StatusCode::UNPROCESSABLE_ENTITY,
            ErrorCode::COUNTRY_NOT_FOUND,
            "err_admin_user.country_not_found",
        ),
        "company_required" => (
            StatusCode::UNPROCESSABLE_ENTITY,
            ErrorCode::COMPANY_REQUIRED,
            "err_admin_user.company_required",
        ),
        "company_not_found" => (
            StatusCode::UNPROCESSABLE_ENTITY,
            ErrorCode::COMPANY_NOT_FOUND,
            "err_admin_user.company_not_found",
        ),
        "department_not_found" => (
            StatusCode::UNPROCESSABLE_ENTITY,
            ErrorCode::DEPARTMENT_NOT_FOUND,
            "err_admin_user.department_not_found",
        ),
        "department_team_not_found" => (
            StatusCode::UNPROCESSABLE_ENTITY,
            ErrorCode::DEPARTMENT_TEAM_NOT_FOUND,
            "err_admin_user.department_team_not_found",
        ),
        "department_team_mismatch" => (
            StatusCode::UNPROCESSABLE_ENTITY,
            ErrorCode::DEPARTMENT_TEAM_MISMATCH,
            "err_admin_user.department_team_mismatch",
        ),
        "position_not_found" => (
            StatusCode::UNPROCESSABLE_ENTITY,
            ErrorCode::POSITION_NOT_FOUND,
            "err_admin_user.position_not_found",
        ),
        "invalid_salary" => (
            StatusCode::UNPROCESSABLE_ENTITY,
            ErrorCode::INVALID_SALARY,
            "err_admin_user.invalid_salary",
        ),
        "work_time_required" => (
            StatusCode::UNPROCESSABLE_ENTITY,
            ErrorCode::WORK_TIME_REQUIRED,
            "err_admin_user.work_time_required",
        ),

        "admin_role_not_found" => (
            StatusCode::INTERNAL_SERVER_ERROR,
            ErrorCode::ADMIN_ROLE_NOT_FOUND,
            "err_admin_user.admin_role_not_found",
        ),
        "employee_role_not_found" => (
            StatusCode::INTERNAL_SERVER_ERROR,
            ErrorCode::EMPLOYEE_ROLE_NOT_FOUND,
            "err_admin_user.employee_role_not_found",
        ),

        _ => (
            StatusCode::INTERNAL_SERVER_ERROR,
            ErrorCode::INTERNAL,
            "err.internal",
        ),
    }
}

#[allow(dead_code)]
fn _hasher_anchor(_: &dyn PasswordHasherPort) {}

#[cfg(test)]
mod base64_decode_tests {

    use super::decode_base64_payload;

    const PLAIN: &str = "aGk=";

    #[test]
    fn decodes_plain_payload() {
        let out = decode_base64_payload(PLAIN).unwrap();
        assert_eq!(out, b"hi");
    }

    #[test]
    fn decodes_data_url_prefix() {
        let s = format!("data:image/jpeg;base64,{PLAIN}");
        assert_eq!(decode_base64_payload(&s).unwrap(), b"hi");
    }

    #[test]
    fn drops_ascii_whitespace() {
        let s = "data:image/png;base64,aGk=\n";
        assert_eq!(decode_base64_payload(s).unwrap(), b"hi");
        let s = "aG k=";
        assert_eq!(decode_base64_payload(s).unwrap(), b"hi");
    }

    #[test]
    fn rejects_invalid_base64() {
        assert!(decode_base64_payload("not!base64$").is_err());
    }
}

#[cfg(test)]
mod resolve_b64_image_tests {

    use super::*;
    use kokkak_infra::image_processor::{ImageProcessor, ImageProcessorConfig};
    use kokkak_infra::storage::MemoryStorage;

    fn proc() -> Arc<ImageProcessor> {
        Arc::new(ImageProcessor::new(
            Arc::new(MemoryStorage::new()),
            ImageProcessorConfig {
                max_input_bytes: 1024 * 1024,
                max_dimension_px: 256,
                webp_quality: 80,
            },
        ))
    }

    #[allow(dead_code)]
    fn tiny_png() -> Vec<u8> {
        vec![
            0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00,
            0x00, 0x90, 0x77, 0x53, 0xde, 0x00, 0x00, 0x00, 0x0c, 0x49, 0x44, 0x41, 0x54, 0x08,
            0xd7, 0x63, 0xf8, 0xcf, 0xc0, 0x00, 0x00, 0x00, 0x03, 0x00, 0x01, 0x5b, 0x6b, 0x4f,
            0xa6, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
        ]
    }

    #[tokio::test]
    async fn none_returns_none() {
        let p = proc();
        let r = resolve_b64_image(
            p,
            "u-1",
            None,
            kokkak_infra::image_processor::UserImageKind::Profile,
        )
        .await
        .unwrap();
        assert!(r.is_none());
    }

    #[tokio::test]
    async fn decodes_and_stores_under_kind_folder() {
        let p = proc();

        let b64 = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8/5+hHgAHggJ/PchI7wAAAABJRU5ErkJggg==";
        let r = resolve_b64_image(
            p,
            "u-1",
            Some(b64),
            kokkak_infra::image_processor::UserImageKind::Profile,
        )
        .await
        .unwrap();
        let key = r.expect("b64 was Some");
        assert!(key.starts_with("users/u-1/profile/"));
        assert!(key.ends_with(".webp"));
    }

    #[tokio::test]
    async fn attachment_kind_lands_in_subfolder() {
        let p = proc();
        let b64 = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8/5+hHgAHggJ/PchI7wAAAABJRU5ErkJggg==";
        let r = resolve_b64_image(
            p,
            "u-2",
            Some(b64),
            kokkak_infra::image_processor::UserImageKind::Attachment(
                kokkak_infra::storage::UserAttachment::IdCardFront,
            ),
        )
        .await
        .unwrap();
        let key = r.expect("b64 was Some");
        assert!(key.contains("/attachments/id-card-front/"));
    }

    #[tokio::test]
    async fn invalid_base64_surfaces_decode_error() {
        let p = proc();
        let r = resolve_b64_image(
            p,
            "u-3",
            Some("not!base64$"),
            kokkak_infra::image_processor::UserImageKind::Profile,
        )
        .await;
        assert!(matches!(
            r,
            Err(kokkak_infra::image_processor::ImageError::Decode(_))
        ));
    }
}

#[cfg(test)]
mod admin_insert_full_tests {

    use super::*;

    const KNOWN_SP_CODES: &[&str] = &[
        "actor_required",
        "actor_not_found",
        "permission_denied",
        "first_name_required",
        "last_name_required",
        "email_required",
        "username_required",
        "password_hash_required",
        "invalid_user_status",
        "user_guid_exists",
        "username_exists",
        "email_exists",
        "id_card_exists",
        "country_required",
        "country_not_found",
        "company_required",
        "company_not_found",
        "department_not_found",
        "department_team_not_found",
        "department_team_mismatch",
        "position_not_found",
        "invalid_salary",
        "work_time_required",
        "admin_role_not_found",
        "employee_role_not_found",
    ];

    #[test]
    fn all_known_sp_codes_resolve_to_distinct_statuses() {
        let mut seen = std::collections::HashSet::new();
        for code in KNOWN_SP_CODES {
            let (status, ec, key) = sp_insert_full_status(code);

            assert!(
                seen.insert((status, ec)),
                "duplicate (status, code) for SP code `{code}`: {status} + {ec}"
            );
            assert!(key.starts_with("err_"), "i18n key must live under err_*");
            assert!(
                key == "err.internal" || key.starts_with("err_admin_user."),
                "i18n key for admin-user codes must start with err_admin_user. (got `{key}`)"
            );
        }
    }

    #[test]
    fn actor_codes_map_to_400_401_403() {
        assert_eq!(
            sp_insert_full_status("actor_required").0,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            sp_insert_full_status("actor_not_found").0,
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            sp_insert_full_status("permission_denied").0,
            StatusCode::FORBIDDEN
        );
    }

    #[test]
    fn conflict_codes_map_to_409() {
        for code in [
            "user_guid_exists",
            "username_exists",
            "email_exists",
            "id_card_exists",
        ] {
            assert_eq!(
                sp_insert_full_status(code).0,
                StatusCode::CONFLICT,
                "{code} should map to 409"
            );
        }
    }

    #[test]
    fn validation_codes_map_to_422() {
        for code in [
            "first_name_required",
            "last_name_required",
            "email_required",
            "username_required",
            "password_hash_required",
            "invalid_user_status",
            "country_required",
            "country_not_found",
            "company_required",
            "company_not_found",
            "department_not_found",
            "department_team_not_found",
            "department_team_mismatch",
            "position_not_found",
            "invalid_salary",
            "work_time_required",
        ] {
            assert_eq!(
                sp_insert_full_status(code).0,
                StatusCode::UNPROCESSABLE_ENTITY,
                "{code} should map to 422"
            );
        }
    }

    #[test]
    fn role_seed_codes_map_to_500() {
        assert_eq!(
            sp_insert_full_status("admin_role_not_found").0,
            StatusCode::INTERNAL_SERVER_ERROR
        );
        assert_eq!(
            sp_insert_full_status("employee_role_not_found").0,
            StatusCode::INTERNAL_SERVER_ERROR
        );
    }

    #[test]
    fn unknown_code_falls_back_to_500_internal() {
        let (status, ec, key) = sp_insert_full_status("SOME_FUTURE_CODE");
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(ec, ErrorCode::INTERNAL);
        assert_eq!(key, "err.internal");
    }

    #[test]
    fn username_exists_maps_to_existing_username_taken_catalog_code() {
        assert_eq!(
            sp_insert_full_status("username_exists").1,
            ErrorCode::USERNAME_TAKEN
        );
    }
}
