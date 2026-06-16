//! Worker handlers — one per NATS subject (M4).

use std::sync::Arc;

use async_trait::async_trait;
use thiserror::Error;

use crate::idempotency::Idempotency;

pub mod chat_persist;
pub mod comm_email;
pub mod noti_push;
pub mod order_dispatch;
pub mod points_recalc;

pub use chat_persist::ChatPersistHandler;
pub use comm_email::CommEmailHandler;
pub use noti_push::NotiPushHandler;
pub use order_dispatch::OrderDispatchHandler;
pub use points_recalc::PointsRecalcHandler;

#[derive(Debug, Error)]
pub enum HandlerError {
    /// Generic handler failure (DB / external / parse).
    #[error("handler failed: {0}")]
    Failed(String),
}

/// Port every handler implements.
#[async_trait]
pub trait Handler: Send + Sync {
    /// Subject name this handler is bound to (e.g. `"noti.push"`).
    fn subject(&self) -> &str;
    /// Process the raw payload. **Must be idempotent** — the runner
    /// has already checked the idempotency cache, but defensive
    /// re-checks are welcome.
    async fn handle(&self, message_id: &str, payload: &[u8]) -> Result<(), HandlerError>;
}

/// Shared state handed to every handler.
#[derive(Clone)]
pub struct HandlerContext {
    pub idempotency: Arc<dyn Idempotency>,
}
