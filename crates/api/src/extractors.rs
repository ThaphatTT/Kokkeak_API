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
    extract::{FromRequest, Request},
    Json,
};
use kokkak_common::error::AppError;
use serde::de::DeserializeOwned;
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
}
