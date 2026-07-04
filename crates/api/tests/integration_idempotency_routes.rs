

use axum::{body::Body, http::Request, Router};
use kokkak_api::middleware::idempotency::IDEMPOTENCY_KEY_HEADER;
use tower::ServiceExt;

fn build_protected_router() -> Router {
    use axum::{middleware::from_fn, response::IntoResponse, routing::post};

    use kokkak_api::middleware::idempotency::require_idempotency_key;

    Router::new()
        .route(
            "/api/v1/auth/register",
            post(|| async { (axum::http::StatusCode::CREATED, "registered").into_response() }),
        )
        .route(
            "/api/v1/orders",
            post(|| async { (axum::http::StatusCode::CREATED, "order").into_response() }),
        )
        .route(
            "/api/v1/payments",
            post(|| async { (axum::http::StatusCode::CREATED, "payment").into_response() }),
        )
        .layer(from_fn(require_idempotency_key))
}

fn post(uri: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(uri)
        .body(Body::empty())
        .unwrap()
}

fn post_with_key(uri: &str, key: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(uri)
        .header(IDEMPOTENCY_KEY_HEADER, key)
        .body(Body::empty())
        .unwrap()
}

#[tokio::test]
async fn register_without_key_returns_400() {
    let app = build_protected_router();
    let resp = app
        .clone()
        .oneshot(post("/api/v1/auth/register"))
        .await
        .unwrap();
    assert_eq!(resp.status(), axum::http::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn register_with_key_returns_201() {
    let app = build_protected_router();
    let resp = app
        .clone()
        .oneshot(post_with_key("/api/v1/auth/register", "reg-1"))
        .await
        .unwrap();
    assert_eq!(resp.status(), axum::http::StatusCode::CREATED);
}

#[tokio::test]
async fn orders_without_key_returns_400() {
    let app = build_protected_router();
    let resp = app.clone().oneshot(post("/api/v1/orders")).await.unwrap();
    assert_eq!(resp.status(), axum::http::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn orders_with_key_returns_201() {
    let app = build_protected_router();
    let resp = app
        .clone()
        .oneshot(post_with_key("/api/v1/orders", "ord-1"))
        .await
        .unwrap();
    assert_eq!(resp.status(), axum::http::StatusCode::CREATED);
}

#[tokio::test]
async fn payments_without_key_returns_400() {
    let app = build_protected_router();
    let resp = app.clone().oneshot(post("/api/v1/payments")).await.unwrap();
    assert_eq!(resp.status(), axum::http::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn payments_with_key_returns_201() {
    let app = build_protected_router();
    let resp = app
        .clone()
        .oneshot(post_with_key("/api/v1/payments", "pay-1"))
        .await
        .unwrap();
    assert_eq!(resp.status(), axum::http::StatusCode::CREATED);
}

#[tokio::test]
async fn empty_string_key_returns_400() {

    let app = build_protected_router();
    let resp = app
        .clone()
        .oneshot(post_with_key("/api/v1/orders", "   "))
        .await
        .unwrap();
    assert_eq!(resp.status(), axum::http::StatusCode::BAD_REQUEST);
}
