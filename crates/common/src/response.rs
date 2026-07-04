

use axum::Json;
use serde::Serialize;

use crate::error::ApiErrorBody;

#[derive(Debug, Serialize, Clone)]
pub struct ApiResponse<T> {

    pub success: bool,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ApiErrorBody>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<PageMeta>,
}

#[derive(Debug, Serialize, Clone)]
pub struct PageMeta {

    pub limit: usize,

    pub has_next: bool,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

impl<T> ApiResponse<T> {

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

pub fn ok<T: Serialize>(data: T) -> Json<ApiResponse<T>> {
    Json(ApiResponse {
        success: true,
        data: Some(data),
        error: None,
        meta: None,
    })
}

pub fn created<T: Serialize>(data: T) -> Json<ApiResponse<T>> {
    Json(ApiResponse {
        success: true,
        data: Some(data),
        error: None,
        meta: None,
    })
}

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
