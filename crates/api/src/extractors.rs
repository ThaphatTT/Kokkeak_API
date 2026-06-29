//! Custom axum extractors (T-07).
//!
//! Provides [`ValidatedJson<T>`] — a thin wrapper around
//! [`axum::Json`] that runs the `validator` crate's
//! [`Validate::validate`] after JSON deserialization. Handlers
//! that take a request body should prefer `ValidatedJson` over
//! plain `Json` so semantic checks (length, email shape, regex)
//! happen at the trust boundary, before any business logic.
//!
//! ## Why a custom extractor?
//!
//! `validator` works on plain structs — the cleanest way to plug
//! it into axum is to wrap `Json<T>` and call `.validate()` inside
//! the extractor. That keeps handler signatures ergonomic
//! (`ValidatedJson<RegisterRequest>` reads as "validated JSON of
//! type X") and centralizes the failure mapping (`ValidationErrors`
//! → [`AppError::Validation`]).
//!
//! ponytail: zero new dep — `validator` is already in the
//! workspace deps. The failure path goes through the existing
//! `AppError::Validation` variant so handlers don't need a
//! separate error type. Ceiling: if a future endpoint needs
//! *contextual* validation (e.g. cross-field checks against the
//! DB), it should call `.validate()` manually after the extractor
//! deserializes — the extractor can only check shape, not state.

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

/// JSON request body that has been both deserialized and
/// semantically validated.
///
/// Behaves like [`Json<T>`] but rejects the request with a 422
/// `validation` envelope if [`Validate::validate`] returns an
/// error after deserialization.
pub struct ValidatedJson<T>(pub T);

#[async_trait]
impl<T, S> FromRequest<S> for ValidatedJson<T>
where
    T: DeserializeOwned + Validate,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        // Stage 1: JSON shape — delegate to axum's Json extractor so
        // we get its standard 400 for malformed bodies. Map any
        // rejection through AppError::BadRequest so the envelope
        // shape matches the rest of the API.
        let Json(value) = Json::<T>::from_request(req, state)
            .await
            .map_err(|e| ApiError(AppError::BadRequest(format!("invalid JSON body: {e}"))))?;

        // Stage 2: semantic validation (length, email, regex, ...).
        // The validator crate returns a ValidationErrors tree; we
        // serialize it with Display so the user sees which field
        // failed without needing to parse the structured errors.
        value
            .validate()
            .map_err(|e| ApiError(AppError::Validation(e.to_string())))?;

        Ok(Self(value))
    }
}

/// `ClientIp` — extracts the **real** client IP for audit logging
/// and per-(username, IP) rate limiting.
///
/// Resolution order:
///
/// 1. **`X-Forwarded-For` header** (leftmost IP). Used when the
///    service runs behind a reverse proxy (nginx, Cloudflare, AWS
///    ALB, ...). The leftmost IP in `X-Forwarded-For: client, p1, p2`
///    is the **original** client per the de-facto convention. The
///    proxy must be configured to **strip** any client-supplied
///    `X-Forwarded-For` and append the real client IP — see the
///    deployment guide at the bottom of this doc-comment.
///
/// 2. **`ConnectInfo<SocketAddr>`** — the TCP peer address when the
///    service is hit directly (no proxy, dev mode, integration
///    tests with `into_make_service_with_connect_info`).
///
/// 3. **`None`** — neither source is available (e.g. an in-process
///    call or a test that doesn't set up the connect info). The
///    handler decides what to do; today the auth use case skips the
///    per-(username, IP) rate-limit gate when this is `None`.
///
/// Security:
/// - `X-Forwarded-For` is **spoofable** by any client if the
///   deployment does not run behind a trusted proxy. In a
///   direct-internet deployment without a proxy, an attacker can
///   set this header to any IP. Today we **always trust** the
///   header for simplicity — the trade-off is a slightly weaker
///   audit log when the deployment is mis-configured. The
///   upgrade path is a `KOKKAK_TRUST_FORWARDED_FOR` config flag
///   (default `true` in deployments with a reverse proxy, default
///   `false` in direct-internet deployments); the actual logic
///   change is a 3-line `if settings.trust_forwarded_for { ... }`.
/// - `ConnectInfo` is not spoofable — the kernel fills in the peer
///   address on `accept()`.
///
/// ponytail: deliberately small and stateless. No cached IP, no
/// header rewriting. Each request pays one extra header read +
/// one extra extractor instantiation. Acceptable for a login
/// endpoint (sub-ms work); if hot paths ever need the same
/// extractor, hoist it to a `FromRequestParts` impl that caches
/// in a request extension.
pub struct ClientIp(pub Option<IpAddr>);

#[async_trait]
impl<S> FromRequestParts<S> for ClientIp
where
    S: Send + Sync,
{
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        // 1. X-Forwarded-For (RFC 7239 / de-facto header). Format:
        //    "client, proxy1, proxy2" — leftmost is the original
        //    client. We accept IPv4 and IPv6; anything malformed is
        //    silently skipped so a misbehaving header can't lock
        //    out the whole endpoint.
        //
        // Security: gated by `KOKKAK_SERVER__TRUST_FORWARDED_FOR`
        // (default `true`). Production behind IIS / nginx / envoy /
        // ALB MUST keep it `true`; the proxy strips any
        // client-supplied header and appends the real peer IP.
        // Direct-internet deployments (no proxy) MUST set it
        // `false` — otherwise any client can spoof its IP and
        // bypass per-(username, IP) rate limits / poison the
        // audit log.
        //
        // ponytail: defaulting to `true` when the extension is
        // missing preserves backward compatibility with the
        // bare `Router<()>` tests below — they don't wire up
        // `Arc<Settings>` so they implicitly trust the header.
        // In production, `crates/api/src/main.rs` always injects
        // the extension, so the gate is always live.
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
        // 2. ConnectInfo<SocketAddr> (axum auto-fills from the
        //    accepted TCP socket when the router is wrapped in
        //    `into_make_service_with_connect_info::<SocketAddr>()`).
        //    The Result is always Ok because ConnectInfo's
        //    Rejection is `Infallible`; `.ok()` keeps the compiler
        //    happy without `unwrap`.
        // `ConnectInfo::Rejection` is `Infallible` so the `match`
        // collapses to the `Ok` arm — `let-else` keeps the original
        // behaviour (return early) without the `if-let + .ok()` that
        // clippy flags as redundant.
        if let Ok(ConnectInfo(addr)) =
            ConnectInfo::<SocketAddr>::from_request_parts(parts, state).await
        {
            return Ok(ClientIp(Some(addr.ip())));
        }
        // 3. Neither source — in-process callers, tests, or
        //    mis-wired routers. The handler treats this as "no
        //    IP" and skips the per-(username, IP) gate; audit
        //    log records `ip: null`.
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
        // Json extractor rejects before we get to validation, so the
        // status is 400 (BadRequest from our map_err), not 422.
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
        // Two fields failed — the ValidationErrors tree carries
        // both. Display uses the path: field format.
        let msg = err.to_string();
        assert!(msg.contains("name"), "expected name in error, got: {msg}");
        assert!(msg.contains("age"), "expected age in error, got: {msg}");
    }

    // ---- ClientIp tests ----
    //
    // These don't exercise `oneshot()` end-to-end (axum requires a
    // router that knows how to call the extractor) — we just drive
    // the extractor directly through `FromRequestParts::from_request_parts`
    // on a hand-built `Parts`. Same coverage, half the ceremony.

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
        // Direct connection from 10.0.0.1 (mimicked by
        // ConnectInfo<SocketAddr>), behind a proxy that added
        // X-Forwarded-For: 203.0.113.5 (the real client). The
        // extractor MUST pick the header value.
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
        // Multiple proxies in chain — leftmost is the original
        // client per de-facto convention.
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
        // No X-Forwarded-For — direct connection case.
        let mut req = Request::builder().body(Body::empty()).unwrap();
        req.extensions_mut()
            .insert(ConnectInfo(SocketAddr::from(([10, 0, 0, 1], 12345))));
        let ip = extract_ip(req).await;
        assert_eq!(ip, Some("10.0.0.1".parse::<IpAddr>().unwrap()));
    }

    #[tokio::test]
    async fn malformed_x_forwarded_for_falls_through() {
        // Garbage value — must NOT panic, must fall through to
        // ConnectInfo.
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
        // In-process call, no proxy, no ConnectInfo. The handler
        // should treat this as "no IP" and skip the rate-limit gate.
        let req = Request::builder().body(Body::empty()).unwrap();
        let ip = extract_ip(req).await;
        assert_eq!(ip, None);
    }

    #[tokio::test]
    async fn x_forwarded_for_with_whitespace_trims() {
        // nginx and CF sometimes add a leading space.
        let req = Request::builder()
            .header("x-forwarded-for", "  203.0.113.5  ,  198.51.100.1  ")
            .body(Body::empty())
            .unwrap();
        let ip = extract_ip(req).await;
        assert_eq!(ip, Some("203.0.113.5".parse::<IpAddr>().unwrap()));
    }
}
