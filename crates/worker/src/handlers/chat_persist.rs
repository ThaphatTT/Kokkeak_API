

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use kokkak_domain::{ChatMessage, ChatRepository};
use tracing::{error, info, warn};

use super::{Handler, HandlerContext, HandlerError};

static CHAT_REPO: tokio::sync::OnceCell<Arc<dyn ChatRepository>> =
    tokio::sync::OnceCell::const_new();

pub fn set_chat_repo(repo: Arc<dyn ChatRepository>) {
    if CHAT_REPO.set(repo).is_err() {
        warn!("chat persist handler: repo already set; ignoring second set");
    }
}

pub struct ChatPersistHandler {
    #[allow(dead_code)]
    ctx: HandlerContext,
}

impl ChatPersistHandler {

    pub fn new(ctx: HandlerContext) -> Self {
        Self { ctx }
    }
}

#[async_trait]
impl Handler for ChatPersistHandler {
    fn subject(&self) -> &str {
        "chat.persist"
    }

    async fn handle(&self, message_id: &str, payload: &[u8]) -> Result<(), HandlerError> {
        let Some(repo) = CHAT_REPO.get() else {
            warn!(
                message_id,
                "chat.persist: no chat repo installed; dropping message"
            );
            return Ok(());
        };
        let body = std::str::from_utf8(payload)
            .map_err(|e| HandlerError::Failed(format!("non-utf8 payload: {e}")))?;
        let parsed: WireChatMessage = match serde_json::from_str(body) {
            Ok(m) => m,
            Err(e) => {
                error!(message_id, error = %e, "chat.persist: invalid JSON");
                return Err(HandlerError::Failed(format!("invalid json: {e}")));
            }
        };
        let msg = match parsed.into_message() {
            Ok(m) => m,
            Err(e) => {
                error!(message_id, error = %e, "chat.persist: bad payload");
                return Err(HandlerError::Failed(e));
            }
        };
        match repo.insert_message(&msg).await {
            Ok(()) => {
                info!(
                    message_id,
                    room_id = %msg.room_id,
                    "chat.persist: ok"
                );
                Ok(())
            }
            Err(e) => {
                error!(message_id, error = %e, "chat.persist: repo failed");
                Err(HandlerError::Failed(e.to_string()))
            }
        }
    }
}

#[derive(Debug, serde::Deserialize)]
struct WireChatMessage {
    id: uuid::Uuid,
    room_id: uuid::Uuid,
    sender_id: uuid::Uuid,
    body: String,
    sent_at: DateTime<Utc>,
    #[serde(default)]
    read_by: Vec<(uuid::Uuid, DateTime<Utc>)>,
}

impl WireChatMessage {
    fn into_message(self) -> Result<ChatMessage, String> {
        if self.body.trim().is_empty() {
            return Err("empty body".into());
        }
        Ok(ChatMessage {
            id: self.id,
            room_id: self.room_id,
            sender_id: self.sender_id,
            body: self.body,
            sent_at: self.sent_at,
            read_by: self.read_by,
        })
    }
}
