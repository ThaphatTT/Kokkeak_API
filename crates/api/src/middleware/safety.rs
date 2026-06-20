//! T-16: request safety middleware (concurrency cap).
//!
//! `tower::load_shed::LoadShedLayer` returns `BoxError` on overload,
//! which axum's `Router::layer` cannot accept (it requires
//! `Into<Infallible>`). Rather than wrap `LoadShed` with a custom
//! error mapper, we implement the cap directly with
//! [`tokio::sync::Semaphore::try_acquire_owned`] — same semantics
//! (fail fast at the cap), no error type friction, no extra
//! dependency.
//!
//! ## Behaviour
//!
//! - Each in-flight request holds one permit.
//! - When the cap is reached, the next request is shed
//!   immediately with HTTP 503 (`Service Unavailable`).
//! - The permit is held for the duration of the inner service
//!   call and dropped when the response is returned.
//!
//! ## Layer placement
//!
//! Wired in `main.rs` OUTSIDE the timeout/compression/cors
//! layers so a shed request does not pay any per-request
//! middleware cost.

use std::sync::Arc;

use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
};
use tokio::sync::Semaphore;

/// Wrapper that carries the shared semaphore through `from_fn_with_state`.
#[derive(Clone)]
pub struct ConcurrencyCap(Arc<Semaphore>);

impl ConcurrencyCap {
    /// Build a cap that allows at most `max` in-flight requests.
    pub fn new(max: usize) -> Self {
        Self(Arc::new(Semaphore::new(max)))
    }
}

/// T-16 middleware: shed the request with 503 if the cap is
/// reached, otherwise run the inner service to completion.
pub async fn concurrency_cap(
    State(ConcurrencyCap(sem)): State<ConcurrencyCap>,
    req: Request,
    next: Next,
) -> Response {
    match sem.clone().try_acquire_owned() {
        Ok(permit) => {
            // Permit lives until the response is fully built;
            // dropping it here releases the slot for the next
            // request.
            let resp = next.run(req).await;
            drop(permit);
            resp
        }
        Err(_) => (
            StatusCode::SERVICE_UNAVAILABLE,
            axum::Json(serde_json::json!({
                "success": false,
                "error": {
                    "code": "overloaded",
                    "message": "server is at capacity, retry shortly"
                }
            })),
        )
            .into_response(),
    }
}
