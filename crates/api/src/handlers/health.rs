

use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use kokkak_common::response::ApiResponse;
use kokkak_domain::HealthRegistry;
use serde::Serialize;

#[utoipa::path(
    get,
    path = "/healthz",
    tag = "health",
    responses(
        (status = 200, description = "Process is alive", body = String),
    )
)]
pub async fn healthz() -> impl IntoResponse {
    (StatusCode::OK, "OK")
}

#[utoipa::path(
    get,
    path = "/readyz",
    tag = "health",
    responses(
        (status = 200, description = "All checks passed"),
        (status = 503, description = "One or more checks failed"),
    )
)]
pub async fn readyz(State(registry): State<HealthRegistry>) -> impl IntoResponse {
    let report = registry.run_all().await;

    for outcome in &report.checks {
        if !outcome.ok {
            tracing::warn!(
                check = %outcome.name,
                error = outcome.error.as_deref().unwrap_or("unknown"),
                "health check failed"
            );
        }
    }

    let is_ready = report.is_ready();
    let data = ReadyData::from(report);
    let envelope = ApiResponse {
        success: is_ready,
        data: Some(data),
        error: None,
        meta: None,
    };
    let status = if is_ready {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (status, Json(envelope))
}

#[derive(Debug, Serialize)]
struct ReadyData {

    checks: Vec<CheckView>,
}

#[derive(Debug, Serialize)]
struct CheckView {

    name: String,

    status: &'static str,

    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

impl From<kokkak_domain::ReadyReport> for ReadyData {
    fn from(report: kokkak_domain::ReadyReport) -> Self {
        let checks = report
            .checks
            .into_iter()
            .map(|o| CheckView {
                status: if o.ok { "up" } else { "down" },
                name: o.name,
                error: o.error,
            })
            .collect();
        Self { checks }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use axum::routing::get;
    use axum::Router;
    use kokkak_domain::HealthCheck;
    use std::sync::Arc;
    use tower::ServiceExt;

    struct UpCheck;
    #[async_trait::async_trait]
    impl HealthCheck for UpCheck {
        fn name(&self) -> &str {
            "up"
        }
        async fn check(&self) -> Result<(), kokkak_domain::HealthError> {
            Ok(())
        }
    }

    struct DownCheck;
    #[async_trait::async_trait]
    impl HealthCheck for DownCheck {
        fn name(&self) -> &str {
            "down"
        }
        async fn check(&self) -> Result<(), kokkak_domain::HealthError> {
            Err(kokkak_domain::HealthError::Failed("boom".into()))
        }
    }

    fn app_with(checks: Vec<Arc<dyn HealthCheck>>) -> Router {
        let mut reg = HealthRegistry::new();
        for c in checks {
            reg.register(c);
        }
        Router::new()
            .route("/healthz", get(healthz))
            .route("/readyz", get(readyz))
            .with_state(reg)
    }

    #[tokio::test]
    async fn healthz_returns_200_ok() {
        let app = app_with(vec![]);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/healthz")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn readyz_empty_registry_returns_200_with_empty_checks() {
        let app = app_with(vec![]);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/readyz")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["success"], true);
        assert_eq!(v["data"]["checks"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn readyz_all_up_returns_200() {
        let app = app_with(vec![Arc::new(UpCheck)]);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/readyz")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["success"], true);
        assert_eq!(v["data"]["checks"][0]["name"], "up");
        assert_eq!(v["data"]["checks"][0]["status"], "up");
    }

    #[tokio::test]
    async fn readyz_any_down_returns_503_with_error_detail() {
        let app = app_with(vec![Arc::new(UpCheck), Arc::new(DownCheck)]);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/readyz")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
        let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["success"], false);
        assert_eq!(v["data"]["checks"][0]["status"], "up");
        assert_eq!(v["data"]["checks"][1]["status"], "down");
        assert_eq!(v["data"]["checks"][1]["error"], "boom");
    }
}
