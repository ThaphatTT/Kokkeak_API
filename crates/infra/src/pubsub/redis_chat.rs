

use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use futures::StreamExt;
use kokkak_application::{
    BroadcastTransport, ChatEvent, ChatTransport, ChatUseCaseError as ChatError,
};
use redis::AsyncCommands;
use thiserror::Error;

const CHANNEL_PREFIX: &str = "chat:room:";

fn channel_for(room_id: uuid::Uuid) -> String {
    format!("{CHANNEL_PREFIX}{room_id}")
}

#[derive(Clone)]
pub struct RedisChatPubSub {
    local: Arc<BroadcastTransport>,

    pool: deadpool_redis::Pool,

    client: redis::Client,
}

impl RedisChatPubSub {

    pub fn new(
        local: Arc<BroadcastTransport>,
        pool: deadpool_redis::Pool,
        client: redis::Client,
    ) -> Self {
        Self {
            local,
            pool,
            client,
        }
    }

    pub fn start(self: &Arc<Self>) -> tokio::task::JoinHandle<()> {
        let me = self.clone();
        tokio::spawn(async move {
            loop {
                if let Err(e) = me.run_subscriber().await {
                    tracing::warn!(error = %e, "chat pub/sub subscriber exited; retrying in 5s");
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                }
            }
        })
    }

    async fn run_subscriber(&self) -> Result<(), RedisChatError> {

        #[allow(deprecated)]
        let conn = self.client.get_async_connection().await?;
        let mut pubsub = conn.into_pubsub();

        pubsub.psubscribe("chat:room:*").await?;
        tracing::info!("chat pub/sub bridge online (psubscribe chat:room:*)");
        let mut stream = pubsub.on_message();
        while let Some(msg) = stream.next().await {
            let channel: String = msg.get_channel_name().to_string();
            let payload: Bytes = Bytes::from(msg.get_payload_bytes().to_vec());

            if let Some(room_str) = channel.strip_prefix(CHANNEL_PREFIX) {
                if let Ok(room_id) = room_str.parse::<uuid::Uuid>() {
                    if let Ok(env) = serde_json::from_slice::<WireChatEvent>(&payload) {
                        let _ = self.local.tx.send(ChatEvent {
                            room_id,
                            message: env.into_message(),
                        });
                    }
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct WireChatEvent {
    id: uuid::Uuid,
    room_id: uuid::Uuid,
    sender_id: uuid::Uuid,
    body: String,
    sent_at_ms: i64,
    read_by: Vec<(uuid::Uuid, i64)>,
}

impl WireChatEvent {
    fn from_message(m: &kokkak_domain::ChatMessage) -> Self {
        Self {
            id: m.id,
            room_id: m.room_id,
            sender_id: m.sender_id,
            body: m.body.clone(),
            sent_at_ms: m.sent_at.timestamp_millis(),
            read_by: m
                .read_by
                .iter()
                .map(|(u, t)| (*u, t.timestamp_millis()))
                .collect(),
        }
    }

    fn into_message(self) -> kokkak_domain::ChatMessage {
        use chrono::TimeZone;
        let sent_at = chrono::Utc
            .timestamp_millis_opt(self.sent_at_ms)
            .single()
            .unwrap_or_else(chrono::Utc::now);
        let read_by = self
            .read_by
            .into_iter()
            .filter_map(|(u, ms)| {
                chrono::Utc
                    .timestamp_millis_opt(ms)
                    .single()
                    .map(|t| (u, t))
            })
            .collect();
        kokkak_domain::ChatMessage {
            id: self.id,
            room_id: self.room_id,
            sender_id: self.sender_id,
            body: self.body,
            sent_at,
            read_by,
        }
    }
}

#[async_trait]
impl ChatTransport for RedisChatPubSub {
    async fn broadcast_message(&self, event: ChatEvent) -> Result<(), ChatError> {

        let _ = self.local.broadcast_message(event.clone()).await;

        let wire = WireChatEvent::from_message(&event.message);
        let payload = match serde_json::to_vec(&wire) {
            Ok(b) => b,
            Err(e) => return Err(ChatError::Backend(format!("redis chat encode failed: {e}"))),
        };
        let channel = channel_for(event.room_id);
        let mut conn = self
            .pool
            .get()
            .await
            .map_err(|e| ChatError::Backend(format!("redis pool: {e}")))?;
        let _: i64 = conn
            .publish(channel, payload)
            .await
            .map_err(|e| ChatError::Backend(format!("redis publish: {e}")))?;
        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum RedisChatError {

    #[error("redis error: {0}")]
    Redis(#[from] redis::RedisError),

    #[error("redis pool: {0}")]
    Pool(#[from] deadpool_redis::PoolError),

    #[error("codec error: {0}")]
    Codec(String),
}
