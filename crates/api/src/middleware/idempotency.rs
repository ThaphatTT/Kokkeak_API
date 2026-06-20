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
    response::{IntoResponse, Response},
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
