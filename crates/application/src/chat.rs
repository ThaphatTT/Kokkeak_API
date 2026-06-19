//! Chat use cases (M8).
//!
//! Pure orchestration:
//!
//! 1. `open_room` — dedup the 1:1 case (find-or-create).
//! 2. `send_message` — validate, persist (idempotent on
//!    `MessageId`), then publish on Redis pub/sub so every API
//!    instance can fan it out to its WebSocket subscribers.
//! 3. `list_rooms` / `list_messages` — read-side projections
//!    that call straight into the repository.
//! 4. `mark_read` — append a receipt.
//!
//! The transport port ([`ChatTransport`]) is intentionally
//! minimal: only `broadcast_message`. Anything richer (typing
//! indicators, presence) lands in M12+.

use std::sync::Arc;

use chrono::Utc;
use kokkak_domain::{
    ChatError, ChatMembership, ChatMessage, ChatRepoError, ChatRepository, ChatRoom, MessageId,
    MessagePage, Participant, RoomId, RoomSummary, User,
};
use tokio::sync::broadcast;
use uuid::Uuid;

const MAX_BODY_LEN: usize = 4_000;

/// Broadcast payload sent on the local `tokio::sync::broadcast`
/// channel (one per process). The Redis pub/sub adapter
/// re-broadcasts the same payload across instances.
#[derive(Debug, Clone)]
pub struct ChatEvent {
    /// Room the event belongs to.
    pub room_id: RoomId,
    /// The new message.
    pub message: ChatMessage,
}

/// Local + cross-instance fan-out for chat events.
///
/// `ChatService` calls [`Self::broadcast_message`] **after** a
/// successful `insert_message` so WebSocket subscribers see the
/// message without polling. The infra layer implements this port
/// using Redis pub/sub (cross-instance) and a local
/// `tokio::sync::broadcast` (in-process).
#[async_trait::async_trait]
pub trait ChatTransport: Send + Sync {
    /// Broadcast a newly persisted message to every interested
    /// subscriber (local + remote). Returns immediately;
    /// subscribers consume the event asynchronously.
    async fn broadcast_message(&self, event: ChatEvent) -> Result<(), ChatError>;
}

/// In-process + Redis-pubsub transport (production adapter).
pub struct BroadcastTransport {
    /// Public so the `infra::pubsub` adapter can publish
    /// events it received from Redis into the local channel.
    pub tx: broadcast::Sender<ChatEvent>,
}

impl BroadcastTransport {
    /// Build a new transport with a bounded in-process channel.
    pub fn new(capacity: usize) -> Self {
        let (tx, _rx) = broadcast::channel(capacity);
        Self { tx }
    }

    /// Subscribe to the local event stream. The
    /// `WebSocketHandler` calls this once per connection.
    pub fn subscribe(&self) -> broadcast::Receiver<ChatEvent> {
        self.tx.subscribe()
    }
}

#[async_trait::async_trait]
impl ChatTransport for BroadcastTransport {
    async fn broadcast_message(&self, event: ChatEvent) -> Result<(), ChatError> {
        // Best-effort: ignore the "no subscribers" error.
        let _ = self.tx.send(event);
        Ok(())
    }
}

impl Default for BroadcastTransport {
    fn default() -> Self {
        Self::new(256)
    }
}

/// Chat use case service (one action = one method).
pub struct ChatService {
    repo: Arc<dyn ChatRepository>,
    transport: Arc<dyn ChatTransport>,
}

impl ChatService {
    /// Build a service. `transport` is required for live
    /// WebSocket fan-out; tests can pass a no-op.
    pub fn new(repo: Arc<dyn ChatRepository>, transport: Arc<dyn ChatTransport>) -> Self {
        Self { repo, transport }
    }

    /// Borrow the underlying repository (used by the
    /// WebSocket gateway for membership checks).
    pub fn repo(&self) -> &Arc<dyn ChatRepository> {
        &self.repo
    }

    /// Open a room with exactly the given participants
    /// (sorted by id, set semantics). When a matching room
    /// already exists, return it instead of creating a
    /// duplicate.
    pub async fn open_room(
        &self,
        mut participants: Vec<Participant>,
    ) -> Result<ChatRoom, ChatError> {
        if participants.len() < 2 {
            return Err(ChatError::InvalidBody(
                "room needs at least 2 participants".into(),
            ));
        }
        // Sort for set semantics.
        participants.sort_by_key(|p| p.user_id);
        let ids: Vec<Uuid> = participants.iter().map(|p| p.user_id).collect();
        if let Some(room) = self
            .repo
            .find_room_by_participants(&ids)
            .await
            .map_err(|e| ChatError::Backend(e.to_string()))?
        {
            return Ok(room);
        }
        let now = Utc::now();
        let room = ChatRoom {
            id: Uuid::new_v4(),
            participants,
            created_at: now,
            last_msg_at: now,
            title: None,
        };
        self.repo
            .create_room(&room)
            .await
            .map_err(|e| ChatError::Backend(e.to_string()))?;
        Ok(room)
    }

    /// Send a message. The `MessageId` is generated server-side
    /// so the worker's `chat.persist` queue can re-deliver
    /// safely (idempotent insert).
    pub async fn send_message(
        &self,
        room_id: RoomId,
        sender_id: Uuid,
        body: String,
    ) -> Result<ChatMessage, ChatError> {
        if body.trim().is_empty() {
            return Err(ChatError::InvalidBody("empty body".into()));
        }
        if body.len() > MAX_BODY_LEN {
            return Err(ChatError::InvalidBody(format!(
                "body too long (max {MAX_BODY_LEN} bytes)"
            )));
        }
        let room = self
            .repo
            .find_room(room_id)
            .await
            .map_err(|e| ChatError::Backend(e.to_string()))?
            .ok_or(ChatError::RoomNotFound(room_id))?;
        if !room.contains(sender_id) {
            return Err(ChatError::NotParticipant(room_id));
        }
        let now = Utc::now();
        let msg = ChatMessage {
            id: Uuid::new_v4(),
            room_id,
            sender_id,
            body,
            sent_at: now,
            read_by: vec![],
        };
        self.repo
            .insert_message(&msg)
            .await
            .map_err(|e| ChatError::Backend(e.to_string()))?;
        self.repo
            .touch_room(room_id, now)
            .await
            .map_err(|e| ChatError::Backend(e.to_string()))?;
        // Best-effort broadcast — never block the request on
        // transport errors.
        if let Err(e) = self
            .transport
            .broadcast_message(ChatEvent {
                room_id,
                message: msg.clone(),
            })
            .await
        {
            tracing::warn!(room_id = %room_id, error = %e, "chat broadcast failed");
        }
        Ok(msg)
    }

    /// Inbox projection for `user`.
    pub async fn list_rooms_for(
        &self,
        user: &User,
        limit: u32,
    ) -> Result<Vec<RoomSummary>, ChatError> {
        let limit = limit.clamp(1, 200);
        self.repo
            .list_rooms_for_user(user.id, limit)
            .await
            .map_err(|e| ChatError::Backend(e.to_string()))
    }

    /// Message timeline, newest first, paginated.
    pub async fn list_messages(
        &self,
        room_id: RoomId,
        user: &User,
        before: Option<chrono::DateTime<Utc>>,
        limit: u32,
    ) -> Result<Vec<ChatMessage>, ChatError> {
        // Membership check — share with the WebSocket gateway
        // so the REST endpoint and the live channel see the
        // same authorization policy.
        if !self
            .repo
            .is_participant(room_id, user.id)
            .await
            .map_err(|e| ChatError::Backend(e.to_string()))?
        {
            return Err(ChatError::NotParticipant(room_id));
        }
        let limit = limit.clamp(1, 200);
        self.repo
            .list_messages(room_id, MessagePage { limit, before })
            .await
            .map_err(|e| ChatError::Backend(e.to_string()))
    }

    /// Append a read receipt to every message the user has not
    /// yet marked as read.
    pub async fn mark_read(&self, room_id: RoomId, user: &User) -> Result<(), ChatError> {
        if !self
            .repo
            .is_participant(room_id, user.id)
            .await
            .map_err(|e| ChatError::Backend(e.to_string()))?
        {
            return Err(ChatError::NotParticipant(room_id));
        }
        self.repo
            .mark_read(room_id, user.id, Utc::now())
            .await
            .map_err(|e| ChatError::Backend(e.to_string()))
    }
}

/// Compactly re-export the message id type for the WebSocket
/// handler that needs to construct server-generated ids.
pub type ServerMessageId = MessageId;

/// Re-export of [`kokkak_domain::ChatError`] for infra adapters
/// that want to construct the same error variants as the
/// application use case.
pub use kokkak_domain::ChatError as ChatUseCaseError;

// Internal: keep the imports list tight for cargo-machete etc.
#[allow(dead_code)]
fn _ensure_used() {
    let _ = ChatRepoError::NotFound;
}
