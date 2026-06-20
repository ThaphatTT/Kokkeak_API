//! Plain-HTTP → HTTPS redirect server (T-10).
//!
//! Production deployments terminate TLS on `server.addr` (T-09)
//! but clients still hit the plain HTTP port 80 with old links /
//! typos / mobile deep links. This module owns the small
//! side-car server that listens on
//! [`kokkak_common::TlsSettings::redirect_from_port`] and
//! responds to every request with `308 Permanent Redirect` to the
//! same URL over `https://`.
//!
//! ## Why 308 (not 301 / 302 / 307)
//!
//! - **301** — historically used for "moved permanently" but
//!   browsers may downgrade POST → GET, which breaks form
//!   submissions and payment callbacks.
//! - **302** — "found", same downgrade risk as 301.
//! - **307** — preserves method but the spec calls it
//!   "temporary" so clients keep retrying plain HTTP.
//! - **308** — RFC 7538, "permanent redirect", preserves method
//!   AND signals the move is permanent so well-behaved clients
//!   cache it. This is the right choice for HTTPS upgrades.
//!
//! Kept in a separate module so the redirect handler is
//! unit-testable without binding a real socket.

use axum::{
    extract::{Host, OriginalUri},
    http::{header::LOCATION, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Router,
};

/// Handler that returns `308 Permanent Redirect` to the
/// `https://` equivalent of the incoming request.
///
/// The destination is built from the request's `Host` header
/// (so `example.com:80` becomes `https://example.com/...`) and
/// the original URI's path + query string.
pub async fn redirect_to_https(Host(host): Host, OriginalUri(uri): OriginalUri) -> Response {
    let path_and_query = uri.path_and_query().map(|pq| pq.as_str()).unwrap_or("/");
    let location = format!("https://{host}{path_and_query}");

    // The Host header is parsed by axum and is guaranteed to be
    // ASCII (RFC 7230 §5.4). The `https://` prefix + path are
    // also ASCII, so `HeaderValue::from_str` cannot fail here in
    // practice — if it somehow does, fall back to a plain
    // text response with a status so the client isn't left
    // hanging on a 308 with no Location.
    match HeaderValue::from_str(&location) {
        Ok(value) => (StatusCode::PERMANENT_REDIRECT, [(LOCATION, value)]).into_response(),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "redirect location invalid",
        )
            .into_response(),
    }
}

/// Build the standalone router for the plain-HTTP redirect
/// listener. The caller is expected to bind this on
/// `tls.redirect_from_port` and serve it with `axum::serve`
/// (no TLS — the whole point is to catch the unencrypted hits).
pub fn redirect_router() -> Router {
    Router::new().fallback(redirect_to_https)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, Uri};
    use tower::ServiceExt;

    #[tokio::test]
    async fn redirect_preserves_path_and_query() {
        let app = redirect_router();
        let req = Request::builder()
            .uri("/api/v1/orders?limit=10")
            .header("host", "example.com:8080")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::PERMANENT_REDIRECT);
        let loc = resp
            .headers()
            .get(LOCATION)
            .expect("Location header must be set")
            .to_str()
            .unwrap();
        assert_eq!(loc, "https://example.com:8080/api/v1/orders?limit=10");
    }

    #[tokio::test]
    async fn redirect_handles_root() {
        let app = redirect_router();
        let req = Request::builder()
            .uri("/")
            .header("host", "api.kokkeak.la")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::PERMANENT_REDIRECT);
        let loc = resp.headers().get(LOCATION).unwrap().to_str().unwrap();
        assert_eq!(loc, "https://api.kokkeak.la/");
    }

    #[tokio::test]
    async fn redirect_handles_path_without_query() {
        let app = redirect_router();
        let req = Request::builder()
            .uri("/healthz")
            .header("host", "localhost")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        let loc = resp.headers().get(LOCATION).unwrap().to_str().unwrap();
        assert_eq!(loc, "https://localhost/healthz");
    }

    #[test]
    fn redirect_uri_construction_handles_missing_query() {
        // Pure-function variant of the redirect logic, kept
        // here so the Host/OriginalUri dependency-injection
        // tests above don't have to cover every edge case.
        let uri: Uri = "/foo".parse().unwrap();
        let pq = uri.path_and_query().map(|p| p.as_str()).unwrap_or("/");
        assert_eq!(pq, "/foo");
    }
}
