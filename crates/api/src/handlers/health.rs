//! Liveness and readiness probes.
//!
//! - [`healthz`] is **liveness**: the process is up. Always 200.
//!   Used by orchestrators (k8s liveness probe) to decide whether to
//!   restart the container.
//!
//! - [`readyz`] is **readiness**: the process can serve traffic.
//!   Runs every check registered in [`HealthRegistry`]. Returns 200
//!   if all pass, 503 if any fail. Used by load balancers and k8s
//!   readiness probes to decide whether to send traffic to this
//!   instance.
//!
//! The body always lists every check (regardless of overall status)
//! so operators can curl `/readyz` and see exactly which dependency
//! is the problem.

use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use kokkak_common::response::ApiResponse;
use kokkak_domain::HealthRegistry;
use serde::Serialize;

/// `GET /healthz` — liveness probe.
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

/// `GET /readyz` — readiness probe.
///
/// Reads the [`HealthRegistry`] from axum state, runs every check in
/// parallel, and returns:
/// - **200 OK** with `success: true`  when all checks pass (or none are registered)
/// - **503 Service Unavailable** with `success: false` when any check fails
///
/// The body always includes a per-check breakdown so the failure
/// cause is visible without parsing logs.
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

    // Log each failed check at WARN so it surfaces in centralised
    // log aggregators even when no one curls /readyz.
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

/// Response body for `/readyz`.
#[derive(Debug, Serialize)]
struct ReadyData {
    /// One entry per registered check, in registration order.
    checks: Vec<CheckView>,
}

/// One row of the `/readyz` body.
#[derive(Debug, Serialize)]
struct CheckView {
    /// Stable check identifier (e.g. `"sqlserver"`).
    name: String,
    /// `"up"` or `"down"`.
    status: &'static str,
    /// Set only when `status == "down"`. Skipped from JSON otherwise.
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
