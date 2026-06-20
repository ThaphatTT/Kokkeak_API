//! Cross-instance chat pub/sub via Redis (M8).
//!
//! The flow is:
//
//! ```text
//! ChatService::send_message
//!        │
//!        ├─► repo.insert_message   (Mongo / JSON-DB)
//!        │
//!        └─► transport.broadcast_message
//!                  │
//!                  ▼
//!            RedisChatPubSub
//!                  │  ┌─── local tokio::broadcast ──► WebSocket subscribers (this instance)
//!                  │  │
//!                  └──┴── Redis PUBLISH chat:room:{id} ──► peer instances re-broadcast locally
//! ```
//!
//! Peer-instance reception happens on a dedicated long-lived
//! Redis connection that is `spawn`-ed by [`Self::start`] and
//! drives the local `tokio::broadcast::Sender`.

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

/// Wraps the local [`BroadcastTransport`] and adds a
/// Redis pub/sub bridge. Constructed by `api::main` when
/// Redis is configured.
#[derive(Clone)]
pub struct RedisChatPubSub {
    local: Arc<BroadcastTransport>,
    /// Pool used for the publisher side (cheap, multiplexed).
    pool: deadpool_redis::Pool,
    /// Dedicated client for the subscriber side (psubscribe
    /// needs a non-multiplexed connection).
    client: redis::Client,
}

impl RedisChatPubSub {
    /// Build a new pub/sub bridge. `local` is the underlying
    /// in-process transport; `pool` is the shared Redis
    /// connection pool (used for PUBLISH); `client` is a
    /// dedicated `redis::Client` for the long-lived subscriber.
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

    /// Start the long-lived subscriber task. The task lives
    /// for the entire process lifetime; the caller can cancel
    /// it by dropping the returned `JoinHandle` (which the
    /// process does on shutdown).
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
        // Dedicated non-multiplexed connection for psubscribe.
        // The pool's MultiplexedConnection cannot drive a
        // pubsub stream; we open a fresh async connection
        // here from the dedicated client.
        // `get_async_connection` is deprecated in favour of the
        // multiplexed variant, but the multiplexed connection
        // cannot drive a pubsub stream — so we pin the deprecated
        // API here intentionally.
        #[allow(deprecated)]
        let conn = self.client.get_async_connection().await?;
        let mut pubsub = conn.into_pubsub();
        // Subscribe to every chat:* channel via PSUBSCRIBE so
        // we get one connection for the whole fleet.
        pubsub.psubscribe("chat:room:*").await?;
        tracing::info!("chat pub/sub bridge online (psubscribe chat:room:*)");
        let mut stream = pubsub.on_message();
        while let Some(msg) = stream.next().await {
            let channel: String = msg.get_channel_name().to_string();
            let payload: Bytes = Bytes::from(msg.get_payload_bytes().to_vec());
            // The local transport only needs the JSON; the
            // room_id is in the channel name. We re-derive it.
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
        // 1. Local fan-out (in-process WebSocket subscribers).
        let _ = self.local.broadcast_message(event.clone()).await;
        // 2. Cross-instance fan-out via Redis.
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

/// Errors raised by the pub/sub bridge.
#[derive(Debug, Error)]
pub enum RedisChatError {
    /// Underlying Redis error.
    #[error("redis error: {0}")]
    Redis(#[from] redis::RedisError),
    /// Pool exhaustion.
    #[error("redis pool: {0}")]
    Pool(#[from] deadpool_redis::PoolError),
    /// Codec error.
    #[error("codec error: {0}")]
    Codec(String),
}
