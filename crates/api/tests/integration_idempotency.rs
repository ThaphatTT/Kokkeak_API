

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use axum::{
    body::Body,
    http::{Request, StatusCode},
    response::IntoResponse,
    routing::post,
    Router,
};
use http_body_util::BodyExt;
use kokkak_api::middleware::idempotency::{
    self, IDEMPOTENCY_KEY_HEADER, IDEMPOTENCY_REPLAYED_HEADER,
};
use kokkak_infra::idempotency::InMemoryIdempotencyStore;
use tower::ServiceExt;

fn counter() -> Arc<AtomicU32> {
    Arc::new(AtomicU32::new(0))
}

fn app_with_idempotency(counter: Arc<AtomicU32>) -> Router {
    let store: Arc<dyn kokkak_domain::IdempotencyStore> =
        Arc::new(InMemoryIdempotencyStore::new(100));
    let ttl = Duration::from_secs(60);

    Router::new()
        .route(
            "/echo",
            post({
                let counter = counter.clone();
                move || async move {
                    let n = counter.fetch_add(1, Ordering::SeqCst) + 1;
                    (StatusCode::CREATED, format!("{n}")).into_response()
                }
            }),
        )
        .layer(axum::middleware::from_fn(move |req, next| {
            let store = store.clone();
            let ttl = ttl;
            async move { idempotency::handle(req, next, store, ttl).await }
        }))
}

fn app_with_typed_response(counter: Arc<AtomicU32>) -> Router {
    let store: Arc<dyn kokkak_domain::IdempotencyStore> =
        Arc::new(InMemoryIdempotencyStore::new(100));
    let ttl = Duration::from_secs(60);

    Router::new()
        .route(
            "/typed",
            post({
                let counter = counter.clone();
                move || async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                    (
                        StatusCode::OK,
                        [(
                            axum::http::header::CONTENT_TYPE,
                            "application/vnd.kokkak+json",
                        )],
                        r#"{"hello":"world"}"#,
                    )
                        .into_response()
                }
            }),
        )
        .layer(axum::middleware::from_fn(move |req, next| {
            let store = store.clone();
            let ttl = ttl;
            async move { idempotency::handle(req, next, store, ttl).await }
        }))
}

fn request_with_key(uri: &str, key: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(uri)
        .header(IDEMPOTENCY_KEY_HEADER, key)
        .body(Body::empty())
        .unwrap()
}

fn request_without_key(uri: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(uri)
        .body(Body::empty())
        .unwrap()
}

async fn body_string(resp: axum::response::Response) -> String {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8(bytes.to_vec()).unwrap()
}

#[tokio::test]
async fn first_request_runs_handler_and_caches_response() {
    let counter = counter();
    let app = app_with_idempotency(counter.clone());

    let resp = app
        .clone()
        .oneshot(request_with_key("/echo", "key-1"))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    assert_eq!(body_string(resp).await, "1");
    assert_eq!(counter.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn second_request_with_same_key_replays_cached_response() {
    let counter = counter();
    let app = app_with_idempotency(counter.clone());

    let r1 = app
        .clone()
        .oneshot(request_with_key("/echo", "key-2"))
        .await
        .unwrap();
    assert_eq!(body_string(r1).await, "1");
    assert_eq!(counter.load(Ordering::SeqCst), 1);

    let r2 = app
        .clone()
        .oneshot(request_with_key("/echo", "key-2"))
        .await
        .unwrap();

    assert_eq!(r2.status(), StatusCode::CREATED);
    assert_eq!(
        r2.headers()
            .get(&IDEMPOTENCY_REPLAYED_HEADER)
            .map(|v| v.to_str().unwrap()),
        Some("true"),
        "replay response must include Idempotency-Replayed: true"
    );
    assert_eq!(body_string(r2).await, "1");
    assert_eq!(
        counter.load(Ordering::SeqCst),
        1,
        "handler must not run on a cache hit"
    );
}

#[tokio::test]
async fn different_keys_invoke_handler_independently() {
    let counter = counter();
    let app = app_with_idempotency(counter.clone());

    let r1 = app
        .clone()
        .oneshot(request_with_key("/echo", "key-A"))
        .await
        .unwrap();
    assert_eq!(body_string(r1).await, "1");

    let r2 = app
        .clone()
        .oneshot(request_with_key("/echo", "key-B"))
        .await
        .unwrap();

    assert_eq!(body_string(r2).await, "2");
    assert_eq!(counter.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn request_without_key_passes_through() {

    let counter = counter();
    let app = app_with_idempotency(counter.clone());

    let r1 = app
        .clone()
        .oneshot(request_without_key("/echo"))
        .await
        .unwrap();
    assert_eq!(body_string(r1).await, "1");

    let r2 = app
        .clone()
        .oneshot(request_without_key("/echo"))
        .await
        .unwrap();
    assert_eq!(
        body_string(r2).await,
        "2",
        "no-key requests must each invoke the handler"
    );
    assert_eq!(counter.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn empty_key_header_passes_through() {
    let counter = counter();
    let app = app_with_idempotency(counter.clone());

    let mut req = request_without_key("/echo");
    req.headers_mut()
        .insert(IDEMPOTENCY_KEY_HEADER, "   ".parse().unwrap());

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(body_string(resp).await, "1");

    let resp = app
        .clone()
        .oneshot(request_without_key("/echo"))
        .await
        .unwrap();
    assert_eq!(body_string(resp).await, "2");
    assert_eq!(counter.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn cached_response_includes_original_content_type() {
    let counter = counter();
    let app = app_with_typed_response(counter.clone());

    let r1 = app
        .clone()
        .oneshot(request_with_key("/typed", "ct-key"))
        .await
        .unwrap();
    assert_eq!(
        r1.headers()
            .get(axum::http::header::CONTENT_TYPE)
            .map(|v| v.to_str().unwrap()),
        Some("application/vnd.kokkak+json")
    );

    let r2 = app
        .clone()
        .oneshot(request_with_key("/typed", "ct-key"))
        .await
        .unwrap();
    assert_eq!(
        r2.headers()
            .get(axum::http::header::CONTENT_TYPE)
            .map(|v| v.to_str().unwrap()),
        Some("application/vnd.kokkak+json"),
        "replay must preserve the original Content-Type"
    );
}
