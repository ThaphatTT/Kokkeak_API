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
//! - admin (payouts list / mark paid, user create)
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
            Protected POSTs require `Idempotency-Key`.",
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
            handlers::admin::CreateUserRequest,
            // Domain entities (cfg-gated `ToSchema` via the `openapi` feature).
            kokkak_domain::PublicUser,
            kokkak_domain::ServiceCategory,
            kokkak_domain::Order,
            kokkak_domain::OrderStatus,
            kokkak_domain::Payment,
            kokkak_domain::PaymentStatus,
            kokkak_domain::Payout,
            kokkak_domain::PayoutStatus,
            kokkak_domain::Role,
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
