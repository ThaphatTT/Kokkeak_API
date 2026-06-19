//! HTTP router (composition root for routes).

use axum::{
    middleware::from_fn,
    routing::{get, post},
    Router,
};

use crate::handlers;
use crate::middleware::i18n::locale_middleware;
use crate::state::AppState;

/// Build the full application router.
pub fn build(state: AppState) -> Router {
    // Health probes (no auth).
    let health_routes = Router::new()
        .route("/healthz", get(handlers::health::healthz))
        .route("/readyz", get(handlers::health::readyz))
        .with_state(state.health.clone());

    // Public auth routes.
    let auth_routes = Router::new()
        .route("/api/v1/auth/register", post(handlers::auth::register))
        .route("/api/v1/auth/login", post(handlers::auth::login))
        .route("/api/v1/auth/refresh", post(handlers::auth::refresh))
        .route("/api/v1/auth/logout", post(handlers::auth::logout));

    // Authenticated user/catalog/order routes.
    let protected_routes = Router::new()
        .route("/api/v1/users/me", get(handlers::user::get_me))
        .route(
            "/api/v1/catalog/services",
            get(handlers::catalog::list_services),
        )
        .route("/api/v1/orders/me", get(handlers::order::list_my_orders))
        .route("/api/v1/orders", post(handlers::order::create_order))
        .route(
            "/api/v1/orders/assigned",
            get(handlers::order::list_assigned_orders),
        );

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
        .route("/api/v1/chat/ws/:id", get(handlers::ws::ws_upgrade));

    // M9: Payments.
    let payment_routes = Router::new()
        .route("/api/v1/payments", post(handlers::payment::create_payment))
        .route(
            "/api/v1/payments/me",
            get(handlers::payment::list_my_payments),
        )
        .route("/api/v1/payments/:id", get(handlers::payment::get_payment))
        .route(
            "/api/v1/payments/:id/confirm",
            post(handlers::payment::confirm_payment),
        );

    // M9: Admin payouts.
    let admin_payout_routes = Router::new()
        .route(
            "/api/v1/admin/payouts",
            get(handlers::payment::list_payouts_admin),
        )
        .route(
            "/api/v1/admin/payouts/:id/pay",
            post(handlers::payment::mark_payout_paid_admin),
        );

    // M14.5: Admin user creation (register role split).
    // Admin / super_admin accounts must be provisioned here, not
    // via the public `/auth/register` endpoint.
    let admin_users_routes = Router::new().route(
        "/api/v1/admin/users",
        post(handlers::admin::create_user_admin),
    );

    // Merge into a single router, then attach state.
    // Layer order (LIFO: last-attached runs first):
    //   1. locale_middleware (innermost) — sets task-local locale
    //      from `Accept-Language` / `?lang=` before the handler runs.
    Router::new()
        .merge(health_routes)
        .merge(auth_routes)
        .merge(protected_routes)
        .merge(chat_routes)
        .merge(payment_routes)
        .merge(admin_payout_routes)
        .merge(admin_users_routes)
        .with_state(state)
        .layer(from_fn(locale_middleware))
}
