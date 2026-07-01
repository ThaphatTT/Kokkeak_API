//! OpenAPI spec for the Kokkeak API (T-16).
//!
//! We use `utoipa` to derive the spec at compile time from the
//! handler signatures + `#[derive(ToSchema)]` types. The
//! resulting `ApiDoc` is served at `/api/openapi.json` (raw JSON)
//! and `/api/docs` (Swagger UI).
//!
//! ## Scope
//!
//! Paths are listed explicitly per route group. Mobile team
//! needs every endpoint that the BFF / mobile app might call:
//! - auth (register, login, refresh, logout)
//! - users (get_me)
//! - catalog (list_services)
//! - orders (list_my_orders, list_assigned_orders, create_order)
//! - payments (list_my_payments, create_payment, confirm_payment,
//!   get_payment)
//! - admin (payouts list / mark paid, user create, user list,
//!   per-user permissions, role × permission matrix)
//! - chat (rooms list / open, messages list / send, mark read)
//! - health (healthz, readyz)
//!
//! ## Idempotency-Key header
//!
//! The 3 protected POSTs (`/orders`, `/payments`, `/auth/register`)
//! carry a required `Idempotency-Key: <unique>` header. Mobile
//! retries MUST send the same key. See `AGENTS.md` § 12.4.

use utoipa::openapi::security::{HttpAuthScheme, HttpBuilder, SecurityScheme};
use utoipa::Modify;
use utoipa::OpenApi;

use crate::handlers;

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Kokkeak API",
        version = "0.1.0",
        description = "Handyman / technician marketplace backend (Laos). \
            Mobile-first JSON over HTTPS. All responses use the standard \
            envelope: `{ success, data, error, meta }`. Errors include a \
            machine-readable `error.code` for programmatic handling. \
            Protected POSTs require `Idempotency-Key`. \
            See GET /api/error-codes.json for the full catalog.",
        contact(name = "Kokkeak Team"),
    ),
    paths(
        // ---- T-16: health probes (always available) ----
        handlers::health::healthz,
        handlers::health::readyz,
        // ---- Auth ----
        handlers::auth::register,
        handlers::auth::login,
        handlers::auth::refresh,
        handlers::auth::logout,
        // ---- User / catalog ----
        handlers::user::get_me,
        handlers::catalog::list_services,
        // ---- M20: Master data ----
                        handlers::master::list_countries,
                        handlers::master::autocomplete_user_department_team,
                        handlers::master::autocomplete_user_department,
                        handlers::master::autocomplete_master_positions,
        // ---- Orders ----
        handlers::order::list_my_orders,
        handlers::order::list_assigned_orders,
        handlers::order::create_order,
        // ---- Payments ----
        handlers::payment::list_my_payments,
        handlers::payment::get_payment,
        handlers::payment::create_payment,
        handlers::payment::confirm_payment,
        // ---- Admin ----
                        handlers::payment::list_payouts_admin,
                        handlers::payment::mark_payout_paid_admin,
                        handlers::admin::create_user_admin,
                        handlers::admin::admin_insert_user_full,
                        handlers::admin::list_users_admin,
                        handlers::admin::list_user_permissions_admin,
                                handlers::admin::list_permissions,
                                handlers::admin::update_permissions_admin,
                // ---- Permission (M18: batch override upsert) ----
                handlers::permission::update_permission_overrides,
    ),
    components(
        schemas(
            // Request DTOs (auth + admin — the rest are inline in the path annotations).
                        handlers::auth::RegisterRequest,
                        handlers::auth::LoginRequest,
                        handlers::auth::RefreshRequest,
                        handlers::auth::AuthResponse,
                        handlers::auth::LogoutResponse,
                        handlers::catalog::ListQuery,
                        handlers::catalog::ServiceItem,
                        handlers::master::CountriesQuery,
                                                kokkak_domain::MasterDropdownRow,
                                                                                                handlers::master::AutocompleteUserDepartmentTeamQuery,
                                                                                                kokkak_domain::UserDepartmentTeamAutocompleteRow,
                                                                                                handlers::master::AutocompleteUserDepartmentQuery,
                                                handlers::master::PositionsAutocompleteQuery,
                                                kokkak_domain::MasterPositionAutocompleteRow,
                        handlers::admin::CreateUserRequest,
                        // M20-b: rich admin user creation (SP_USER_INSERT_FULL)
                        handlers::admin::AdminInsertUserRequest,
                        handlers::admin::AdminInsertUserResponse,
                        handlers::admin::WeeklyScheduleDto,
                        handlers::admin::DayScheduleDto,
                                    handlers::admin::ListUsersQuery,
                                                handlers::admin::ListUsersResponse,
                                                handlers::admin::PermissionsQuery,
                        handlers::admin::UpdatePermissionsRequest,
                        handlers::admin::UpdatePermissionsResponse,
                        handlers::admin::PermissionUpdateItem,
                        handlers::admin::PermissionUpdateResultItem,
                        kokkak_domain::PermissionUpdateRow,
            // Domain entities (cfg-gated `ToSchema` via the `openapi` feature).
                        kokkak_domain::PublicUser,
                                    kokkak_domain::UserListRow,
                                    kokkak_domain::admin_user::UserListPagingRow,
                                    kokkak_domain::admin_user::AdminUserListPagingPage,
                        kokkak_domain::ServiceCategory,
            kokkak_domain::Order,
            kokkak_domain::OrderStatus,
            kokkak_domain::Payment,
            kokkak_domain::PaymentStatus,
            kokkak_domain::Payout,
            kokkak_domain::PayoutStatus,
            kokkak_domain::Role,
            kokkak_domain::UserRolePermission,
            kokkak_domain::UserRolePermissionRow,
            kokkak_domain::UserRoleWithPermissions,
            // M17: permission-page wire payload.
            kokkak_domain::PermissionUserListRow,
            kokkak_domain::PermissionUserDetailRow,
            kokkak_domain::PermissionUserGroupEntry,
            kokkak_domain::PermissionUserGroup,
            // M18: batch permission-override update wire payload.
            kokkak_domain::PermissionOverrideUpdateItem,
            kokkak_domain::PermissionOverrideUpdateResult,
            handlers::permission::UpdatePermissionOverridesRequest,
            handlers::permission::UpdatePermissionOverridesResponse,
            // Error envelope (used by all 4xx / 5xx responses).
            ApiError,
            ApiErrorBody,
        ),
    ),
    modifiers(&SecurityAddon),
    tags(
        (name = "health", description = "Liveness + readiness probes (no auth)"),
        (name = "auth", description = "Login, register, refresh, logout"),
        (name = "users", description = "Current user profile"),
        (name = "catalog", description = "Service category catalog (master data)"),
        (name = "orders", description = "Order lifecycle — requires Idempotency-Key on POST"),
        (name = "payments", description = "Payment intents — requires Idempotency-Key on POST"),
        (name = "admin", description = "Admin-only endpoints (requires admin JWT)"),
    )
)]
pub struct ApiDoc;

/// T-16: add the bearer auth security scheme via a Modify
/// impl. The utoipa `security_schemes(...)` macro syntax is
/// fiddlier than the `components(schemas(...))` syntax, so we
/// use the documented `Modify` pattern instead — same effect.
struct SecurityAddon;

impl Modify for SecurityAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        let components = openapi.components.get_or_insert_default();
        components.security_schemes.insert(
            "bearer_auth".into(),
            SecurityScheme::Http(
                HttpBuilder::new()
                    .scheme(HttpAuthScheme::Bearer)
                    .bearer_format("JWT")
                    .build(),
            ),
        );
    }
}

/// T-17: catalog of stable error codes. Mobile teams should fetch
/// this on app start (or bake it into their build) so they can
/// generate strongly-typed error handlers in the client SDK.
#[derive(Clone, Debug, serde::Serialize, utoipa::ToSchema)]
pub struct ErrorCodeEntry {
    /// Stable snake_case string from `kokkak_common::error_codes`.
    pub code: &'static str,
    /// HTTP status code that always accompanies this error.
    pub status: u16,
    /// One-line description for the mobile / BFF developer.
    pub description: &'static str,
}

/// T-17: full catalog rendered as JSON for the
/// `GET /api/error-codes.json` endpoint.
pub fn error_codes_catalog() -> Vec<ErrorCodeEntry> {
    use kokkak_common::error_codes::ErrorCode;
    vec![
        // 400
        (
            ErrorCode::BAD_REQUEST,
            400,
            "Request is malformed (invalid JSON, missing required field).",
        ),
        (
            ErrorCode::IDEMPOTENCY_KEY_REQUIRED,
            400,
            "`Idempotency-Key` header is missing or whitespace on a protected endpoint.",
        ),
        // 401
        (
            ErrorCode::UNAUTHORIZED,
            401,
            "Credentials missing, wrong, or otherwise invalid.",
        ),
        (
            ErrorCode::INVALID_TOKEN,
            401,
            "Bearer token signature / format invalid.",
        ),
        (
            ErrorCode::TOKEN_EXPIRED,
            401,
            "Bearer token expired (`exp` claim in the past).",
        ),
        (
            ErrorCode::REFRESH_INVALID,
            401,
            "Refresh token rejected (revoked, malformed, or expired).",
        ),
        // 403
        (
            ErrorCode::FORBIDDEN,
            403,
            "Authenticated but the role is not allowed on this endpoint.",
        ),
        (
            ErrorCode::ADMIN_REQUIRED,
            403,
            "Admin role required (admin-only endpoints).",
        ),
        (
            ErrorCode::NOT_A_PARTICIPANT,
            403,
            "Caller is not a participant of the target chat room.",
        ),
        // 404
        (ErrorCode::NOT_FOUND, 404, "Resource not found."),
        (ErrorCode::ROOM_NOT_FOUND, 404, "Chat room not found."),
        // 409
        (
            ErrorCode::CONFLICT,
            409,
            "State conflict (generic; prefer a more specific code).",
        ),
        (
            ErrorCode::USERNAME_TAKEN,
            409,
            "Username already taken (registration, admin user create).",
        ),
        (
            ErrorCode::PAYMENT_ALREADY_CAPTURED,
            409,
            "Payment already captured (cannot confirm twice).",
        ),
        // 422
        (ErrorCode::VALIDATION, 422, "Semantic validation failure."),
        (
            ErrorCode::ROLE_NOT_ALLOWED,
            422,
            "Role string is not in the public-registration allow-list.",
        ),
        (
            ErrorCode::INVALID_BODY,
            422,
            "Chat message body was empty or too long.",
        ),
        // 429
        (ErrorCode::RATE_LIMITED, 429, "Per-IP rate limit hit."),
        // 5xx
        (
            ErrorCode::INTERNAL,
            500,
            "Unexpected internal error (catch-all).",
        ),
    ]
    .into_iter()
    .map(|(code, status, description)| ErrorCodeEntry {
        code,
        status,
        description,
    })
    .collect()
}

/// Minimal stand-in for the standard error envelope. The real
/// one lives in `kokkak_common::response::ApiResponse<T>` and is
/// generic over the success payload — utoipa can't derive a
/// schema for the full envelope without a concrete `T`, so we
/// document the shape here as a flat object that matches what
/// `ApiResponse::err(...)` actually serializes.
#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct ApiError {
    /// Always `false` for an error response.
    pub success: bool,
    /// Null on error.
    pub data: Option<serde_json::Value>,
    /// Populated on error.
    pub error: ApiErrorBody,
    /// Null on error (would carry pagination on success).
    pub meta: Option<serde_json::Value>,
}

#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct ApiErrorBody {
    /// Machine-readable code, e.g. `"validation"`, `"username_taken"`,
    /// `"idempotency_key_required"`. Mobile clients pattern-match on
    /// this string instead of parsing the human message.
    pub code: String,
    /// Localized human-readable message. Server picks the locale
    /// from `Accept-Language` / `?lang=` (see AGENTS.md § 13).
    pub message: String,
}
