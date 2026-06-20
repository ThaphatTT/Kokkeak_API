//! HTTP router (composition root for routes).

use std::sync::Arc;

use axum::{
    middleware::from_fn,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

use crate::handlers;
use crate::middleware::feature_gate::{
    admin_flag, auth_flag, chat_flag, orders_flag, payments_flag,
};
use crate::middleware::i18n::locale_middleware;
use crate::middleware::idempotency::require_idempotency_key;
use crate::openapi::ApiDoc;
use crate::state::AppState;

/// Build the full application router.
pub fn build(state: AppState) -> Router {
    // Health probes (no auth).
    let health_routes = Router::new()
        .route("/healthz", get(handlers::health::healthz))
        .route("/readyz", get(handlers::health::readyz))
        .with_state(state.health.clone());

    // T-15: routes that REQUIRE an `Idempotency-Key` header on
    // every POST. Without the header, the request is rejected
    // with 400 before the handler runs. With the header, the
    // global permissive idempotency layer (wired in `main.rs`)
    // caches the response for retry safety.
    //
    // Critical = side effects that are non-idempotent at the
    // business level. Adding a route here means "mobile retries
    // could double-charge / double-create, force the client to
    // send a key".
    //
    // T-31: feature gates (auth + orders + payments) are layered
    // individually on these routes so flipping one flag doesn't
    // accidentally disable the others.
    let idempotent_routes = Router::new()
        // Identity: a duplicate registration = duplicate account.
        .route("/api/v1/auth/register", post(handlers::auth::register))
        // Money: a duplicate order = real double-charge.
        .route("/api/v1/orders", post(handlers::order::create_order))
        // Money: a duplicate payment = real double-charge.
        .route("/api/v1/payments", post(handlers::payment::create_payment))
        .layer(from_fn(require_idempotency_key))
        .layer(from_fn(auth_flag(Arc::new(state.clone()))))
        .layer(from_fn(orders_flag(Arc::new(state.clone()))))
        .layer(from_fn(payments_flag(Arc::new(state.clone()))));

    // Public auth routes that do NOT require an idempotency key
    // (login / refresh / logout are token-issuing / revoking
    // operations, not state-mutating).
    let auth_routes = Router::new()
        .route("/api/v1/auth/login", post(handlers::auth::login))
        .route("/api/v1/auth/refresh", post(handlers::auth::refresh))
        .route("/api/v1/auth/logout", post(handlers::auth::logout))
        .layer(from_fn(auth_flag(Arc::new(state.clone()))));

    // Authenticated user/catalog/order routes.
    let protected_routes = Router::new()
        .route("/api/v1/users/me", get(handlers::user::get_me))
        .route(
            "/api/v1/catalog/services",
            get(handlers::catalog::list_services),
        )
        .route("/api/v1/orders/me", get(handlers::order::list_my_orders))
        .route(
            "/api/v1/orders/assigned",
            get(handlers::order::list_assigned_orders),
        )
        .layer(from_fn(orders_flag(Arc::new(state.clone()))));

    // M8: Chat (REST + WebSocket).
    let chat_routes = Router::new()
        .route("/api/v1/chat/rooms", get(handlers::chat::list_rooms))
        .route("/api/v1/chat/rooms", post(handlers::chat::open_room))
        .route(
            "/api/v1/chat/rooms/:id/messages",
            get(handlers::chat::list_messages),
        )
        .route(
            "/api/v1/chat/rooms/:id/messages",
            post(handlers::chat::send_message),
        )
        .route(
            "/api/v1/chat/rooms/:id/read",
            post(handlers::chat::mark_read),
        )
        .route("/api/v1/chat/ws/:id", get(handlers::ws::ws_upgrade))
        .layer(from_fn(chat_flag(Arc::new(state.clone()))));

    // M9: Payments (read-only + confirm are NOT idempotent-critical;
    // the confirm call is naturally idempotent on the server side).
    let payment_routes = Router::new()
        .route(
            "/api/v1/payments/me",
            get(handlers::payment::list_my_payments),
        )
        .route("/api/v1/payments/:id", get(handlers::payment::get_payment))
        .route(
            "/api/v1/payments/:id/confirm",
            post(handlers::payment::confirm_payment),
        )
        .layer(from_fn(payments_flag(Arc::new(state.clone()))));

    // M9: Admin payouts.
    let admin_payout_routes = Router::new()
        .route(
            "/api/v1/admin/payouts",
            get(handlers::payment::list_payouts_admin),
        )
        .route(
            "/api/v1/admin/payouts/:id/pay",
            post(handlers::payment::mark_payout_paid_admin),
        )
        .layer(from_fn(admin_flag(Arc::new(state.clone()))));

    // M14.5: Admin user creation (register role split).
    // Admin / super_admin accounts must be provisioned here, not
    // via the public `/auth/register` endpoint. Admin users
    // don't need the public-mobile idempotency guard — they are
    // created via the admin web console with auth, not retries.
    let admin_users_routes = Router::new()
        .route(
            "/api/v1/admin/users",
            post(handlers::admin::create_user_admin),
        )
        .layer(from_fn(admin_flag(Arc::new(state.clone()))));

    // Merge into a single router, then attach state.
    // Layer order (LIFO: last-attached runs first):
    //   1. require_idempotency_key (innermost, only on the 3
    //      protected routes) — rejects POSTs without a key.
    //   2. locale_middleware — sets task-local locale.
    //   3. T-31: feature_gate per route group. Each Strangler
    //      flag short-circuits with 404 when off, so the
    //      upstream proxy / BFF falls through to ASP.NET.
    Router::new()
        .merge(health_routes)
        .merge(auth_routes)
        .merge(protected_routes)
        .merge(chat_routes)
        .merge(payment_routes)
        .merge(admin_payout_routes)
        .merge(admin_users_routes)
        .merge(idempotent_routes)
        .merge(openapi_routes())
        .with_state(state)
        .layer(from_fn(locale_middleware))
}

/// T-16: OpenAPI spec + Swagger UI.
///
/// - `GET /api/openapi.json` returns the generated spec.
/// - `GET /api/docs` serves the interactive Swagger UI.
/// - `GET /api/docs/*` serves the Swagger UI assets.
fn openapi_routes() -> Router<AppState> {
    let spec = ApiDoc::openapi();
    let catalog = crate::openapi::error_codes_catalog();
    Router::new()
        .route(
            "/api/openapi.json",
            get(move || {
                let spec = spec.clone();
                async move { Json(spec).into_response() }
            }),
        )
        .route(
            "/api/error-codes.json",
            get(move || {
                let catalog = catalog.clone();
                async move { Json(catalog).into_response() }
            }),
        )
        .merge(SwaggerUi::new("/api/docs").url("/api/openapi.json", ApiDoc::openapi()))
}
