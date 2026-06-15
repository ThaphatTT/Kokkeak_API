//! HTTP middleware: per-request tracing + Prometheus metrics.
//!
//! Responsibilities:
//! 1. Generate a request id (UUID v7 — time-sortable) per request.
//! 2. Inject it into request extensions (handlers can read via `Extension<RequestId>`).
//! 3. Add it to the response as the `x-request-id` header.
//! 4. Log request start / complete with method, path, status, latency.
//! 5. Increment `http_requests_total` counter and record duration histogram.

use std::time::Instant;

use axum::{
    extract::Request,
    http::{HeaderValue, header::HeaderName},
    middleware::Next,
    response::Response,
};
use uuid::Uuid;

/// Header name for the request id (`x-request-id`).
pub const X_REQUEST_ID: HeaderName = HeaderName::from_static("x-request-id");

/// Extracted request id (use `Extension<RequestId>` in handlers).
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)] // read by handlers via `Extension<RequestId>` in future tasks
pub struct RequestId(pub Uuid);

/// Per-request tracing + metrics middleware.
pub async fn trace_request(mut req: Request, next: Next) -> Response {
    let request_id = Uuid::now_v7();
    let method = req.method().clone();
    let path = req.uri().path().to_string();
    let start = Instant::now();

    // Make the request id available to downstream handlers
    req.extensions_mut().insert(RequestId(request_id));

    tracing::info!(
        request_id = %request_id,
        method = %method,
        path = %path,
        "request started"
    );

    let mut response = next.run(req).await;
    let status = response.status().as_u16();
    let latency = start.elapsed();

    tracing::info!(
        request_id = %request_id,
        method = %method,
        path = %path,
        status,
        latency_ms = latency.as_millis() as u64,
        "request completed"
    );

    // Record metrics (use display labels to keep cardinality bounded)
    metrics::counter!(
        "http_requests_total",
        "method" => method.as_str().to_string(),
        "path" => path.clone(),
        "status" => status.to_string(),
    )
    .increment(1);
    metrics::histogram!(
        "http_request_duration_seconds",
        "method" => method.as_str().to_string(),
        "path" => path.clone(),
    )
    .record(latency.as_secs_f64());

    // Stamp the response so clients can correlate with logs
    if let Ok(val) = HeaderValue::from_str(&request_id.to_string()) {
        response.headers_mut().insert(X_REQUEST_ID, val);
    }

    response
}
