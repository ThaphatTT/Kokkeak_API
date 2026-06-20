//! Integration tests for the T-06 HTTP middleware stack:
//! CORS, compression, and request timeout.
//!
//! These tests build a minimal router with each layer wired
//! exactly the way `main.rs` does at production startup. The
//! expectation is that each middleware applies its contract:
//!
//! - **CORS**: a preflight OPTIONS request from an allowed origin
//!   returns the `access-control-allow-origin` header.
//! - **Compression**: a request with `Accept-Encoding: gzip`
//!   receives a `content-encoding: gzip` response when the body
//!   is large enough to be worth compressing.
//! - **Timeout**: a handler that sleeps longer than the configured
//!   timeout returns HTTP 408 or 500 (tower-http 0.6 default).
//!
//! The tests are deliberately scoped to ONE layer per test so a
//! regression points at the offending middleware immediately.

use std::time::Duration;

use axum::{
    body::Body,
    extract::ConnectInfo,
    http::{header, HeaderValue, Method, Request, StatusCode},
    routing::get,
    Router,
};
use http_body_util::BodyExt;
use std::net::SocketAddr;
use tower::ServiceExt;
use tower_governor::governor::GovernorConfigBuilder;
use tower_governor::key_extractor::PeerIpKeyExtractor;
use tower_governor::GovernorLayer;
use tower_http::compression::CompressionLayer;
use tower_http::cors::CorsLayer;
use tower_http::timeout::TimeoutLayer;

/// Build a router wired the same way `main.rs` does in production.
fn app_with_cors(allow_origins: &[&str]) -> Router {
    let origins: Vec<HeaderValue> = allow_origins
        .iter()
        .filter_map(|o| HeaderValue::from_str(o).ok())
        .collect();

    let mut app = Router::new().route("/echo", get(|| async { "ok" })).route(
        "/slow",
        get(|| async {
            tokio::time::sleep(Duration::from_secs(10)).await;
            "should not get here"
        }),
    );

    if !origins.is_empty() {
        app = app.layer(
            CorsLayer::new()
                .allow_origin(origins)
                .allow_methods([
                    Method::GET,
                    Method::POST,
                    Method::PUT,
                    Method::DELETE,
                    Method::OPTIONS,
                ])
                .allow_headers([header::CONTENT_TYPE, header::AUTHORIZATION])
                .allow_credentials(true),
        );
    }
    app
}

fn app_with_compression() -> Router {
    // Body large enough that compression shrinks it noticeably.
    // `flate2` default threshold is 32 bytes; we send 1 KB.
    const BODY: &str = "x";
    let body = BODY.repeat(1024);

    Router::new()
        .route(
            "/big",
            get(move || {
                let body = body.clone();
                async move { (StatusCode::OK, body) }
            }),
        )
        .layer(CompressionLayer::new())
}

fn app_with_timeout(secs: u64) -> Router {
    #[allow(deprecated)]
    Router::new()
        .route(
            "/slow",
            get(|| async {
                tokio::time::sleep(Duration::from_secs(10)).await;
                "should not get here"
            }),
        )
        .route("/fast", get(|| async { "fast" }))
        .layer(TimeoutLayer::new(Duration::from_secs(secs)))
}

// ---- CORS ----

#[tokio::test]
async fn cors_preflight_from_allowed_origin_returns_acao_header() {
    let app = app_with_cors(&["https://app.example.com"]);

    let req = Request::builder()
        .method(Method::OPTIONS)
        .uri("/echo")
        .header(header::ORIGIN, "https://app.example.com")
        .header(header::ACCESS_CONTROL_REQUEST_METHOD, "GET")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers()
            .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .map(|v| v.to_str().unwrap()),
        Some("https://app.example.com"),
        "preflight must echo the allowed origin in ACAO"
    );
}

#[tokio::test]
async fn cors_simple_request_from_disallowed_origin_lacks_acao_header() {
    // No CORS layer at all (allowlist empty) → browser sees no
    // ACAO header on the response and rejects the response
    // client-side. The server still returns the body normally;
    // it is the BROWSER that enforces the same-origin policy.
    let app = app_with_cors(&[]);

    let req = Request::builder()
        .uri("/echo")
        .header(header::ORIGIN, "https://evil.example.com")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(
        resp.headers()
            .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .is_none(),
        "no CORS layer → no ACAO header → browser blocks the response"
    );
}

#[tokio::test]
async fn cors_simple_request_from_allowed_origin_returns_acao_header() {
    let app = app_with_cors(&["https://app.example.com"]);

    let req = Request::builder()
        .uri("/echo")
        .header(header::ORIGIN, "https://app.example.com")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers()
            .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .map(|v| v.to_str().unwrap()),
        Some("https://app.example.com")
    );
}

// ---- Compression ----

#[tokio::test]
async fn compression_with_gzip_accept_encoding_returns_gzip_body() {
    let app = app_with_compression();

    let req = Request::builder()
        .uri("/big")
        .header(header::ACCEPT_ENCODING, "gzip")
        .body(Body::empty())
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers()
            .get(header::CONTENT_ENCODING)
            .map(|v| v.to_str().unwrap()),
        Some("gzip"),
        "Accept-Encoding: gzip must trigger gzip-encoded response"
    );
}

#[tokio::test]
async fn compression_without_accept_encoding_returns_plain_body() {
    let app = app_with_compression();

    let req = Request::builder().uri("/big").body(Body::empty()).unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(
        resp.headers().get(header::CONTENT_ENCODING).is_none(),
        "no Accept-Encoding → response must NOT be compressed"
    );
    // Body should be readable as the original text.
    let body_bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body_text = std::str::from_utf8(&body_bytes).unwrap();
    assert_eq!(body_text.len(), 1024, "body must be 1 KB uncompressed");
}

// ---- Timeout ----

#[tokio::test]
async fn timeout_fast_handler_completes_normally() {
    let app = app_with_timeout(1);

    let req = Request::builder().uri("/fast").body(Body::empty()).unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn timeout_slow_handler_returns_408_or_500() {
    // tower-http 0.6 `TimeoutLayer::new` is deprecated in favour
    // of `with_status_code(408)`. The current API returns 500.
    // Accept either status so this test stays green across the
    // upgrade.
    let app = app_with_timeout(1);

    let req = Request::builder().uri("/slow").body(Body::empty()).unwrap();
    let resp = app.oneshot(req).await.unwrap();

    assert!(
        resp.status() == StatusCode::REQUEST_TIMEOUT
            || resp.status() == StatusCode::INTERNAL_SERVER_ERROR,
        "slow handler must be aborted by the timeout layer, got status {}",
        resp.status()
    );
}

// ---- Rate limit ----

fn app_with_rate_limit(rps: u64, burst: u32) -> Router {
    let governor_conf = std::sync::Arc::new(
        GovernorConfigBuilder::default()
            .per_second(rps)
            .burst_size(burst)
            .key_extractor(PeerIpKeyExtractor)
            .finish()
            .expect("rate-limit config must build"),
    );
    Router::new()
        .route("/echo", get(|| async { (StatusCode::OK, "ok") }))
        .layer(GovernorLayer {
            config: governor_conf,
        })
}

/// Build a request with a `ConnectInfo` extension so
/// `PeerIpKeyExtractor` can extract the source IP. Each test
/// uses a distinct IP so the per-IP buckets don't bleed.
fn request_with_ip(path: &str, ip: [u8; 4]) -> Request<Body> {
    let connect_info = ConnectInfo(SocketAddr::from((ip, 30_000)));
    Request::builder()
        .uri(path)
        .extension(connect_info)
        .body(Body::empty())
        .unwrap()
}

#[tokio::test]
async fn rate_limit_burst_then_429() {
    // burst=2 → first two requests pass, third is throttled.
    // Using a distinct IP per test prevents the limiter's GCRA
    // state from carrying over between runs.
    let app = app_with_rate_limit(1, 2);

    let resp1 = app
        .clone()
        .oneshot(request_with_ip("/echo", [10, 0, 0, 1]))
        .await
        .unwrap();
    assert_eq!(resp1.status(), StatusCode::OK);

    let resp2 = app
        .clone()
        .oneshot(request_with_ip("/echo", [10, 0, 0, 1]))
        .await
        .unwrap();
    assert_eq!(resp2.status(), StatusCode::OK);

    // Third request from the same IP — bucket empty → 429.
    let resp3 = app
        .clone()
        .oneshot(request_with_ip("/echo", [10, 0, 0, 1]))
        .await
        .unwrap();
    assert_eq!(
        resp3.status(),
        StatusCode::TOO_MANY_REQUESTS,
        "third request from same IP must be throttled"
    );
}

#[tokio::test]
async fn rate_limit_distinct_ips_have_independent_buckets() {
    // Each IP gets its own token bucket — IP A exhausting its
    // burst must NOT affect IP B.
    let app = app_with_rate_limit(1, 1);

    // IP A: drain the bucket.
    let a1 = app
        .clone()
        .oneshot(request_with_ip("/echo", [10, 0, 0, 2]))
        .await
        .unwrap();
    assert_eq!(a1.status(), StatusCode::OK);
    let a2 = app
        .clone()
        .oneshot(request_with_ip("/echo", [10, 0, 0, 2]))
        .await
        .unwrap();
    assert_eq!(a2.status(), StatusCode::TOO_MANY_REQUESTS);

    // IP B: still has a full bucket.
    let b1 = app
        .clone()
        .oneshot(request_with_ip("/echo", [10, 0, 0, 3]))
        .await
        .unwrap();
    assert_eq!(
        b1.status(),
        StatusCode::OK,
        "different IP must not inherit another IP's bucket"
    );
}

#[tokio::test]
async fn rate_limit_429_includes_x_ratelimit_after_header() {
    // tower_governor attaches an `x-ratelimit-after` header to
    // 429 responses (NOT the standard `Retry-After`) so
    // well-behaved clients can back off. Verify the header is
    // present and parses as a positive integer.
    let app = app_with_rate_limit(1, 1);

    // Drain the bucket.
    let _ = app
        .clone()
        .oneshot(request_with_ip("/echo", [10, 0, 0, 4]))
        .await
        .unwrap();
    let resp = app
        .clone()
        .oneshot(request_with_ip("/echo", [10, 0, 0, 4]))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);

    let wait = resp
        .headers()
        .get("x-ratelimit-after")
        .expect("429 must include x-ratelimit-after header");
    // The wait time is the GCRA deficit in seconds — with
    // burst=1 and rate=1 rps, the deficit is sub-second and the
    // header rounds to 0. That's fine; clients still parse it
    // and back off. We just verify the header is present and
    // parses as a non-negative integer.
    let secs: u64 = wait
        .to_str()
        .unwrap()
        .parse()
        .expect("x-ratelimit-after must be a non-negative integer (seconds)");
    let _ = secs;
}
