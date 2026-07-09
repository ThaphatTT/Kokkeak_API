use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use kokkak_common::error::AppError;
use kokkak_common::error_codes::ErrorCode;
use kokkak_common::i18n::{current_locale, tr};
use kokkak_common::response::{paginated, ApiResponse, PageMeta};
use kokkak_domain::permission::PermissionOverrideUpdateItem;
use kokkak_domain::traits::user::RepoError;
use kokkak_domain::{Permission, PermissionUserGroup, PermissionUserListRow};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{ApiError, IntoLocalizedResponse};
use crate::middleware::auth::{assert_scope, AuthnUser};
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct ListUsersQuery {
    pub after: Option<String>,

    pub limit: Option<u32>,
}

pub async fn list_users_permission(
    State(state): State<AppState>,
    user: AuthnUser,
    Query(q): Query<ListUsersQuery>,
) -> Result<Response, Response> {
    let locale = current_locale();
    assert_scope(&user, "admin_page", tr("err_auth.forbidden", &locale, &[]))?;

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

    let limit = q.limit.unwrap_or(20).clamp(1, 100);

    let page = match state
        .permission
        .list_permission_users(q.after, limit, user.id())
        .await
    {
        Ok(p) => p,
        Err(e) => return Err(ApiError::from(e).into_localized_response(&state).await),
    };

    let meta = PageMeta {
        limit: limit as usize,
        has_next: page.next_cursor.is_some(),
        next_cursor: page.next_cursor,
    };
    Ok((StatusCode::OK, paginated(page.items, meta)).into_response())
}

pub async fn list_user_permissions_permission(
    State(state): State<AppState>,
    user: AuthnUser,
    Path(guid): Path<Uuid>,
) -> Result<Response, Response> {
    let locale = current_locale();
    assert_scope(&user, "admin_page", tr("err_auth.forbidden", &locale, &[]))?;

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

pub const MAX_BULK_PERMISSION_OVERRIDE_UPDATES: usize = 500;

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct UpdatePermissionOverridesRequest {
    #[serde(default)]
    pub items: Vec<PermissionOverrideUpdateItem>,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct UpdatePermissionOverridesResponse {
    pub total: usize,

    pub updated: usize,

    pub created: usize,

    pub failed: usize,

    pub results: Vec<kokkak_domain::PermissionOverrideUpdateResult>,
}

#[utoipa::path(
    post,
    path = "/api/v1/permission/overrides",
    tag = "permission",
    request_body = UpdatePermissionOverridesRequest,
    responses(
        (status = 200, description = "Per-item results (always 200; per-item `success` field carries the outcome)", body = UpdatePermissionOverridesResponse),
        (status = 400, description = "Malformed JSON body", body = crate::openapi::ApiError),
        (status = 401, description = "Not authenticated", body = crate::openapi::ApiError),
        (status = 403, description = "Not an admin", body = crate::openapi::ApiError),
        (status = 422, description = "Validation error (empty list, out-of-range, invalid effect/status)", body = crate::openapi::ApiError),
    ),
    security(("bearer_auth" = []))
)]
pub async fn update_permission_overrides(
    State(state): State<AppState>,
    user: AuthnUser,
    Json(req): Json<UpdatePermissionOverridesRequest>,
) -> Result<Response, Response> {
    let locale = current_locale();
    assert_scope(&user, "admin_page", tr("err_auth.forbidden", &locale, &[]))?;

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

    if req.items.is_empty() {
        let locale = current_locale();
        let msg = tr("err_permission.empty_override_list", &locale, &[]);
        return Err(validation_envelope(&msg, 0, "items"));
    }
    if req.items.len() > MAX_BULK_PERMISSION_OVERRIDE_UPDATES {
        let locale = current_locale();
        let msg = tr("err_permission.too_many_override_items", &locale, &[]);
        return Err(validation_envelope(&msg, 0, "items"));
    }

    for (i, item) in req.items.iter().enumerate() {
        let effect_lc = item.effect.to_lowercase();
        if effect_lc != "allow" && effect_lc != "deny" {
            let locale = current_locale();
            let msg = tr("err_permission.invalid_override_effect", &locale, &[]);
            return Err(validation_envelope(&msg, i, "effect"));
        }
        if let Some(s) = item.status {
            if s != 0 && s != 1 {
                let locale = current_locale();
                let msg = tr("err_permission.invalid_override_status", &locale, &[]);
                return Err(validation_envelope(&msg, i, "status"));
            }
        }
    }

    let actor = user.id();

    let results = match state
        .permission
        .update_permission_overrides(&req.items, actor)
        .await
    {
        Ok(r) => r,
        Err(e) => return Err(ApiError::from(e).into_localized_response(&state).await),
    };

    let mut updated = 0usize;
    let mut created = 0usize;
    let mut failed = 0usize;
    for r in &results {
        match r.code.as_str() {
            kokkak_domain::PermissionOverrideUpdateResult::CODE_UPDATED => updated += 1,
            kokkak_domain::PermissionOverrideUpdateResult::CODE_CREATED => created += 1,
            _ => failed += 1,
        }
    }
    let total = results.len();

    Ok((
        StatusCode::OK,
        Json(ApiResponse {
            success: true,
            data: Some(UpdatePermissionOverridesResponse {
                total,
                updated,
                created,
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
    let body = serde_json::json!({
        "success": false,
        "data": null,
        "error": {
            "code": "validation",
            "message": message,
            "details": { "index": index, "field": field }
        },
        "meta": null
    });
    (StatusCode::UNPROCESSABLE_ENTITY, Json(body)).into_response()
}

#[allow(dead_code)]
fn _type_anchor() -> Option<PermissionUserListRow> {
    None
}
