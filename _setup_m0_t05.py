"""Create M0 T05 file structure: /readyz + HealthRegistry + graceful shutdown.

T05 (AGENTS.md § 19.1 / build plan):
    - /healthz (liveness, always 200) — kept from T01
    - /readyz  (readiness, runs HealthCheck[]; 503 if any fail)
    - SIGTERM / Ctrl-C → graceful shutdown (drain in-flight, then exit)

Layer split:
    - domain: HealthCheck trait + HealthRegistry + HealthError + CheckOutcome
    - api   : handlers::health (healthz, readyz) + main wiring
    - infra : (no impl yet — M0 has no concrete deps; M1+ adds SqlServer/Redis/NATS/Mongo)
"""

from pathlib import Path

ROOT = Path(r"C:\Users\crybo\Desktop\Develop\Kokkeak_API")

# ---------- 1. Update domain/Cargo.toml: add async-trait + thiserror ----------
(ROOT / "crates" / "domain" / "Cargo.toml").write_text(
    """[package]
name = "kokkak-domain"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
authors.workspace = true
description = "Domain entities, value objects, business rules, traits (no framework/DB)"

[dependencies]
# Ports (traits) + their typed errors
async-trait = { workspace = true }
thiserror = { workspace = true }

# Pure utility crate (no runtime); used for `future::join_all` to
# run health checks concurrently. Allowed in `domain` because it is
# a combinator library, not an IO/runtime.
futures = { workspace = true }

[dev-dependencies]
# Test runtime only — kept out of `[dependencies]` to keep `domain`
# runtime-pure (AGENTS.md § 6).
tokio = { workspace = true, features = ["macros", "rt"] }
""",
    encoding="utf-8",
)
print("  crates/domain/Cargo.toml")

# ---------- 2. Create domain/src/health.rs ----------
(ROOT / "crates" / "domain" / "src" / "health.rs").write_text(
    """//! Health-check ports (traits) and a small registry that runs them in parallel.
//!
//! This module follows the hexagonal pattern from AGENTS.md § 6:
//! the [`HealthCheck`] trait is a **port**. Concrete adapters
//! (SQL Server, Redis, NATS, Mongo) live in `infra` and implement
//! this trait.
//!
//! M0 ships only the trait + registry — no concrete checks are
//! wired yet. M1+ adds the real adapters in `crates/infra/src/health/`.

use async_trait::async_trait;
use thiserror::Error;

/// Why a single health check failed.
#[derive(Debug, Error)]
pub enum HealthError {
    /// Adapters wrap any IO / network / auth failure into this variant.
    #[error("{0}")]
    Failed(String),
}

/// A single dependency probe.
///
/// Implementors are adapters (e.g. `SqlServerHealthCheck`) that live in
/// `infra`. They must be cheap (a `SELECT 1`, a Redis `PING`, a NATS
/// `flush`, etc.) and bounded by a short timeout.
#[async_trait]
pub trait HealthCheck: Send + Sync {
    /// Stable identifier surfaced in `/readyz` output and logs
    /// (e.g. `"sqlserver"`, `"redis"`, `"nats"`, `"mongo"`).
    fn name(&self) -> &str;

    /// Run the probe. `Ok(())` = up. `Err` = down (with a short reason).
    async fn check(&self) -> Result<(), HealthError>;
}

/// One row of the readiness report.
#[derive(Debug, Clone)]
pub struct CheckOutcome {
    /// Check identifier (from [`HealthCheck::name`]).
    pub name: String,
    /// `true` if the probe returned `Ok`.
    pub ok: bool,
    /// Human-readable failure reason (only set when `ok == false`).
    pub error: Option<String>,
}

impl CheckOutcome {
    fn up(name: String) -> Self {
        Self { name, ok: true, error: None }
    }

    fn down(name: String, error: String) -> Self {
        Self { name, ok: false, error: Some(error) }
    }
}

/// Aggregated result of [`HealthRegistry::run_all`].
#[derive(Debug, Clone, Default)]
pub struct ReadyReport {
    /// One entry per registered check, in registration order.
    pub checks: Vec<CheckOutcome>,
}

impl ReadyReport {
    /// `true` iff every check passed (or the registry was empty).
    pub fn is_ready(&self) -> bool {
        self.checks.iter().all(|c| c.ok)
    }
}

/// Collection of [`HealthCheck`]s run together by `/readyz`.
///
/// Cheap to clone (the checks are behind `Arc`).
#[derive(Clone, Default)]
pub struct HealthRegistry {
    checks: Vec<std::sync::Arc<dyn HealthCheck>>,
}

impl HealthRegistry {
    /// Empty registry — every `/readyz` call returns 200 with no checks.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add one check. Order is preserved in `/readyz` output.
    pub fn register(&mut self, check: std::sync::Arc<dyn HealthCheck>) {
        self.checks.push(check);
    }

    /// Builder-style variant of [`Self::register`].
    #[must_use]
    pub fn with_check(mut self, check: std::sync::Arc<dyn HealthCheck>) -> Self {
        self.register(check);
        self
    }

    /// Number of registered checks.
    pub fn len(&self) -> usize {
        self.checks.len()
    }

    /// `true` when no checks have been registered.
    pub fn is_empty(&self) -> bool {
        self.checks.is_empty()
    }

    /// Run every registered check **in parallel** and collect outcomes.
    ///
    /// Checks are independent, so concurrency is the right call —
    /// the slowest check dominates total latency, not their sum.
    pub async fn run_all(&self) -> ReadyReport {
        use futures::future;

        let results = future::join_all(self.checks.iter().map(|check| {
            // Clone the name outside the async block so the borrow
            // does not span the `.await`.
            let name = check.name().to_string();
            async move {
                match check.check().await {
                    Ok(()) => CheckOutcome::up(name),
                    Err(err) => CheckOutcome::down(name, err.to_string()),
                }
            }
        }))
        .await;

        ReadyReport { checks: results }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct AlwaysOk;
    #[async_trait]
    impl HealthCheck for AlwaysOk {
        fn name(&self) -> &str {
            "always_ok"
        }
        async fn check(&self) -> Result<(), HealthError> {
            Ok(())
        }
    }

    struct AlwaysFail;
    #[async_trait]
    impl HealthCheck for AlwaysFail {
        fn name(&self) -> &str {
            "always_fail"
        }
        async fn check(&self) -> Result<(), HealthError> {
            Err(HealthError::Failed("simulated outage".into()))
        }
    }

    #[test]
    fn new_registry_is_empty() {
        let reg = HealthRegistry::new();
        assert!(reg.is_empty());
        assert_eq!(reg.len(), 0);
    }

    #[test]
    fn default_registry_is_empty() {
        let reg = HealthRegistry::default();
        assert!(reg.is_empty());
    }

    #[test]
    fn register_increments_len() {
        let mut reg = HealthRegistry::new();
        reg.register(std::sync::Arc::new(AlwaysOk));
        reg.register(std::sync::Arc::new(AlwaysFail));
        assert_eq!(reg.len(), 2);
        assert!(!reg.is_empty());
    }

    #[test]
    fn with_check_returns_updated_registry() {
        let reg = HealthRegistry::new().with_check(std::sync::Arc::new(AlwaysOk));
        assert_eq!(reg.len(), 1);
    }

    #[tokio::test]
    async fn run_all_with_no_checks_returns_empty_report() {
        let reg = HealthRegistry::new();
        let report = reg.run_all().await;
        assert!(report.checks.is_empty());
        // Empty registry = vacuously ready.
        assert!(report.is_ready());
    }

    #[tokio::test]
    async fn run_all_with_passing_check_reports_up() {
        let reg = HealthRegistry::new().with_check(std::sync::Arc::new(AlwaysOk));
        let report = reg.run_all().await;
        assert_eq!(report.checks.len(), 1);
        assert!(report.checks[0].ok);
        assert_eq!(report.checks[0].name, "always_ok");
        assert!(report.checks[0].error.is_none());
        assert!(report.is_ready());
    }

    #[tokio::test]
    async fn run_all_with_failing_check_reports_down() {
        let reg = HealthRegistry::new().with_check(std::sync::Arc::new(AlwaysFail));
        let report = reg.run_all().await;
        assert_eq!(report.checks.len(), 1);
        assert!(!report.checks[0].ok);
        assert_eq!(report.checks[0].name, "always_fail");
        assert_eq!(
            report.checks[0].error.as_deref(),
            Some("simulated outage")
        );
        assert!(!report.is_ready());
    }

    #[tokio::test]
    async fn run_all_reports_each_check_independently() {
        // Mixed: one up, one down -> overall not ready.
        let reg = HealthRegistry::new()
            .with_check(std::sync::Arc::new(AlwaysOk))
            .with_check(std::sync::Arc::new(AlwaysFail));
        let report = reg.run_all().await;
        assert_eq!(report.checks.len(), 2);
        assert!(report.checks[0].ok);
        assert!(!report.checks[1].ok);
        assert!(!report.is_ready());
    }
}
""",
    encoding="utf-8",
)
print("  crates/domain/src/health.rs")

# ---------- 3. Update domain/src/lib.rs: expose health module ----------
(ROOT / "crates" / "domain" / "src" / "lib.rs").write_text(
    """//! Domain layer
//!
//! Pure Rust: entities, value objects, business rules, and repository
//! **traits** (ports).
//!
//! **Dependency rule** (AGENTS.md § 6): this crate MUST NOT import
//! anything from the framework or DB world (no `axum`, no `tiberius`,
//! no `mongodb`). All IO is expressed through traits in this crate.

#![deny(unsafe_code)]
#![warn(missing_docs)]

pub mod health;

pub use health::{CheckOutcome, HealthCheck, HealthError, HealthRegistry, ReadyReport};
""",
    encoding="utf-8",
)
print("  crates/domain/src/lib.rs")

# ---------- 4. Create api/src/handlers/mod.rs ----------
(ROOT / "crates" / "api" / "src" / "handlers" / "mod.rs").write_text(
    """//! HTTP handlers grouped by concern.
//!
//! Each submodule owns one route family and exposes the axum handler
//! functions. The router in `main.rs` mounts them under their paths.

pub mod health;
""",
    encoding="utf-8",
)
print("  crates/api/src/handlers/mod.rs")

# ---------- 5. Create api/src/handlers/health.rs ----------
(ROOT / "crates" / "api" / "src" / "handlers" / "health.rs").write_text(
    """//! Liveness and readiness probes.
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

use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
use kokkak_common::response::ApiResponse;
use kokkak_domain::HealthRegistry;
use serde::Serialize;

/// `GET /healthz` — liveness probe.
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
            .oneshot(Request::builder().uri("/healthz").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn readyz_empty_registry_returns_200_with_empty_checks() {
        let app = app_with(vec![]);
        let resp = app
            .oneshot(Request::builder().uri("/readyz").body(Body::empty()).unwrap())
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
            .oneshot(Request::builder().uri("/readyz").body(Body::empty()).unwrap())
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
            .oneshot(Request::builder().uri("/readyz").body(Body::empty()).unwrap())
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
""",
    encoding="utf-8",
)
print("  crates/api/src/handlers/health.rs")

# ---------- 6. Update api/Cargo.toml: add domain ----------
(ROOT / "crates" / "api" / "Cargo.toml").write_text(
    """[package]
name = "kokkak-api"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
authors.workspace = true
description = "HTTP API server (axum) — entry point for web/admin/mobile clients"

[[bin]]
name = "kokkak-api"
path = "src/main.rs"

[dependencies]
# Core
tokio = { workspace = true }
axum = { workspace = true }
tower = { workspace = true }
tower-http = { workspace = true }
uuid = { workspace = true }
serde = { workspace = true }

# Telemetry
tracing = { workspace = true }
metrics = { workspace = true }

# Test helpers
async-trait = { workspace = true }

# Internal
kokkak-common = { path = "../common" }
kokkak-domain = { path = "../domain" }

[dev-dependencies]
# Test body / JSON inspection in handler tests.
serde_json = { workspace = true }
""",
    encoding="utf-8",
)
print("  crates/api/Cargo.toml")

# ---------- 7. Update api/src/main.rs: registry + /readyz + graceful shutdown ----------
(ROOT / "crates" / "api" / "src" / "main.rs").write_text(
    """//! Kokkeak API entry point.
//!
//! T01: `GET /healthz` returns 200 OK.
//! T02: load + validate `Settings` from env, fail fast on misconfig.
//! T03: tracing + Prometheus metrics, `GET /metrics`, per-request
//!      trace middleware (request id, latency).
//! T05: `GET /readyz` (readiness probe over [`HealthRegistry`]) +
//!      graceful shutdown on SIGTERM / Ctrl-C.

use std::sync::Arc;

use axum::{
    Router, body::Body,
    extract::State,
    http::{StatusCode, header::CONTENT_TYPE},
    response::{IntoResponse, Response},
    routing::get,
};
use kokkak_common::{config::Settings, telemetry};
use kokkak_domain::HealthRegistry;

mod handlers;
mod middleware;

/// T03: serve Prometheus text-format metrics.
async fn metrics_handler() -> impl IntoResponse {
    let body = telemetry::render_metrics();
    Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, "text/plain; version=0.0.4; charset=utf-8")
        .body(Body::from(body))
        .expect("failed to build metrics response")
}

#[tokio::main]
async fn main() {
    // ---- T02: load & validate configuration ----
    let settings = Settings::load().unwrap_or_else(|err| {
        eprintln!("[kokkak-api] invalid configuration: {err}");
        eprintln!("[kokkak-api] see .env.example for required variables");
        std::process::exit(1);
    });

    // ---- T03: init tracing (JSON or pretty) + Prometheus metrics ----
    telemetry::init_tracing(settings.log.format);
    let _metrics_handle = Arc::new(telemetry::init_metrics());

    tracing::info!(
        addr = %settings.server.addr,
        workers = settings.server.workers,
        log_format = ?settings.log.format,
        "kokkak-api starting"
    );

    // ---- T05: build readiness registry ----
    // M0: no concrete checks. M1+ will add SQL Server, Redis, NATS, Mongo
    // adapters implementing `kokkak_domain::HealthCheck`.
    let registry = HealthRegistry::new();
    if registry.is_empty() {
        tracing::warn!(
            "/readyz registered with zero health checks — \
             /readyz will report 200 unconditionally. \
             Wire real checks in M1 (T06-T09)."
        );
    }

    // ---- Routes ----
    let app = Router::new()
        .route("/healthz", get(handlers::health::healthz))
        .route("/readyz", get(handlers::health::readyz))
        .route("/metrics", get(metrics_handler))
        .layer(axum::middleware::from_fn(
            middleware::trace::trace_request,
        ))
        .with_state(registry);

    // ---- Bind + serve with graceful shutdown ----
    let listener = tokio::net::TcpListener::bind(&settings.server.addr)
        .await
        .unwrap_or_else(|err| {
            eprintln!(
                "[kokkak-api] failed to bind {}: {err}",
                settings.server.addr
            );
            std::process::exit(1);
        });

    tracing::info!(addr = %settings.server.addr, "kokkak-api listening");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .expect("server error");

    tracing::info!("kokkak-api exited cleanly");
}

/// Resolves on the first of: SIGINT (Ctrl-C), SIGTERM (Unix only).
///
/// Wired into [`axum::serve::Serve::with_graceful_shutdown`] so the
/// server stops accepting new connections, drains in-flight requests,
/// and returns. Process then exits via the normal `main` return path.
async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(err) = tokio::signal::ctrl_c().await {
            tracing::error!(error = %err, "failed to install Ctrl-C handler");
        }
        tracing::info!("SIGINT received, starting graceful shutdown");
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut sig) => {
                sig.recv().await;
                tracing::info!("SIGTERM received, starting graceful shutdown");
            }
            Err(err) => {
                tracing::error!(error = %err, "failed to install SIGTERM handler");
                std::future::pending::<()>().await;
            }
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {}
        _ = terminate => {}
    }
}

// Suppress unused-import warning on non-test builds.
#[allow(dead_code)]
fn _state_unused(_: State<HealthRegistry>) {}
""",
    encoding="utf-8",
)
print("  crates/api/src/main.rs")

# ---------- 8. Update README.md: mark T05 done ----------
readme = (ROOT / "README.md").read_text(encoding="utf-8")
readme = readme.replace(
    "| M0 Foundation | 🚧 in progress | T01–T05 |",
    "| M0 Foundation | ✅ done | T01–T05 |",
)
(ROOT / "README.md").write_text(readme, encoding="utf-8")
print("  README.md (T05 marked done)")

print("\n=== Files created/updated ===")
for p in sorted((ROOT / "crates").rglob("*")):
    if p.is_file() and "target" not in str(p):
        print(f"  {p.relative_to(ROOT)}")
