

use std::time::Instant;

use axum::{
    extract::Request,
    http::{header::HeaderName, HeaderValue},
    middleware::Next,
    response::Response,
};
use uuid::Uuid;

pub const X_REQUEST_ID: HeaderName = HeaderName::from_static("x-request-id");

#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
pub struct RequestId(pub Uuid);

pub async fn trace_request(mut req: Request, next: Next) -> Response {
    let request_id = Uuid::now_v7();
    let method = req.method().clone();
    let path = req.uri().path().to_string();
    let start = Instant::now();

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

    if let Ok(val) = HeaderValue::from_str(&request_id.to_string()) {
        response.headers_mut().insert(X_REQUEST_ID, val);
    }

    response
}
