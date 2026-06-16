//! Standard response envelope used by every API handler.
//!
//! Shape (per AGENTS.md § 11.2):
//! ```json
//! { "success": true, "data": {...}, "error": null, "meta": { "page": {...} } }
//! ```
//!
//! Use [`ok`], [`created`], [`paginated`] to build success responses, and
//! [`ApiResponse::error`] (or [`crate::error::AppError`] via
//! `IntoResponse`) for failures.

use axum::Json;
use serde::Serialize;

use crate::error::ApiErrorBody;

/// Standard envelope wrapping either a `data` payload, an `error` body,
/// or both. `error` and `meta` are omitted from the serialized JSON
/// when `None` to keep payloads tidy.
#[derive(Debug, Serialize, Clone)]
pub struct ApiResponse<T> {
    /// `true` for `2xx` responses, `false` for `4xx`/`5xx`.
    pub success: bool,

    /// Success payload. `None` on failure.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,

    /// Failure payload. `None` on success.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ApiErrorBody>,

    /// Pagination metadata. `None` if not applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<PageMeta>,
}

/// Pagination metadata embedded in [`ApiResponse::meta`].
#[derive(Debug, Serialize, Clone)]
pub struct PageMeta {
    /// Page size (capped at the configured maximum).
    pub limit: usize,
    /// `true` if more rows are available after this page.
    pub has_next: bool,
    /// Opaque cursor to fetch the next page; absent when `has_next = false`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

impl<T> ApiResponse<T> {
    /// Build an error envelope with no data/meta.
    pub fn error(code: impl Into<String>, message: impl Into<String>) -> ApiResponse<T> {
        ApiResponse {
            success: false,
            data: None,
            error: Some(ApiErrorBody {
                code: code.into(),
                message: message.into(),
            }),
            meta: None,
        }
    }
}

/// 200 OK envelope: `{"success": true, "data": <T>}`.
pub fn ok<T: Serialize>(data: T) -> Json<ApiResponse<T>> {
    Json(ApiResponse {
        success: true,
        data: Some(data),
        error: None,
        meta: None,
    })
}

/// 201 Created envelope (same shape as `ok`; the status is set by the
/// call site — typically by returning `(StatusCode::CREATED, created(...))`).
pub fn created<T: Serialize>(data: T) -> Json<ApiResponse<T>> {
    Json(ApiResponse {
        success: true,
        data: Some(data),
        error: None,
        meta: None,
    })
}

/// 200 OK envelope with pagination metadata.
pub fn paginated<T: Serialize>(data: T, meta: PageMeta) -> Json<ApiResponse<T>> {
    Json(ApiResponse {
        success: true,
        data: Some(data),
        error: None,
        meta: Some(meta),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn ok_envelope_shape() {
        let resp = ok(42);
        let v = serde_json::to_value(&resp.0).unwrap();
        assert_eq!(v, json!({ "success": true, "data": 42 }));
    }

    #[test]
    fn created_envelope_shape() {
        let resp = created("ok");
        let v = serde_json::to_value(&resp.0).unwrap();
        assert_eq!(v, json!({ "success": true, "data": "ok" }));
    }

    #[test]
    fn paginated_envelope_with_cursor() {
        let meta = PageMeta {
            limit: 20,
            has_next: true,
            next_cursor: Some("abc".into()),
        };
        let resp = paginated(vec![1, 2, 3], meta);
        let v = serde_json::to_value(&resp.0).unwrap();
        assert_eq!(
            v,
            json!({
                "success": true,
                "data": [1, 2, 3],
                "meta": { "limit": 20, "has_next": true, "next_cursor": "abc" }
            })
        );
    }

    #[test]
    fn paginated_no_next_cursor_omits_field() {
        let meta = PageMeta {
            limit: 20,
            has_next: false,
            next_cursor: None,
        };
        let resp = paginated(vec![1], meta);
        let v = serde_json::to_value(&resp.0).unwrap();
        assert_eq!(
            v,
            json!({
                "success": true,
                "data": [1],
                "meta": { "limit": 20, "has_next": false }
            })
        );
    }

    #[test]
    fn error_envelope_shape() {
        let resp: ApiResponse<i32> = ApiResponse::error("not_found", "user 42");
        let v = serde_json::to_value(&resp).unwrap();
        assert_eq!(
            v,
            json!({
                "success": false,
                "error": { "code": "not_found", "message": "user 42" }
            })
        );
    }
}
