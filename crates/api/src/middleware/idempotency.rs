//! HTTP idempotency middleware (T-14).
//!
//! Reads the `Idempotency-Key` request header on POST endpoints
//! and replays the cached response for retries with the same key.
//! See [`kokkak_domain::IdempotencyStore`] for the storage
//! abstraction and the [module docs](crate) for the overall
//! contract (TTL, 2xx-only cache, replay header).
//!
//! ## Headers
//!
//! - Request: `Idempotency-Key: <unique-string>`. Header is
//!   **optional** — requests without the header pass through
//!   unchanged. T-15 wires the protected POSTs to send the
//!   header, but the middleware itself is permissive.
//! - Response: when a cache hit happens, the response carries
//!   `Idempotency-Replayed: true` so clients can distinguish a
//!   fresh response from a replay.
//!
//! ## Scope
//!
//! This middleware operates on the entire router. To restrict it
//! to specific routes, wrap only those routes in a sub-router
//! and apply the middleware there (T-15 demonstrates this).

use std::sync::Arc;
use std::time::Duration;

use axum::{
    body::{to_bytes, Body},
    extract::Request,
    http::{header, HeaderName, HeaderValue, Method, StatusCode},
    middleware::Next,
    response::IntoResponse,
    response::Response,
};
use kokkak_domain::{CachedResponse, IdempotencyStore};

/// Header that carries the per-request idempotency token. Lowercase
/// because axum normalises header names on the wire.
pub const IDEMPOTENCY_KEY_HEADER: HeaderName = HeaderName::from_static("idempotency-key");

/// Response header set to `"true"` when the response was served
/// from the idempotency cache (i.e. a retry of a prior request).
pub const IDEMPOTENCY_REPLAYED_HEADER: HeaderName = HeaderName::from_static("idempotency-replayed");

/// Maximum body size we will buffer to cache a response. 1 MiB is
/// generous for our JSON responses (orders / payments are KB) and
/// still safe to hold in memory. Ponytail: minimum code that
/// works for the realistic payload; bump or make configurable
/// when a real endpoint needs more.
const MAX_CACHEABLE_BODY: usize = 1024 * 1024;

/// Per-request handler used by `axum::middleware::from_fn`.
///
/// Call sites wrap this with `axum::middleware::from_fn` and
/// capture the store + TTL in the wrapping closure, e.g.:
///
/// ```ignore
/// let store = ...;
/// let ttl = Duration::from_secs(86400);
/// .layer(axum::middleware::from_fn(move |req, next| {
///     let store = store.clone();
///     let ttl = ttl;
///     async move { idempotency::handle(req, next, store, ttl).await }
/// }))
/// ```
///
/// We don't expose a `make_layer` factory because the
/// `FromFnLayerFactory` return type is hostile to callers; the
/// call site is three lines and easier to read.
pub async fn handle(
    req: Request,
    next: Next,
    store: Arc<dyn IdempotencyStore>,
    ttl: Duration,
) -> Response {
    // 1. Only POSTs are eligible for idempotency replay.
    if req.method() != Method::POST {
        return next.run(req).await;
    }

    // 2. Header must be present and non-empty.
    let key = req
        .headers()
        .get(&IDEMPOTENCY_KEY_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from);
    let key = match key {
        Some(k) => k,
        None => return next.run(req).await,
    };

    // 3. Cache hit → replay.
    if let Some(cached) = store.get(&key).await {
        return replay(cached);
    }

    // 4. Miss → execute + (maybe) cache.
    let response = next.run(req).await;
    let (parts, body) = response.into_parts();

    if !parts.status.is_success() {
        // Don't cache 4xx / 5xx — clients should be free to retry
        // on transient failures without hitting a stale error
        // response.
        return Response::from_parts(parts, body);
    }

    // Buffer the body so we can both cache it and re-emit it.
    let body_bytes = match to_bytes(body, MAX_CACHEABLE_BODY).await {
        Ok(b) => b,
        Err(err) => {
            // Body too large to buffer — log + pass through without
            // caching. The client gets a fresh response next time
            // they retry; a stream endpoint that legitimately
            // exceeds 1 MiB is free to opt out by not sending the
            // header.
            tracing::warn!(
                key = %key,
                error = %err,
                "idempotency body too large to buffer; skipping cache"
            );
            // Reconstruct an empty body so the response is valid.
            return Response::from_parts(parts, Body::empty());
        }
    };

    let content_type = parts
        .headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/json")
        .to_string();

    let cached = CachedResponse {
        status: parts.status.as_u16(),
        content_type,
        body: body_bytes.to_vec(),
    };
    store.put(&key, cached, ttl).await;

    Response::from_parts(parts, Body::from(body_bytes))
}

/// Reconstruct a `Response` from a cached entry, tagging it with
/// `Idempotency-Replayed: true` so the client can confirm.
fn replay(cached: CachedResponse) -> Response {
    let status = StatusCode::from_u16(cached.status).unwrap_or(StatusCode::OK);
    let mut response = (status, Body::from(cached.body)).into_response();
    if let Ok(val) = HeaderValue::from_str(&cached.content_type) {
        response.headers_mut().insert(header::CONTENT_TYPE, val);
    }
    response.headers_mut().insert(
        &IDEMPOTENCY_REPLAYED_HEADER,
        HeaderValue::from_static("true"),
    );
    response
}

/// Strict-mode guard (T-15).
///
/// For protected routes (`/orders`, `/payments`, `/auth/register`)
/// the `Idempotency-Key` header is **required**. Requests without
/// it get a 400 with a clear error message; clients can then
/// retry with the header. This makes the contract explicit
/// instead of silently permissively caching — the global
/// permissive layer (`handle`) only fires when the header is
/// actually present, so a request that *does* have a header
/// flows through both layers without double-caching.
pub async fn require_idempotency_key(req: Request, next: Next) -> Response {
    // Only enforce on POSTs — GETs don't need idempotency.
    if req.method() != Method::POST {
        return next.run(req).await;
    }

    let has_key = req
        .headers()
        .get(&IDEMPOTENCY_KEY_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .map(|s| !s.is_empty())
        .unwrap_or(false);

    if has_key {
        next.run(req).await
    } else {
        let body = serde_json::json!({
            "success": false,
            "error": {
                "code": "idempotency_key_required",
                "message": "Idempotency-Key header is required for this endpoint",
            }
        });
        let mut response = (
            StatusCode::BAD_REQUEST,
            [(header::CONTENT_TYPE, "application/json")],
            body.to_string(),
        )
            .into_response();
        response.headers_mut().insert(
            &IDEMPOTENCY_KEY_HEADER,
            HeaderValue::from_static("required"),
        );
        response
    }
}

#[cfg(test)]
mod strict_tests {
    use super::*;
    use axum::body::to_bytes;
    use axum::routing::{get, post};
    use axum::Router;
    use tower::ServiceExt;

    #[tokio::test]
    async fn require_idempotency_key_passes_through_with_key() {
        let app = Router::new()
            .route("/p", post(|| async { "ok" }))
            .route("/g", get(|| async { "ok" }))
            .layer(axum::middleware::from_fn(require_idempotency_key));

        // POST with key → handler runs.
        let r = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/p")
                    .header(IDEMPOTENCY_KEY_HEADER, "abc")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::OK);
        let body = to_bytes(r.into_body(), 1024).await.unwrap();
        assert_eq!(&body[..], b"ok");

        // GET without key → handler still runs (GETs are exempt).
        let r = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/g")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn require_idempotency_key_rejects_post_without_key() {
        let app = Router::new()
            .route("/p", post(|| async { "ok" }))
            .layer(axum::middleware::from_fn(require_idempotency_key));

        let r = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/p")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn require_idempotency_key_rejects_whitespace_key() {
        let app = Router::new()
            .route("/p", post(|| async { "ok" }))
            .layer(axum::middleware::from_fn(require_idempotency_key));

        let r = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/p")
                    .header(IDEMPOTENCY_KEY_HEADER, "   ")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::BAD_REQUEST);
    }
}
