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

pub const IDEMPOTENCY_KEY_HEADER: HeaderName = HeaderName::from_static("idempotency-key");

pub const IDEMPOTENCY_REPLAYED_HEADER: HeaderName = HeaderName::from_static("idempotency-replayed");

pub const X_RETRY_COUNT_HEADER: HeaderName = HeaderName::from_static("x-retry-count");

const MAX_CACHEABLE_BODY: usize = 1024 * 1024;

pub async fn handle(
    req: Request,
    next: Next,
    store: Arc<dyn IdempotencyStore>,
    ttl: Duration,
) -> Response {
    if req.method() != Method::POST {
        return next.run(req).await;
    }

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

    if let Some(cached) = store.get(&key).await {
        return replay(cached);
    }

    let response = next.run(req).await;
    let (parts, body) = response.into_parts();

    if !parts.status.is_success() {
        return Response::from_parts(parts, body);
    }

    let body_bytes = match to_bytes(body, MAX_CACHEABLE_BODY).await {
        Ok(b) => b,
        Err(err) => {
            tracing::warn!(
                key = %key,
                error = %err,
                "idempotency body too large to buffer; skipping cache"
            );

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

pub async fn require_idempotency_key(req: Request, next: Next) -> Response {
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
