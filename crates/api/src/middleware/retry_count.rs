use axum::{extract::Request, http::HeaderValue, middleware::Next, response::Response};

use super::idempotency::X_RETRY_COUNT_HEADER;

pub async fn retry_count_middleware(req: Request, next: Next) -> Response {
    let retry_count = req
        .headers()
        .get(&X_RETRY_COUNT_HEADER)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(0);

    if retry_count > 0 {
        tracing::debug!(
            retry_count = retry_count,
            path = %req.uri().path(),
            "request is a retry"
        );
    }

    let mut response = next.run(req).await;

    if retry_count > 0 {
        if let Ok(val) = HeaderValue::from_str(&retry_count.to_string()) {
            response.headers_mut().insert(&X_RETRY_COUNT_HEADER, val);
        }
    }

    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request, routing::get, Router};
    use tower::ServiceExt;

    #[tokio::test]
    async fn no_retry_count_header_passes_through() {
        let app = Router::new()
            .route("/test", get(|| async { "ok" }))
            .layer(axum::middleware::from_fn(retry_count_middleware));

        let resp = app
            .oneshot(Request::builder().uri("/test").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        assert!(resp.headers().get(&X_RETRY_COUNT_HEADER).is_none());
    }

    #[tokio::test]
    async fn retry_count_header_is_echoed_back() {
        let app = Router::new()
            .route("/test", get(|| async { "ok" }))
            .layer(axum::middleware::from_fn(retry_count_middleware));

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/test")
                    .header(&X_RETRY_COUNT_HEADER, "2")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let val = resp
            .headers()
            .get(&X_RETRY_COUNT_HEADER)
            .unwrap()
            .to_str()
            .unwrap();
        assert_eq!(val, "2");
    }

    #[tokio::test]
    async fn invalid_retry_count_ignored() {
        let app = Router::new()
            .route("/test", get(|| async { "ok" }))
            .layer(axum::middleware::from_fn(retry_count_middleware));

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/test")
                    .header(&X_RETRY_COUNT_HEADER, "abc")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        assert!(resp.headers().get(&X_RETRY_COUNT_HEADER).is_none());
    }
}
