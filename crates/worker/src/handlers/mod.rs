

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

    #[error("handler failed: {0}")]
    Failed(String),
}

#[async_trait]
pub trait Handler: Send + Sync {

    fn subject(&self) -> &str;

    async fn handle(&self, message_id: &str, payload: &[u8]) -> Result<(), HandlerError>;
}

#[derive(Clone)]
pub struct HandlerContext {

    pub idempotency: Arc<dyn Idempotency>,
}
