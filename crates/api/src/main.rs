//! Kokkeak API entry point.
//!
//! T01: minimal — only `GET /healthz` returns 200 OK.
//! T02: load + validate `Settings` from env, fail fast on misconfig.

use axum::{Router, http::StatusCode, response::IntoResponse, routing::get};
use kokkak_common::config::Settings;

#[tokio::main]
async fn main() {
    // T02: load & validate configuration (fail-fast on misconfig)
    let settings = Settings::load().unwrap_or_else(|err| {
        eprintln!("[kokkak-api] invalid configuration: {err}");
        eprintln!("[kokkak-api] see .env.example for required variables");
        std::process::exit(1);
    });

    println!("[kokkak-api] config loaded: server.addr={} workers={} log.format={:?}",
        settings.server.addr, settings.server.workers, settings.log.format);

    let app = Router::new().route("/healthz", get(healthz));

    let listener = tokio::net::TcpListener::bind(&settings.server.addr)
        .await
        .unwrap_or_else(|err| {
            eprintln!("[kokkak-api] failed to bind {}: {err}", settings.server.addr);
            std::process::exit(1);
        });

    println!("[kokkak-api] listening on http://{}", settings.server.addr);
    axum::serve(listener, app).await.expect("server error");
}

async fn healthz() -> impl IntoResponse {
    (StatusCode::OK, "OK")
}
