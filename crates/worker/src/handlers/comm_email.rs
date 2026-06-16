//! `comm.email` handler — SMTP delivery (M4).
//!
//! Stub: logs the payload. Production wires the SMTP client in M6+.

use async_trait::async_trait;
use tracing::info;

use super::{Handler, HandlerContext, HandlerError};

pub struct CommEmailHandler {
    #[allow(dead_code)]
    ctx: HandlerContext,
}

impl CommEmailHandler {
    pub fn new(ctx: HandlerContext) -> Self {
        Self { ctx }
    }
}

#[async_trait]
impl Handler for CommEmailHandler {
    fn subject(&self) -> &str {
        "comm.email"
    }

    async fn handle(&self, message_id: &str, payload: &[u8]) -> Result<(), HandlerError> {
        let body = std::str::from_utf8(payload)
            .map_err(|e| HandlerError::Failed(format!("non-utf8 payload: {e}")))?;
        info!(message_id, body = body, "comm.email (stub) — would call SMTP");
        Ok(())
    }
}
