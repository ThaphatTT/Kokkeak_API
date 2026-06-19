//! `ChatRepository` port (พอร์ตแชท — M8).
//!
//! Two sides of the same aggregate: **rooms** (inbox) and
//! **messages** (timeline). Adapters are responsible for keeping
//! the two consistent — e.g. the MongoDB adapter uses one
//! collection per side with a transaction. The trait surface
//! is intentionally narrow:
//!
//! - `find_room` / `find_or_create_room` for the gateway.
//! - `list_rooms_for_user` for the inbox.
//! - `list_messages` for the message timeline (paginated).
//! - `insert_message` (idempotent on `MessageId`).
//! - `mark_read` to append a read receipt.
//!
//! Read-receipt counting (`unread` in the inbox summary) is
//! computed on the fly by the adapter so callers do not need a
//! second round trip.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::chat::{ChatMessage, ChatRoom, RoomId, RoomSummary};

/// Pagination input for message listing (keyset on `sent_at`).
#[derive(Debug, Clone, Copy)]
pub struct MessagePage {
    /// Max messages to return.
    pub limit: u32,
    /// Return messages strictly older than this cursor.
    /// `None` = from the most recent.
    pub before: Option<DateTime<Utc>>,
}

#[derive(Debug, thiserror::Error)]
pub enum ChatRepoError {
    /// The room / message does not exist.
    #[error("not found: {0}")]
    NotFound(String),

    /// Persistence / driver failure.
    #[error("chat backend error: {0}")]
    Backend(String),
}

#[async_trait]
pub trait ChatRepository: Send + Sync {
    /// Look up a room by id.
    async fn find_room(&self, room_id: RoomId) -> Result<Option<ChatRoom>, ChatRepoError>;

    /// Find a room that already contains exactly the given
    /// participant set (set semantics; order does not matter).
    /// Used by the chat service to dedupe the 1:1 case.
    async fn find_room_by_participants(
        &self,
        participants: &[Uuid],
    ) -> Result<Option<ChatRoom>, ChatRepoError>;

    /// Create a new room. Adapters enforce `participants.len() >= 2`.
    async fn create_room(&self, room: &ChatRoom) -> Result<(), ChatRepoError>;

    /// Update the room's `last_msg_at` cursor (called after
    /// `insert_message`).
    async fn touch_room(
        &self,
        room_id: RoomId,
        last_msg_at: DateTime<Utc>,
    ) -> Result<(), ChatRepoError>;

    /// List rooms the user participates in, newest first.
    /// Returns a summary per room (inbox view).
    async fn list_rooms_for_user(
        &self,
        user_id: Uuid,
        limit: u32,
    ) -> Result<Vec<RoomSummary>, ChatRepoError>;

    /// List messages in `room_id`, newest first, paginated by
    /// `page.before`.
    async fn list_messages(
        &self,
        room_id: RoomId,
        page: MessagePage,
    ) -> Result<Vec<ChatMessage>, ChatRepoError>;

    /// Insert a message. Must be **idempotent on `MessageId`**:
    /// the worker's `chat.persist` queue re-delivers, so the
    /// adapter must dedupe.
    async fn insert_message(&self, message: &ChatMessage) -> Result<(), ChatRepoError>;

    /// Append a read receipt to every message in `room_id` from
    /// the perspective of `user_id`. Idempotent.
    async fn mark_read(
        &self,
        room_id: RoomId,
        user_id: Uuid,
        at: DateTime<Utc>,
    ) -> Result<(), ChatRepoError>;
}

/// Read-only repo lookup used by the WebSocket gateway to check
/// room membership before subscribing.
#[async_trait]
pub trait ChatMembership: Send + Sync {
    /// `true` iff `user_id` is a participant of `room_id`.
    async fn is_participant(&self, room_id: RoomId, user_id: Uuid) -> Result<bool, ChatRepoError>;
}

/// Blanket impl: any [`ChatRepository`] also satisfies
/// [`ChatMembership`] without an extra database hop.
#[async_trait]
impl<T: ChatRepository + ?Sized> ChatMembership for T {
    async fn is_participant(&self, room_id: RoomId, user_id: Uuid) -> Result<bool, ChatRepoError> {
        Ok(self
            .find_room(room_id)
            .await?
            .map(|r| r.contains(user_id))
            .unwrap_or(false))
    }
}
