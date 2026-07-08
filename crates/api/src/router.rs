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

pub fn build(state: AppState) -> Router {
    let health_routes = Router::new()
        .route("/healthz", get(handlers::health::healthz))
        .route("/readyz", get(handlers::health::readyz))
        .with_state(state.health.clone());

    let idempotent_routes = Router::new()
        .route("/api/v1/auth/register", post(handlers::auth::register))
        .route("/api/v1/orders", post(handlers::order::create_order))
        .route("/api/v1/payments", post(handlers::payment::create_payment))
        .layer(from_fn(require_idempotency_key))
        .layer(from_fn(auth_flag(Arc::new(state.clone()))))
        .layer(from_fn(orders_flag(Arc::new(state.clone()))))
        .layer(from_fn(payments_flag(Arc::new(state.clone()))));

    let auth_routes = Router::new()
        .route("/api/v1/auth/login", post(handlers::auth::login))
        .route("/api/v1/auth/refresh", post(handlers::auth::refresh))
        .route("/api/v1/auth/logout", post(handlers::auth::logout))
        .layer(from_fn(auth_flag(Arc::new(state.clone()))));

    let protected_routes = Router::new()
        .route("/api/v1/users/me", get(handlers::user::get_me))
        .route(
            "/api/v1/catalog/services",
            get(handlers::catalog::list_services),
        )
        .route(
            "/api/v1/category-job-mains",
            get(handlers::category_job_main::list_category_job_mains),
        )
        .route(
            "/api/v1/category-job-mains/autocomplete",
            get(handlers::category_job_main::autocomplete_category_job_mains),
        )
        .route(
            "/api/v1/category-job-mains/:guid",
            get(handlers::category_job_main::get_category_job_main),
        )
        .route(
            "/api/v1/category-job-services",
            get(handlers::category_job_service_main::list_category_job_service_mains),
        )
        .route(
            "/api/v1/category-job-services/autocomplete",
            get(handlers::category_job_service_main::autocomplete_category_job_service_mains),
        )
        .route(
            "/api/v1/category-job-services/:service_guid",
            get(handlers::category_job_service_main::get_category_job_service_main),
        )
        .route(
            "/api/v1/category-job-service-subs",
            get(handlers::category_job_service_sub::list_category_job_service_subs),
        )
        .route(
            "/api/v1/category-job-service-subs/:sub_guid",
            get(handlers::category_job_service_sub::get_category_job_service_sub),
        )
        .route(
            "/api/v1/category-job-service-subs/:sub_guid/images",
            get(handlers::category_job_service_sub::list_category_job_service_sub_images),
        )
        .route(
            "/api/v1/master/countries",
            get(handlers::master::list_countries),
        )
        .route(
            "/api/v1/master/user-department-teams/autocomplete",
            get(handlers::master::autocomplete_user_department_team),
        )
        .route(
            "/api/v1/master/user-departments/autocomplete",
            get(handlers::master::autocomplete_user_department),
        )
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

    let admin_category_job_main_routes = Router::new()
        .route(
            "/api/v1/admin/category-job-mains",
            post(handlers::category_job_main::create_category_job_main_admin),
        )
        .route(
            "/api/v1/admin/category-job-mains/:guid",
            put(handlers::category_job_main::update_category_job_main_admin)
                .delete(handlers::category_job_main::delete_category_job_main_admin),
        )
        .layer(from_fn(admin_flag(Arc::new(state.clone()))));

    let admin_category_job_service_main_routes = Router::new()
        .route(
            "/api/v1/admin/category-job-services",
            post(handlers::category_job_service_main::create_category_job_service_main_admin),
        )
        .route(
            "/api/v1/admin/category-job-services/:service_guid",
            put(handlers::category_job_service_main::update_category_job_service_main_admin)
                .delete(
                    handlers::category_job_service_main::delete_category_job_service_main_admin,
                ),
        )
        .layer(from_fn(admin_flag(Arc::new(state.clone()))));

    let admin_category_job_service_sub_routes = Router::new()
        .route(
            "/api/v1/admin/category-job-service-subs",
            post(handlers::category_job_service_sub::create_category_job_service_sub_admin),
        )
        .route(
            "/api/v1/admin/category-job-service-subs/:sub_guid",
            put(handlers::category_job_service_sub::update_category_job_service_sub_admin)
                .delete(handlers::category_job_service_sub::delete_category_job_service_sub_admin),
        )
        .layer(from_fn(admin_flag(Arc::new(state.clone()))));

    let admin_category_job_service_sub_fee_routes = Router::new()
        .route(
            "/api/v1/admin/category-job-service-sub-fees",
            get(handlers::category_job_service_sub_fee::list_category_job_service_sub_fees_admin)
                .post(
                handlers::category_job_service_sub_fee::create_category_job_service_sub_fee_admin,
            ),
        )
        .route(
            "/api/v1/admin/category-job-service-sub-fees/:guid",
            put(handlers::category_job_service_sub_fee::update_category_job_service_sub_fee_admin),
        )
        .layer(from_fn(admin_flag(Arc::new(state.clone()))));

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

    let admin_permissions_routes = Router::new()
        .route(
            "/api/v1/admin/permissions",
            get(handlers::admin::list_permissions).post(handlers::admin::update_permissions_admin),
        )
        .layer(from_fn(admin_flag(Arc::new(state.clone()))));

    let permission_page_routes = Router::new()
        .route(
            "/api/v1/permission/users",
            get(handlers::permission::list_users_permission),
        )
        .route(
            "/api/v1/permission/users/:guid/permissions",
            get(handlers::permission::list_user_permissions_permission),
        )
        .route(
            "/api/v1/permission/overrides",
            post(handlers::permission::update_permission_overrides),
        )
        .layer(from_fn(admin_flag(Arc::new(state.clone()))));

    Router::new()
        .merge(health_routes)
        .merge(auth_routes)
        .merge(protected_routes)
        .merge(chat_routes)
        .merge(payment_routes)
        .merge(admin_payout_routes)
        .merge(admin_category_job_main_routes)
        .merge(admin_category_job_service_main_routes)
        .merge(admin_category_job_service_sub_routes)
        .merge(admin_category_job_service_sub_fee_routes)
        .merge(admin_users_routes)
        .merge(admin_permissions_routes)
        .merge(permission_page_routes)
        .merge(idempotent_routes)
        .merge(openapi_routes::<AppState>(state.settings.environment))
        .with_state(state)
        .layer(from_fn(locale_middleware))
}

fn openapi_routes<S>(env: Environment) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    if matches!(env, Environment::Production) {
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
