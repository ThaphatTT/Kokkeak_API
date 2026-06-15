//! Kokkeak API entry point.
//!
//! T01: `GET /healthz` returns 200 OK.
//! T02: load + validate `Settings` from env, fail fast on misconfig.
//! T03: tracing + Prometheus metrics, `GET /metrics`, per-request
//!      trace middleware (request id, latency).

use std::sync::Arc;

use axum::{
    Router, body::Body,
    http::{StatusCode, header::CONTENT_TYPE},
    response::{IntoResponse, Response},
    routing::get,
};
use kokkak_common::{config::Settings, telemetry};

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

    // ---- Routes ----
    let app = Router::new()
        .route("/healthz", get(healthz))
        .route("/metrics", get(metrics_handler))
        .layer(axum::middleware::from_fn(
            middleware::trace::trace_request,
        ));

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
