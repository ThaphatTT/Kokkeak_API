//! HTTP router (composition root for routes).

use std::sync::Arc;

use axum::{
    middleware::from_fn,
    response::IntoResponse,
    routing::{get, post, put},
    Json, Router,
};
use utoipa::OpenApi;
use utoipa_swagger_ui::{Config, SwaggerUi, Url};

use kokkak_common::config::Environment;

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
        // M20: country dropdown endpoint. Shared by mobile /
        // customer web / admin web — authenticated only, no
        // admin gate (the data is public to every role).
        .route(
            "/api/v1/master/countries",
            get(handlers::master::list_countries),
        )
        // M20+: user-department-team autocomplete for the admin
        // user form. Same gate as the country dropdown — the
        // admin web console enforces the admin role client-side.
        .route(
            "/api/v1/master/user-department-teams/autocomplete",
            get(handlers::master::autocomplete_user_department_team),
        )
        .route(
            "/api/v1/master/user-departments/autocomplete",
            get(handlers::master::autocomplete_user_department),
        )
        // M20+: master-position autocomplete for the admin
        // user form (tech/admin/operator role). Richer wire
        // shape than the user-department autocomplete — the
        // admin UI shows `code` + `description` + `level`
        // alongside the `value` / `label` pair.
        .route(
            "/api/v1/master/positions/autocomplete",
            get(handlers::master::autocomplete_master_positions),
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
    //
    // M16: Admin user listing (`GET /api/v1/admin/users`, backed by
    // `dbo.SP_PERMISSION_USER_LIST`) and per-user permission lookup
    // (`GET /api/v1/admin/users/:guid/permissions`, backed by
    // `dbo.SP_PERMISSION_USER_FIND_BY_USERNAME`). The per-user
    // endpoint keeps the GUID in the URL (stable for the admin UI)
    // and translates GUID → username via the existing
    // `UserRepository::find_by_id` path so the permission module
    // is not touched.
    //
    // M20-b: Rich admin user creation
    // (`POST /api/v1/admin/users/full`, backed by
    // `dbo.SP_USER_INSERT_FULL`). Same `admin_flag` gate — the
    // simple `create_user_admin` (above) uses `API_USER_REGISTER`
    // and only takes first/last/username/password/role; the `full`
    // variant takes every column the legacy ASP.NET admin form
    // collected (address / bank / position / salary / schedule /
    // attachments). Both are exposed so the admin UI can pick
    // the lighter flow when only the basic fields are needed.
    //
    // M22: Read-side counterpart to M20-b —
    //    (`GET /api/v1/admin/users/:guid/detail`, backed by
    //    `dbo.SP_USER_DETAIL_FULL_GET`). Returns the same admin
    //    form data the create flow accepts, with every related
    //    sub-block (login / company / roles / position / salary /
    //    schedule / bank account / four attachments) as nested
    //    objects. Same `PageUsersView` RBAC as the user list —
    //    viewing the page list implies viewing any individual
    //    user's detail.
    //
    // M22-b: Write-side counterpart to M22 —
    //    (`PUT /api/v1/admin/users/:guid/full`, backed by
    //    `dbo.SP_USER_UPDATE_FULL`). Same wire shape as the
    //    create endpoint (minus the password), with the target
    //    GUID on the URL path. RBAC: `Permission::UsersUpdate`
    //    (matches the SP's server-side check).
    let admin_users_routes = Router::new()
        .route(
            "/api/v1/admin/users",
            post(handlers::admin::create_user_admin).get(handlers::admin::list_users_admin),
        )
        .route(
            "/api/v1/admin/users/full",
            post(handlers::admin::admin_insert_user_full),
        )
        .route(
            "/api/v1/admin/users/:guid/permissions",
            get(handlers::admin::list_user_permissions_admin),
        )
        .route(
            "/api/v1/admin/users/:guid/detail",
            get(handlers::admin::get_user_detail_full_admin),
        )
        .route(
            "/api/v1/admin/users/:guid/full",
            put(handlers::admin::admin_update_user_full),
        )
        .layer(from_fn(admin_flag(Arc::new(state.clone()))));

    // M15-prep: Admin role × permission matrix lookup. Read-only;
    // the route is gated by `admin_flag` so flipping the Strangler
    // flag hands it back to the legacy ASP.NET service.
    //
    // M15: the POST side handles bulk grant / revoke — same path,
    // different verb. Both share `admin_flag`. No idempotency-key
    // middleware: this is an authenticated admin web console,
    // not a mobile client with flaky-network retries.
    let admin_permissions_routes = Router::new()
        .route(
            "/api/v1/admin/permissions",
            get(handlers::admin::list_permissions).post(handlers::admin::update_permissions_admin),
        )
        .layer(from_fn(admin_flag(Arc::new(state.clone()))));

    // Permission page (strictly isolated from admin flow).
    //
    // Two read-only routes backed by the same SQL Server SPs as the
    // admin user-management screen, but on a separate route prefix
    // (`/api/v1/permission/...`) and served by a separate handler +
    // application service (`PermissionUserService`). The repository
    // trait + adapter are shared — only the service + handler +
    // router layer is duplicated, on purpose. See
    // `crates/application/src/permission.rs` for the rationale.
    //
    // RBAC: same as the admin routes today (Admin / SuperAdmin via
    // `admin_flag`). If a future permission-page-specific role is
    // added, swap the middleware in one place here without touching
    // the handler or the service.
    let permission_page_routes = Router::new()
        .route(
            "/api/v1/permission/users",
            get(handlers::permission::list_users_permission),
        )
        .route(
            "/api/v1/permission/users/:guid/permissions",
            get(handlers::permission::list_user_permissions_permission),
        )
        // M18: batch permission-override upsert. Read-side
        // routes are GETs and need no idempotency-key middleware
        // (admin web console, no flaky-network retries). The
        // write route is also admin-only; the team's policy is
        // to skip idempotency on authenticated admin web
        // consoles (see M15 admin matrix for the same
        // decision). Same `admin_flag` gate as the rest.
        .route(
            "/api/v1/permission/overrides",
            post(handlers::permission::update_permission_overrides),
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
        .merge(admin_permissions_routes)
        .merge(permission_page_routes)
        .merge(idempotent_routes)
        .merge(openapi_routes::<AppState>(state.settings.environment))
        .with_state(state)
        .layer(from_fn(locale_middleware))
}

/// T-16: OpenAPI spec + Swagger UI.
///
/// - `GET /api/openapi.json` returns the generated spec (explicit
///   route, served from this router — owned by us).
/// - `GET /api/docs` serves the interactive Swagger UI.
/// - `GET /api/docs/*` serves the Swagger UI assets.
///
/// **SwaggerUi fetch wiring**: we configure the UI to **fetch** the
/// spec from `/api/openapi.json` via `Config::new([Url::new(...)])`.
/// We deliberately do NOT use `SwaggerUi::url(...)` — that method
/// *also* registers an internal axum route at the URL it is given,
/// which collides with our explicit `/api/openapi.json` route and
/// panics at startup with "Overlapping method route".
///
/// **Production gate**: in `Environment::Production` we serve NONE
/// of these — no recon, no Swagger UI attack surface, no
/// `utoipa_swagger_ui` JS bundles exposed. Operators verify the
/// service is alive via `/healthz` + `/readyz` (always open, no auth).
fn openapi_routes<S>(env: Environment) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    if matches!(env, Environment::Production) {
        // ponytail: env gate — a single ternary on an existing enum,
        // no feature-flag system. Upgrade path if we ever need "off
        // in staging but on for internal QA": add an `openapi` field
        // to `FeatureFlagSettings` and route through `openapi_flag`
        // middleware like the other feature gates.
        return Router::new();
    }

    let catalog = crate::openapi::error_codes_catalog();
    let spec = ApiDoc::openapi();
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
        .merge(
            SwaggerUi::new("/api/docs")
                .config(Config::new([Url::new("Kokkeak API", "/api/openapi.json")])),
        )
}

#[cfg(test)]
mod tests {
    //! Unit tests for the production gate on OpenAPI / Swagger UI routes.
    //!
    //! These tests construct the subrouter directly via
    //! `openapi_routes::<()>(env)` — no AppState, no DB, no JWT. They
    //! use `tower::ServiceExt::oneshot` to assert that the production
    //! gate actually closes the routes (404), and that the dev path
    //! actually exposes them (200 for `/api/openapi.json`, 200 for
    //! the Swagger UI HTML shell at `/api/docs`).

    use super::openapi_routes;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use kokkak_common::config::Environment;
    use tower::ServiceExt;

    fn app_for(env: Environment) -> axum::Router {
        openapi_routes::<()>(env)
    }

    fn get(uri: &str) -> Request<Body> {
        Request::builder().uri(uri).body(Body::empty()).unwrap()
    }

    #[tokio::test]
    async fn production_closes_openapi_json_route() {
        let res = app_for(Environment::Production)
            .oneshot(get("/api/openapi.json"))
            .await
            .unwrap();
        assert_eq!(
            res.status(),
            StatusCode::NOT_FOUND,
            "in production `/api/openapi.json` must return 404 (closed), not expose the spec"
        );
    }

    #[tokio::test]
    async fn production_closes_swagger_ui_route() {
        let res = app_for(Environment::Production)
            .oneshot(get("/api/docs"))
            .await
            .unwrap();
        assert_eq!(
            res.status(),
            StatusCode::NOT_FOUND,
            "in production `/api/docs` (Swagger UI) must return 404 (closed)"
        );
    }

    #[tokio::test]
    async fn production_closes_error_codes_route() {
        // The error-codes catalog is served from the same subrouter
        // — if it's open in prod, mobile teams could still scrape
        // every error code via prod. Keep it closed together.
        let res = app_for(Environment::Production)
            .oneshot(get("/api/error-codes.json"))
            .await
            .unwrap();
        assert_eq!(
            res.status(),
            StatusCode::NOT_FOUND,
            "in production `/api/error-codes.json` must return 404 (closed)"
        );
    }

    #[tokio::test]
    async fn development_serves_openapi_json() {
        let res = app_for(Environment::Development)
            .oneshot(get("/api/openapi.json"))
            .await
            .unwrap();
        assert_eq!(
            res.status(),
            StatusCode::OK,
            "in development `/api/openapi.json` must return 200 (SDK generator + BFF need it)"
        );
    }

    #[tokio::test]
    async fn development_serves_swagger_ui() {
        let res = app_for(Environment::Development)
            .oneshot(get("/api/docs/"))
            .await
            .unwrap();
        // Swagger UI redirect follows /api/docs -> /api/docs/ — oneshot
        // follows 0 redirects so we hit the trailing-slash form.
        assert_eq!(
            res.status(),
            StatusCode::OK,
            "in development `/api/docs/` (Swagger UI) must return 200"
        );
    }

    #[tokio::test]
    async fn development_serves_error_codes() {
        let res = app_for(Environment::Development)
            .oneshot(get("/api/error-codes.json"))
            .await
            .unwrap();
        assert_eq!(
            res.status(),
            StatusCode::OK,
            "in development `/api/error-codes.json` must return 200"
        );
    }
}
