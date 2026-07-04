

use std::sync::Arc;

use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
};
use tokio::sync::Semaphore;

#[derive(Clone)]
pub struct ConcurrencyCap(Arc<Semaphore>);

impl ConcurrencyCap {

    pub fn new(max: usize) -> Self {
        Self(Arc::new(Semaphore::new(max)))
    }
}

pub async fn concurrency_cap(
    State(ConcurrencyCap(sem)): State<ConcurrencyCap>,
    req: Request,
    next: Next,
) -> Response {
    match sem.clone().try_acquire_owned() {
        Ok(permit) => {

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
