//! `noti.push` handler — FCM push delivery (M4).
//!
//! Production: forward `payload` to the Firebase Cloud Messaging HTTP
//! v1 API. M4 ships a log-only stub — the FCM HTTP client lands in
//! M6+ alongside S3 / external integrations.

use async_trait::async_trait;
use tracing::info;

use super::{Handler, HandlerContext, HandlerError};

/// Stub FCM push handler.
pub struct NotiPushHandler {
    #[allow(dead_code)]
    ctx: HandlerContext,
}

impl NotiPushHandler {
    pub fn new(ctx: HandlerContext) -> Self {
        Self { ctx }
    }
}

#[async_trait]
impl Handler for NotiPushHandler {
    fn subject(&self) -> &str {
        "noti.push"
    }

    async fn handle(&self, message_id: &str, payload: &[u8]) -> Result<(), HandlerError> {
        let body = std::str::from_utf8(payload)
            .map_err(|e| HandlerError::Failed(format!("non-utf8 payload: {e}")))?;
        info!(message_id, body = body, "noti.push (stub) — would call FCM");
        Ok(())
    }
}
