"""Create M0 T03 file structure: telemetry + HTTP trace middleware."""

import os
from pathlib import Path

ROOT = Path(r"C:\Users\crybo\Desktop\Develop\Kokkeak_API")

# ---------- 1. Ensure new directories exist ----------
(ROOT / "crates" / "common" / "src").mkdir(parents=True, exist_ok=True)
(ROOT / "crates" / "api" / "src" / "middleware").mkdir(parents=True, exist_ok=True)
(ROOT / "crates" / "api" / "src" / "handlers").mkdir(parents=True, exist_ok=True)
print("Created directories")

# ---------- 2. Update common/Cargo.toml: add tracing + metrics deps ----------
(ROOT / "crates" / "common" / "Cargo.toml").write_text(
    """[package]
name = "kokkak-common"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
authors.workspace = true
description = "Common utilities: error, config, telemetry, response envelope"

[dependencies]
serde = { workspace = true }
figment = { workspace = true }
thiserror = { workspace = true }

# Telemetry
tracing = { workspace = true }
tracing-subscriber = { workspace = true, features = ["env-filter", "json"] }
metrics = { workspace = true }
metrics-exporter-prometheus = { workspace = true }

[dev-dependencies]
serde_json = { workspace = true }
""",
    encoding="utf-8",
)
print("  crates/common/Cargo.toml")

# ---------- 3. Update common/src/lib.rs: add telemetry mod ----------
(ROOT / "crates" / "common" / "src" / "lib.rs").write_text(
    """//! Common layer
//!
//! Houses shared infrastructure used by every other crate:
//! error types, configuration loader, telemetry, response envelope,
//! and small utilities (UUID v7, time, decimal).
//!
//! See AGENTS.md § 3, 11, 12, 14 for the standards this layer enforces.

#![deny(unsafe_code)]
#![warn(missing_docs)]

pub mod config;
pub mod telemetry;

pub use config::{ConfigError, LogFormat, LogSettings, ServerSettings, Settings};
""",
    encoding="utf-8",
)
print("  crates/common/src/lib.rs")

# ---------- 4. Create common/src/telemetry.rs ----------
(ROOT / "crates" / "common" / "src" / "telemetry.rs").write_text(
    """//! Telemetry: tracing (structured log) + Prometheus metrics.
//!
//! Initialise once at startup (idempotent):
//!
//! ```no_run
//! use kokkak_common::config::Settings;
//! use kokkak_common::telemetry;
//!
//! let settings = Settings::load().expect("invalid config");
//! telemetry::init_tracing(settings.log.format);
//! let _handle = telemetry::init_metrics();
//! ```
//!
//! Then expose `/metrics` with [`render_metrics`].

use std::sync::OnceLock;

use metrics_exporter_prometheus::PrometheusHandle;
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

use crate::config::LogFormat;

/// Lazily-initialised handle to the Prometheus recorder.
///
/// Calling [`init_metrics`] more than once returns the same handle; no
/// second recorder is installed (the global recorder is a singleton).
static METRICS_HANDLE: OnceLock<PrometheusHandle> = OnceLock::new();

/// Initialise the global tracing subscriber.
///
/// Honours `RUST_LOG` for the filter (default: `info,kokkak_api=debug`).
/// Format is chosen by `format` (JSON for prod, pretty for dev).
///
/// Idempotent: if a subscriber is already installed, this is a no-op
/// (prints a notice to stderr).
pub fn init_tracing(format: LogFormat) {
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,kokkak_api=debug"));

    let result = match format {
        LogFormat::Json => tracing_subscriber::registry()
            .with(env_filter)
            .with(fmt::layer().json().flatten_event(true))
            .try_init(),
        LogFormat::Pretty => tracing_subscriber::registry()
            .with(env_filter)
            .with(fmt::layer().pretty().with_target(true))
            .try_init(),
    };

    if result.is_err() {
        eprintln!("[kokkak-common] tracing subscriber already initialised (this is OK if called twice)");
    }
}

/// Initialise the Prometheus metrics recorder and return its handle.
///
/// Idempotent: subsequent calls return the same handle without
/// trying to install a second recorder.
pub fn init_metrics() -> &'static PrometheusHandle {
    METRICS_HANDLE.get_or_init(|| {
        metrics_exporter_prometheus::PrometheusBuilder::new()
            .set_buckets_for_metric(
                metrics_exporter_prometheus::Matcher::Full,
                &[0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0],
            )
            .expect("valid histogram buckets")
            .install_recorder()
            .expect("install Prometheus recorder (is another recorder already installed?)")
    })
}

/// Render Prometheus text-format metrics (for `GET /metrics`).
pub fn render_metrics() -> String {
    init_metrics().render()
}

#[cfg(test)]
mod tests {
    use super::*;
    use metrics::{counter, histogram};

    #[test]
    fn init_metrics_returns_static_handle() {
        let h1 = init_metrics();
        let h2 = init_metrics();
        assert!(
            std::ptr::eq(h1, h2),
            "init_metrics must return the same static handle on every call"
        );
    }

    #[test]
    fn render_metrics_returns_prometheus_text() {
        // Record some metrics so render has something to show
        counter!("kokkak_test_counter_total").increment(7);
        histogram!("kokkak_test_histogram_seconds").record(0.123);

        let text = render_metrics();

        // Prometheus text format starts with `# HELP` and `# TYPE` lines
        assert!(
            text.contains("# HELP") || text.contains("# TYPE"),
            "rendered metrics should be in Prometheus text format"
        );
        assert!(
            text.contains("kokkak_test_counter_total"),
            "rendered metrics should include our counter"
        );
    }
}
""",
    encoding="utf-8",
)
print("  crates/common/src/telemetry.rs")

# ---------- 5. Update api/Cargo.toml: add tracing + tower-http deps ----------
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

# Telemetry
tracing = { workspace = true }
metrics = { workspace = true }

# Internal
kokkak-common = { path = "../common" }
""",
    encoding="utf-8",
)
print("  crates/api/Cargo.toml")

# ---------- 6. Create api/src/middleware/trace.rs ----------
(ROOT / "crates" / "api" / "src" / "middleware" / "trace.rs").write_text(
    """//! HTTP middleware: per-request tracing + Prometheus metrics.
//!
//! Responsibilities:
//! 1. Generate a request id (UUID v7 — time-sortable) per request.
//! 2. Inject it into request extensions (handlers can read via `Extension<RequestId>`).
//! 3. Add it to the response as the `x-request-id` header.
//! 4. Log request start / complete with method, path, status, latency.
//! 5. Increment `http_requests_total` counter and record duration histogram.

use std::time::Instant;

use axum::{
    extract::Request,
    http::{HeaderValue, header::HeaderName},
    middleware::Next,
    response::Response,
};
use uuid::Uuid;

/// Header name for the request id (`x-request-id`).
pub const X_REQUEST_ID: HeaderName = HeaderName::from_static("x-request-id");

/// Extracted request id (use `Extension<RequestId>` in handlers).
#[derive(Debug, Clone, Copy)]
pub struct RequestId(pub Uuid);

/// Per-request tracing + metrics middleware.
pub async fn trace_request(mut req: Request, next: Next) -> Response {
    let request_id = Uuid::now_v7();
    let method = req.method().clone();
    let path = req.uri().path().to_string();
    let start = Instant::now();

    // Make the request id available to downstream handlers
    req.extensions_mut().insert(RequestId(request_id));

    tracing::info!(
        request_id = %request_id,
        method = %method,
        path = %path,
        "request started"
    );

    let mut response = next.run(req).await;
    let status = response.status().as_u16();
    let latency = start.elapsed();

    tracing::info!(
        request_id = %request_id,
        method = %method,
        path = %path,
        status,
        latency_ms = latency.as_millis() as u64,
        "request completed"
    );

    // Record metrics (use display labels to keep cardinality bounded)
    metrics::counter!(
        "http_requests_total",
        "method" => method.as_str().to_string(),
        "path" => path.clone(),
        "status" => status.to_string(),
    )
    .increment(1);
    metrics::histogram!(
        "http_request_duration_seconds",
        "method" => method.as_str().to_string(),
        "path" => path.clone(),
    )
    .record(latency.as_secs_f64());

    // Stamp the response so clients can correlate with logs
    if let Ok(val) = HeaderValue::from_str(&request_id.to_string()) {
        response.headers_mut().insert(X_REQUEST_ID, val);
    }

    response
}
""",
    encoding="utf-8",
)
print("  crates/api/src/middleware/trace.rs")

# ---------- 7. Create api/src/middleware/mod.rs ----------
(ROOT / "crates" / "api" / "src" / "middleware" / "mod.rs").write_text(
    """//! HTTP middleware for the API server.

pub mod trace;
""",
    encoding="utf-8",
)
print("  crates/api/src/middleware/mod.rs")

# ---------- 8. Update api/src/main.rs: init telemetry + add /metrics + trace middleware ----------
(ROOT / "crates" / "api" / "src" / "main.rs").write_text(
    """//! Kokkeak API entry point.
//!
//! T01: `GET /healthz` returns 200 OK.
//! T02: load + validate `Settings` from env, fail fast on misconfig.
//! T03: tracing + Prometheus metrics, `GET /metrics`, per-request
//!      trace middleware (request id, latency).

use std::sync::Arc;

use axum::{
    Router, body::Body,
    extract::Request,
    http::{StatusCode, header::HeaderValue, header::CONTENT_TYPE},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
};
use kokkak_common::{config::Settings, telemetry};

mod middleware;

/// T03: extract the raw text body for /metrics (Prometheus text format)
async fn metrics_handler() -> impl IntoResponse {
    let body = telemetry::render_metrics();
    Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, "text/plain; version=0.0.4; charset=utf-8")
        .body(Body::from(body))
        .unwrap()
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

    // ---- Routes ----
    let app = Router::new()
        .route("/healthz", get(healthz))
        .route("/metrics", get(metrics_handler))
        .layer(middleware::from_fn(middleware::trace::trace_request));

    // ---- Bind + serve ----
    let listener = tokio::net::TcpListener::bind(&settings.server.addr)
        .await
        .unwrap_or_else(|err| {
            eprintln!("[kokkak-api] failed to bind {}: {err}", settings.server.addr);
            std::process::exit(1);
        });

    tracing::info!(addr = %settings.server.addr, "kokkak-api listening");
    axum::serve(listener, app).await.expect("server error");
}

async fn healthz() -> impl IntoResponse {
    (StatusCode::OK, "OK")
}

// Suppress unused import warning for axum internals
#[allow(dead_code)]
fn _axum_imports_used(_req: Request, _next: Next) -> Response {
    (StatusCode::OK, "OK").into_response()
}
""",
    encoding="utf-8",
)
print("  crates/api/src/main.rs")

print("\n=== Files created/updated ===")
for p in sorted((ROOT / "crates").rglob("*")):
    if p.is_file() and "target" not in str(p):
        rel = p.relative_to(ROOT)
        print(f"  {rel}")
