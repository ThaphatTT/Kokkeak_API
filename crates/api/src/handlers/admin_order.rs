use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use kokkak_common::i18n::{current_locale, tr};
use kokkak_common::response::ApiResponse;
use kokkak_domain::Permission;
use serde::{Deserialize, Serialize};

use crate::middleware::auth::{assert_scope_admin_page, AuthnUser};
use crate::state::AppState;

#[derive(Debug, Deserialize, utoipa::ToSchema, utoipa::IntoParams)]
pub struct ListAdminOrderQuery {
    pub keyword: Option<String>,
    pub workflow_status: Option<i32>,
    pub page: Option<u32>,
    pub page_size: Option<u32>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ListAdminOrderMeta {
    pub total_count: i64,
    pub page: u32,
    pub page_size: u32,
    pub total_page: u32,
    pub has_next: bool,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct ListAdminOrderResponse {
    pub items: Vec<kokkak_domain::AdminOrderRow>,
    pub meta: ListAdminOrderMeta,
}

#[utoipa::path(
    get,
    path = "/api/v1/order-services",
    tag = "order-services",
    params(ListAdminOrderQuery),
    responses(
        (status = 200, description = "Admin order list", body = ListAdminOrderResponse),
        (status = 401, description = "Not authenticated", body = crate::openapi::ApiError),
        (status = 403, description = "Permission denied", body = crate::openapi::ApiError),
    ),
    security(("bearer_auth" = []))
)]
pub async fn list_order_services_admin(
    State(state): State<AppState>,
    user: AuthnUser,
    Query(q): Query<ListAdminOrderQuery>,
) -> Result<Response, Response> {
    let locale = current_locale();
    assert_scope_admin_page(&user, tr("err_auth.forbidden", &locale, &[]))?;

    if !user
        .has_permission(Permission::PageJobsView, &state.permission_checker)
        .await
    {
        return Err(permission_denied(&state, "JOBS_VIEW"));
    }

    let page = q.page.unwrap_or(1).max(1);
    let page_size = q.page_size.unwrap_or(20).clamp(1, 100);

    let input = kokkak_domain::AdminOrderListInput {
        keyword: q.keyword,
        workflow_status: q.workflow_status,
        page,
        page_size,
    };

    let result = match state.admin_order_service.list(input).await {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(error = %e, "admin_order_service.list failed");
            return Err(map_repo_error(&state, e));
        }
    };

    let resp = ListAdminOrderResponse {
        items: result.items,
        meta: ListAdminOrderMeta {
            total_count: result.total_count,
            page: result.page,
            page_size: result.page_size,
            total_page: result.total_page,
            has_next: result.page < result.total_page,
        },
    };
    Ok((
        StatusCode::OK,
        Json(ApiResponse {
            success: true,
            data: Some(resp),
            error: None,
            meta: None,
        }),
    )
        .into_response())
}

#[utoipa::path(
    get,
    path = "/api/v1/order-services/{guid}",
    tag = "order-services",
    params(("guid" = String, Path, description = "Order service header GUID")),
    responses(
        (status = 200, description = "Order service detail", body = kokkak_domain::AdminOrderDetailRow),
        (status = 401, description = "Not authenticated", body = crate::openapi::ApiError),
        (status = 403, description = "Permission denied", body = crate::openapi::ApiError),
        (status = 404, description = "Not found", body = crate::openapi::ApiError),
    ),
    security(("bearer_auth" = []))
)]
pub async fn get_order_service_admin(
    State(state): State<AppState>,
    user: AuthnUser,
    Path(guid): Path<String>,
) -> Result<Response, Response> {
    let locale = current_locale();
    assert_scope_admin_page(&user, tr("err_auth.forbidden", &locale, &[]))?;

    if !user
        .has_permission(Permission::PageJobsView, &state.permission_checker)
        .await
    {
        return Err(permission_denied(&state, "JOBS_VIEW"));
    }

    let row = match state.admin_order_service.detail(&guid).await {
        Ok(Some(r)) => r,
        Ok(None) => {
            let envelope: ApiResponse<()> = ApiResponse {
                success: false,
                data: None,
                error: Some(kokkak_common::error::ApiErrorBody {
                    code: "not_found".into(),
                    message: tr("err.not_found", &locale, &[]),
                }),
                meta: None,
            };
            return Err((StatusCode::NOT_FOUND, Json(envelope)).into_response());
        }
        Err(e) => {
            tracing::warn!(error = %e, "admin_order_service.detail failed");
            return Err(map_repo_error(&state, e));
        }
    };

    Ok((
        StatusCode::OK,
        Json(ApiResponse {
            success: true,
            data: Some(row),
            error: None,
            meta: None,
        }),
    )
        .into_response())
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct UpdateOrderServiceRequest {
    pub workflow_status: Option<i32>,
    pub note: Option<String>,
}

impl UpdateOrderServiceRequest {
    pub fn validate(&self) -> Result<(), String> {
        if let Some(s) = self.workflow_status {
            if !(1..=99).contains(&s) {
                return Err("workflow_status must be between 1 and 99".to_string());
            }
        }
        if self.workflow_status.is_none() && self.note.is_none() {
            return Err("at least one of workflow_status or note is required".to_string());
        }
        Ok(())
    }
}

#[utoipa::path(
    put,
    path = "/api/v1/order-services/{guid}",
    tag = "order-services",
    params(("guid" = String, Path, description = "Order service header GUID")),
    request_body = UpdateOrderServiceRequest,
    responses(
        (status = 200, description = "Order service updated", body = kokkak_domain::AdminOrderUpdateResult),
        (status = 401, description = "Not authenticated", body = crate::openapi::ApiError),
        (status = 403, description = "Permission denied", body = crate::openapi::ApiError),
        (status = 404, description = "Not found", body = crate::openapi::ApiError),
        (status = 422, description = "Validation error", body = crate::openapi::ApiError),
    ),
    security(("bearer_auth" = []))
)]
pub async fn update_order_service_admin(
    State(state): State<AppState>,
    user: AuthnUser,
    Path(guid): Path<String>,
    Json(req): Json<UpdateOrderServiceRequest>,
) -> Result<Response, Response> {
    let locale = current_locale();
    assert_scope_admin_page(&user, tr("err_auth.forbidden", &locale, &[]))?;

    if !user
        .has_permission(Permission::JobsUpdate, &state.permission_checker)
        .await
    {
        return Err(permission_denied(&state, "JOBS_UPDATE"));
    }

    if let Err(msg) = req.validate() {
        return Err(validation_envelope(&state, &msg));
    }

    let actor = user.id().to_string();
    let input = kokkak_domain::AdminOrderUpdateInput {
        order_service_header_guid: guid,
        workflow_status: req.workflow_status,
        note: req.note,
        update_by: actor,
    };

    let result = match state.admin_order_service.update(input).await {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(error = %e, "admin_order_service.update failed");
            return Err(map_repo_error(&state, e));
        }
    };

    if !result.success {
        let status = match result.code.as_str() {
            "NOT_FOUND" => StatusCode::NOT_FOUND,
            _ => StatusCode::UNPROCESSABLE_ENTITY,
        };
        let i18n_key = sp_order_status_key(&result.code);
        let msg = tr(i18n_key, &locale, &[]);
        let envelope: ApiResponse<()> = ApiResponse {
            success: false,
            data: None,
            error: Some(kokkak_common::error::ApiErrorBody {
                code: result.code,
                message: msg,
            }),
            meta: None,
        };
        return Err((status, Json(envelope)).into_response());
    }

    Ok((
        StatusCode::OK,
        Json(ApiResponse {
            success: true,
            data: Some(result),
            error: None,
            meta: None,
        }),
    )
        .into_response())
}

#[utoipa::path(
    delete,
    path = "/api/v1/order-services/{guid}",
    tag = "order-services",
    params(("guid" = String, Path, description = "Order service header GUID")),
    responses(
        (status = 200, description = "Order service deleted", body = kokkak_domain::AdminOrderDeleteResult),
        (status = 401, description = "Not authenticated", body = crate::openapi::ApiError),
        (status = 403, description = "Permission denied", body = crate::openapi::ApiError),
        (status = 404, description = "Not found", body = crate::openapi::ApiError),
    ),
    security(("bearer_auth" = []))
)]
pub async fn delete_order_service_admin(
    State(state): State<AppState>,
    user: AuthnUser,
    Path(guid): Path<String>,
) -> Result<Response, Response> {
    let locale = current_locale();
    assert_scope_admin_page(&user, tr("err_auth.forbidden", &locale, &[]))?;

    if !user
        .has_permission(Permission::JobsDelete, &state.permission_checker)
        .await
    {
        return Err(permission_denied(&state, "JOBS_DELETE"));
    }

    let actor = user.id().to_string();
    let result = match state.admin_order_service.delete(&guid, &actor).await {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(error = %e, "admin_order_service.delete failed");
            return Err(map_repo_error(&state, e));
        }
    };

    if !result.success {
        let status = match result.code.as_str() {
            "NOT_FOUND" => StatusCode::NOT_FOUND,
            _ => StatusCode::UNPROCESSABLE_ENTITY,
        };
        let i18n_key = sp_order_status_key(&result.code);
        let msg = tr(i18n_key, &locale, &[]);
        let envelope: ApiResponse<()> = ApiResponse {
            success: false,
            data: None,
            error: Some(kokkak_common::error::ApiErrorBody {
                code: result.code,
                message: msg,
            }),
            meta: None,
        };
        return Err((status, Json(envelope)).into_response());
    }

    Ok((
        StatusCode::OK,
        Json(ApiResponse {
            success: true,
            data: Some(result),
            error: None,
            meta: None,
        }),
    )
        .into_response())
}

fn sp_order_status_key(code: &str) -> &'static str {
    match code {
        "UPDATE_SUCCESS" | "DELETE_SUCCESS" => "err_order.success",
        "NOT_FOUND" | "GUID_REQUIRED" => "err.not_found",
        "INVALID_STATUS" => "err_order.invalid_status",
        _ => "err.internal",
    }
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
