//! `order.dispatch` handler — fan-out to candidate technicians (M4).
//!
//! Production: pre-filter candidates by skill + proximity
//! (AGENTS.md § 20.1), then publish per-candidate notifications on
//! `noti.push` (which the FCM handler picks up). For M4 we ship a
//! log-only stub.

use async_trait::async_trait;
use tracing::info;

use super::{Handler, HandlerContext, HandlerError};

pub struct OrderDispatchHandler {
    #[allow(dead_code)]
    ctx: HandlerContext,
}

impl OrderDispatchHandler {
    pub fn new(ctx: HandlerContext) -> Self {
        Self { ctx }
    }
}

#[async_trait]
impl Handler for OrderDispatchHandler {
    fn subject(&self) -> &str {
        "order.dispatch"
    }

    async fn handle(&self, message_id: &str, payload: &[u8]) -> Result<(), HandlerError> {
        let body = std::str::from_utf8(payload)
            .map_err(|e| HandlerError::Failed(format!("non-utf8 payload: {e}")))?;
        info!(message_id, body = body, "order.dispatch (stub) — would match + fan-out");
        Ok(())
    }
}
