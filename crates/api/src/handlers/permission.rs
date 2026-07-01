//! Permission-page HTTP handlers (M17 — fully decoupled from
//! `UserRepository`).
//!
//! Three routes, all backed by the dedicated
//! [`kokkak_domain::PermissionUserRepository`]:
//!
//! - `GET /api/v1/permission/users` — paginated list (one row per
//!   user, shape: `user_guid` / `full_name` / `email` / `role_codes` /
//!   `role_names` / `has_permission` / `has_override` / status +
//!   timestamps).
//! - `GET /api/v1/permission/users/:guid/permissions` — per-user
//!   permission detail rows **grouped** by user identity (the wire
//!   shape the admin / permission UI consumes).
//! - `POST /api/v1/permission/overrides` — batch upsert of
//!   [`kokkak_domain::PermissionOverrideUpdateItem`] into
//!   `user_permission_override` (M18). One request item → one
//!   SP call → one per-item result row.
//!
//! ## Why this module is separate from `handlers::admin`
//!
//! The admin user-management screen and the permission page happen
//! to surface similar data today. They are still two different
//! flows with different route prefixes, different application
//! services, different SPs (since M17), and different evolution
//! paths. Coupling them at the SP / repository level forced a
//! GUID→username translation in Rust and forced shared CSV
//! `user_role_name` columns that neither screen actually wanted.
//!
//! M17 splits them apart cleanly.
//!
//! ## RBAC
//!
//! All three routes require an `Admin` or `SuperAdmin` JWT. The shared
//! `admin_flag` middleware enforces this at the router level — see
//! [`crate::router::build`].

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
use crate::middleware::auth::AuthnUser;
use crate::state::AppState;

// ============================================================================
// GET /api/v1/permission/users
// ============================================================================
//
// Permission-page user listing. Backed by
// `dbo.SP_PERMISSION_USER_LIST_V2` (one row per user, returns
// `role_codes` / `role_names` / `has_permission` / `has_override` /
// status + timestamps — see [`PermissionUserListRow`]).
// Cursor pagination is applied in the application service
// (`PermissionUserService::list_permission_users`) on top of the SP
// result, then surfaced through `paginated()` with the standard
// `PageMeta` envelope.

/// Query parameters for the permission-page user listing.
///
/// `after` is an opaque cursor (the `email` of the last item on the
/// previous page). `limit` defaults to 20 and is hard-capped at 100
/// so a runaway client can't dump the whole table in one shot.
#[derive(Debug, Deserialize)]
pub struct ListUsersQuery {
    /// Opaque cursor returned by the previous page
    /// (callers MUST treat as a black box).
    pub after: Option<String>,
    /// Max users per page. Defaults to 20, hard cap 100.
    pub limit: Option<u32>,
}

/// `GET /api/v1/permission/users?after=&limit=`
///
/// Permission-page user listing. Backed by
/// `dbo.SP_PERMISSION_USER_LIST_V2`. Wire shape:
/// `Vec<PermissionUserListRow>` (M17 shape — `role_codes` /
/// `role_names` / `has_permission` badge / status + timestamps;
/// the front-end drills down via the detail endpoint when it needs
/// the full permission code list).
///
/// RBAC: `Admin` or `SuperAdmin` only.
pub async fn list_users_permission(
    State(state): State<AppState>,
    user: AuthnUser,
    Query(q): Query<ListUsersQuery>,
) -> Result<Response, Response> {
    // 1. RBAC — M15-prep: page-visibility code `PERMISSIONS_VIEW`.
    if !user
        .has_permission(Permission::PagePermissionsView, &state.permission_checker)
        .await
    {
        let locale = current_locale();
        let code_str = Permission::PagePermissionsView.code();
        let localized = tr("err_auth.permission_denied", &locale, &[code_str]);
        return Err(ApiError::from(AppError::Localized {
            status: StatusCode::FORBIDDEN,
            code: ErrorCode::PERMISSION_DENIED,
            message: localized,
        })
        .into_response());
    }

    // 2. Cap the limit so a runaway client can't dump the whole
    //    table in one shot. 100 covers the permission page's
    //    "show 50 / 100" paginators with headroom.
    let limit = q.limit.unwrap_or(20).clamp(1, 100);

    // 3. Delegate to the permission-page application service. The
    //    SP returns the full set; pagination lives in Rust today.
    //    M19: forward `user.id()` as caller for the SP admin gate.
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

// ============================================================================
// GET /api/v1/permission/users/:guid/permissions
// ============================================================================
//
// Permission-page per-user permission detail. Backed by
// `dbo.SP_PERMISSION_USER_DETAIL_FIND_BY_GUID`. The SP takes the
// GUID directly — no GUID→username translation in Rust (M17).
//
// Wire shape: the **grouped** payload
// [`PermissionUserGroup`] (user identity hoisted to the outer
// object; per-permission entries nested under `permissions`).

/// `GET /api/v1/permission/users/:guid/permissions`
///
/// Per-user effective permission detail for the permission page.
/// Returns the grouped [`PermissionUserGroup`] payload — user
/// identity at the top, `permissions: Vec<PermissionUserGroupEntry>`
/// nested below. Each inner entry carries the three fields the
/// front-end pattern-matches on
/// (`user_permission_code` / `has_override` / `effective_status`).
///
/// RBAC: `Admin` or `SuperAdmin` only.
///
/// Errors:
/// - 401 `unauthorized` — missing / invalid Bearer token (enforced
///   upstream by the auth middleware).
/// - 403 `admin_required` — caller lacks `Admin` / `SuperAdmin`.
/// - 404 `not_found` — GUID doesn't resolve to a user.
/// - 500 `internal` — unexpected backend failure (via
///   `into_localized_response`).
///
/// An empty `permissions: []` (200) is a legitimate response when
/// the user exists but holds no effective permissions — the UI
/// renders an empty-state placeholder.
pub async fn list_user_permissions_permission(
    State(state): State<AppState>,
    user: AuthnUser,
    Path(guid): Path<Uuid>,
) -> Result<Response, Response> {
    // 1. RBAC — M15-prep: same `PERMISSIONS_VIEW` gate as
    //    `list_users_permission`.
    if !user
        .has_permission(Permission::PagePermissionsView, &state.permission_checker)
        .await
    {
        let locale = current_locale();
        let code_str = Permission::PagePermissionsView.code();
        let localized = tr("err_auth.permission_denied", &locale, &[code_str]);
        return Err(ApiError::from(AppError::Localized {
            status: StatusCode::FORBIDDEN,
            code: ErrorCode::PERMISSION_DENIED,
            message: localized,
        })
        .into_response());
    }

    // 2. axum's `Path<Uuid>` extractor already rejected any
    //    non-UUID path segment with 400 before this point. The
    //    handler no longer needs a defensive GUID check.

    // 3. Delegate to the permission-page application service. The
    //    service calls
    //    `dbo.SP_PERMISSION_USER_DETAIL_FIND_BY_GUID` with the
    //    GUID directly — no GUID→username translation needed.
    //    M19: forward `user.id()` as caller for the SP admin gate.
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

    // 4. Standard 200 envelope (no `meta` — a single-user lookup
    //    isn't paginated; if a "paged effective permissions per
    //    user" endpoint lands later, add `meta` then).
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

// ============================================================================
// POST /api/v1/permission/overrides  (M18 — batch override upsert)
// ============================================================================
//
// One request item → one SP call → one per-item result row.
// Per-item validation rejections (`INVALID_EFFECT`,
// `USER_NOT_FOUND`, etc.) land as result rows with
// `success = false` at the matching index — the rest of the
// batch still runs. A real DB failure aborts the loop and
// surfaces as 500 via `into_localized_response`.

/// Max items per batch — defensive cap so a runaway client
/// can't submit a 100k-item body in one call. The SP is one
/// call per item, so 500 ≈ 500 round-trips + transactions.
/// Matches the M15 admin matrix limit for consistency.
pub const MAX_BULK_PERMISSION_OVERRIDE_UPDATES: usize = 500;

/// Request body for `POST /api/v1/permission/overrides`.
///
/// Shape: `{"items": [...]}`. The audit actor (`update_by` in
/// the SP) is **always** the authenticated admin's GUID — the
/// handler never reads it from the body, so the field doesn't
/// exist. `assigned_by` (per-item) is the only audit field the
/// front-end can set, and it stays optional.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct UpdatePermissionOverridesRequest {
    /// 1..=500 per-item updates. Empty lists fail validation
    /// here (callers should not POST an empty batch).
    #[serde(default)]
    pub items: Vec<PermissionOverrideUpdateItem>,
}

/// Aggregated response. Mirrors the M15 admin update response
/// shape: a flat `results` list + a small summary (totals
/// per code) so the front-end can render "N updated, M
/// created, K failed" without re-scanning the array.
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct UpdatePermissionOverridesResponse {
    /// Total items in the request.
    pub total: usize,
    /// Items where the SP flipped an existing row.
    pub updated: usize,
    /// Items where the SP created a new row.
    pub created: usize,
    /// Items where the SP rejected the input (validation or
    /// not-found). The per-item `code` / `message` carries the
    /// specific reason.
    pub failed: usize,
    /// Per-item results, in input order. `results[i]` always
    /// corresponds to `request.items[i]`.
    pub results: Vec<kokkak_domain::PermissionOverrideUpdateResult>,
}

/// `POST /api/v1/permission/overrides`
///
/// Batch upsert of permission overrides into
/// `user_permission_override`. Body shape:
///
/// ```json
/// {
///   "items": [
///     {
///       "user_guid": "...",
///       "permission_guid": "...",
///       "effect": "allow",
///       "reason": "...",            // optional
///       "assigned_by": "...",        // optional (defaults to actor)
///       "status": 1                   // optional, default 1
///     }
///   ]
/// }
/// ```
///
/// Always returns 200 with a per-item `results` array — the
/// top-level `success` is always `true` even when individual
/// items fail (their per-item `success` field carries the
/// outcome). 4xx / 5xx only for malformed body, RBAC failure,
/// or DB outage.
///
/// RBAC: `Admin` or `SuperAdmin` only.
///
/// Errors:
/// - 400 `bad_request` — body is not valid JSON.
/// - 401 `unauthorized` — missing / invalid Bearer token.
/// - 403 `admin_required` — caller lacks `Admin` / `SuperAdmin`.
/// - 422 `validation` — empty `items` list, out-of-range item
///   count, or any item fails pre-flight validation (e.g.
///   `effect` is not `allow` / `deny` — the SP would also
///   reject it, but failing fast in the handler saves a
///   round-trip).
/// - 500 `internal` — real DB failure (via
///   `into_localized_response`).
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
    // 1. RBAC — M15-prep: writing override rows needs the action
    //    code `PERMISSIONS_UPDATE`, not just the page-visibility.
    if !user
        .has_permission(Permission::PermissionsUpdate, &state.permission_checker)
        .await
    {
        let locale = current_locale();
        let code_str = Permission::PermissionsUpdate.code();
        let localized = tr("err_auth.permission_denied", &locale, &[code_str]);
        return Err(ApiError::from(AppError::Localized {
            status: StatusCode::FORBIDDEN,
            code: ErrorCode::PERMISSION_DENIED,
            message: localized,
        })
        .into_response());
    }

    // 2. List-level validation — 1..=MAX items. Fail fast
    //    before any DB call.
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

    // 3. Per-item pre-flight validation. The SP would also
    //    reject these, but failing fast in Rust saves a
    //    round-trip AND lets us point at the specific bad
    //    index in the error envelope (the SP only echoes the
    //    bad value, not its position in the request).
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

    // 4. Audit actor is **always** the JWT's `user.id()`. Never
    //    read it from the body — a SuperAdmin could otherwise
    //    frame another admin by stamping their GUID onto a
    //    change. The `assigned_by` per-item field is the only
    //    audit field the front-end can set, and it stays
    //    optional (the SP defaults it to `update_by` when
    //    omitted).
    let actor = user.id();

    // 5. Delegate to the service. The service loops, calling
    //    the SP once per item. `RepoError::Backend` (real DB
    //    failure, e.g. connection dropped mid-batch) maps to
    //    500 via the standard localized envelope — same path
    //    every other permission handler uses.
    let results = match state
        .permission
        .update_permission_overrides(&req.items, actor)
        .await
    {
        Ok(r) => r,
        Err(e) => return Err(ApiError::from(e).into_localized_response(&state).await),
    };

    // 6. Aggregate the per-item results into the response
    //    shape the front-end consumes. The categorization
    //    happens once on the Rust side so the UI doesn't have
    //    to re-scan the array to render summary stats.
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

/// Helper: build the validation error envelope used by the
/// batch handler. Mirrors the M15 admin pattern (`admin.rs`).
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

// ============================================================================
// OpenAPI registration
// ============================================================================
//
// The permission-page read handlers don't add their own
// `#[utoipa::path]` annotations today (the M18 write handler
// above is the first). If a future OpenAPI surface for the
// permission read routes is needed, add `#[utoipa::path]` to
// each handler the same way `admin.rs` does and register the
// new entries in `crate::openapi::ApiDoc`.

// `PermissionUserListRow` is referenced here for downstream re-export
// clarity in route docs — keep the import warm.
#[allow(dead_code)]
fn _type_anchor() -> Option<PermissionUserListRow> {
    None
}
