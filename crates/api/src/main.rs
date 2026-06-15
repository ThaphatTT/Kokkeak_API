//! Kokkeak API entry point.
//!
//! T01 (M0): minimal — only `GET /healthz` returns 200 OK.
//! Full HTTP routing, middleware, and graceful shutdown are added in T05+.

use axum::{Router, http::StatusCode, response::IntoResponse, routing::get};

#[tokio::main]
async fn main() {
    let app = Router::new().route("/healthz", get(healthz));

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000")
        .await
        .expect("failed to bind 0.0.0.0:3000");

    println!(
        "kokkak-api listening on http://{}",
        listener.local_addr().expect("local_addr")
    );

    axum::serve(listener, app)
        .await
        .expect("server error");
}

async fn healthz() -> impl IntoResponse {
    (StatusCode::OK, "OK")
}
