

use axum::{
    extract::{Host, OriginalUri},
    http::{header::LOCATION, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Router,
};

pub async fn redirect_to_https(Host(host): Host, OriginalUri(uri): OriginalUri) -> Response {
    let path_and_query = uri.path_and_query().map(|pq| pq.as_str()).unwrap_or("/");
    let location = format!("https://{host}{path_and_query}");

    match HeaderValue::from_str(&location) {
        Ok(value) => (StatusCode::PERMANENT_REDIRECT, [(LOCATION, value)]).into_response(),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            "redirect location invalid",
        )
            .into_response(),
    }
}

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

        let uri: Uri = "/foo".parse().unwrap();
        let pq = uri.path_and_query().map(|p| p.as_str()).unwrap_or("/");
        assert_eq!(pq, "/foo");
    }
}
