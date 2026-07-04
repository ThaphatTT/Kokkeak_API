

use axum::{
    async_trait,
    extract::{ConnectInfo, FromRequest, FromRequestParts, Request},
    http::request::Parts,
    Json,
};
use kokkak_common::config::Settings;
use kokkak_common::error::AppError;
use serde::de::DeserializeOwned;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use validator::Validate;

use crate::error::ApiError;

pub struct ValidatedJson<T>(pub T);

#[async_trait]
impl<T, S> FromRequest<S> for ValidatedJson<T>
where
    T: DeserializeOwned + Validate,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {

        let Json(value) = Json::<T>::from_request(req, state)
            .await
            .map_err(|e| ApiError(AppError::BadRequest(format!("invalid JSON body: {e}"))))?;

        value
            .validate()
            .map_err(|e| ApiError(AppError::Validation(e.to_string())))?;

        Ok(Self(value))
    }
}

pub struct ClientIp(pub Option<IpAddr>);

#[async_trait]
impl<S> FromRequestParts<S> for ClientIp
where
    S: Send + Sync,
{
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {

        let trust_forwarded_for = parts
            .extensions
            .get::<Arc<Settings>>()
            .map(|s| s.server.trust_forwarded_for)
            .unwrap_or(true);
        if trust_forwarded_for {
            if let Some(forwarded) = parts.headers.get("x-forwarded-for") {
                if let Ok(s) = forwarded.to_str() {
                    if let Some(first) = s.split(',').next() {
                        if let Ok(ip) = first.trim().parse::<IpAddr>() {
                            return Ok(ClientIp(Some(ip)));
                        }
                    }
                }
            }
        }

        if let Ok(ConnectInfo(addr)) =
            ConnectInfo::<SocketAddr>::from_request_parts(parts, state).await
        {
            return Ok(ClientIp(Some(addr.ip())));
        }

        Ok(ClientIp(None))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        http::{header::CONTENT_TYPE, Request as HttpRequest, StatusCode},
        routing::post,
        Router,
    };
    use serde::{Deserialize, Serialize};
    use tower::ServiceExt;
    use validator::Validate;

    #[derive(Debug, Deserialize, Serialize, Validate, PartialEq)]
    struct Echo {
        #[validate(length(min = 3, max = 10))]
        name: String,
        #[validate(range(min = 0, max = 150))]
        age: u32,
    }

    async fn echo_handler(ValidatedJson(req): ValidatedJson<Echo>) -> Json<Echo> {
        Json(req)
    }

    fn app() -> Router {
        Router::new().route("/echo", post(echo_handler))
    }

    #[tokio::test]
    async fn valid_body_passes_through() {
        let body = serde_json::to_string(&Echo {
            name: "alice".into(),
            age: 30,
        })
        .unwrap();
        let req = HttpRequest::builder()
            .method("POST")
            .uri("/echo")
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from(body))
            .unwrap();
        let resp = app().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
        let parsed: Echo = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(parsed.name, "alice");
        assert_eq!(parsed.age, 30);
    }

    #[tokio::test]
    async fn short_name_returns_422_validation() {
        let body = r#"{"name":"a","age":30}"#;
        let req = HttpRequest::builder()
            .method("POST")
            .uri("/echo")
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from(body))
            .unwrap();
        let resp = app().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn age_out_of_range_returns_422_validation() {
        let body = r#"{"name":"alice","age":200}"#;
        let req = HttpRequest::builder()
            .method("POST")
            .uri("/echo")
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from(body))
            .unwrap();
        let resp = app().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn malformed_json_returns_400_bad_request() {
        let req = HttpRequest::builder()
            .method("POST")
            .uri("/echo")
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from("{not json"))
            .unwrap();
        let resp = app().oneshot(req).await.unwrap();

        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn validate_directly_still_works() {
        let ok = Echo {
            name: "alice".into(),
            age: 30,
        };
        assert!(ok.validate().is_ok());
        let bad = Echo {
            name: "x".into(),
            age: 999,
        };
        let err = bad.validate().unwrap_err();

        let msg = err.to_string();
        assert!(msg.contains("name"), "expected name in error, got: {msg}");
        assert!(msg.contains("age"), "expected age in error, got: {msg}");
    }

    use axum::extract::FromRequestParts;
    use axum::http::Request;
    use std::net::IpAddr;

    fn parts_from_request(req: Request<Body>) -> Parts {
        req.into_parts().0
    }

    async fn extract_ip(req: Request<Body>) -> Option<IpAddr> {
        let mut parts = parts_from_request(req);
        ClientIp::from_request_parts(&mut parts, &())
            .await
            .unwrap()
            .0
    }

    #[tokio::test]
    async fn x_forwarded_for_header_wins() {

        let mut req = Request::builder()
            .header("x-forwarded-for", "203.0.113.5")
            .body(Body::empty())
            .unwrap();
        req.extensions_mut()
            .insert(ConnectInfo(SocketAddr::from(([10, 0, 0, 1], 12345))));
        let ip = extract_ip(req).await;
        assert_eq!(ip, Some("203.0.113.5".parse::<IpAddr>().unwrap()));
    }

    #[tokio::test]
    async fn x_forwarded_for_leftmost_wins_in_chain() {

        let req = Request::builder()
            .header("x-forwarded-for", "203.0.113.5, 198.51.100.1, 10.0.0.1")
            .body(Body::empty())
            .unwrap();
        let ip = extract_ip(req).await;
        assert_eq!(ip, Some("203.0.113.5".parse::<IpAddr>().unwrap()));
    }

    #[tokio::test]
    async fn x_forwarded_for_ipv6_supported() {
        let req = Request::builder()
            .header("x-forwarded-for", "2001:db8::1")
            .body(Body::empty())
            .unwrap();
        let ip = extract_ip(req).await;
        assert_eq!(ip, Some("2001:db8::1".parse::<IpAddr>().unwrap()));
    }

    #[tokio::test]
    async fn falls_back_to_connect_info_when_no_header() {

        let mut req = Request::builder().body(Body::empty()).unwrap();
        req.extensions_mut()
            .insert(ConnectInfo(SocketAddr::from(([10, 0, 0, 1], 12345))));
        let ip = extract_ip(req).await;
        assert_eq!(ip, Some("10.0.0.1".parse::<IpAddr>().unwrap()));
    }

    #[tokio::test]
    async fn malformed_x_forwarded_for_falls_through() {

        let mut req = Request::builder()
            .header("x-forwarded-for", "not-an-ip")
            .body(Body::empty())
            .unwrap();
        req.extensions_mut()
            .insert(ConnectInfo(SocketAddr::from(([10, 0, 0, 1], 12345))));
        let ip = extract_ip(req).await;
        assert_eq!(ip, Some("10.0.0.1".parse::<IpAddr>().unwrap()));
    }

    #[tokio::test]
    async fn returns_none_when_neither_source_available() {

        let req = Request::builder().body(Body::empty()).unwrap();
        let ip = extract_ip(req).await;
        assert_eq!(ip, None);
    }

    #[tokio::test]
    async fn x_forwarded_for_with_whitespace_trims() {

        let req = Request::builder()
            .header("x-forwarded-for", "  203.0.113.5  ,  198.51.100.1  ")
            .body(Body::empty())
            .unwrap();
        let ip = extract_ip(req).await;
        assert_eq!(ip, Some("203.0.113.5".parse::<IpAddr>().unwrap()));
    }
}
