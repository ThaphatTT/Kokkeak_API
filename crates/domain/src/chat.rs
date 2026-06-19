//! Chat domain (แชท — M8).
//!
//! Two entities:
//!
//! - [`ChatRoom`] — a 1:1 / group room between two or more users.
//!   A room is identified by a stable [`RoomId`] and carries the
//!   list of [`Participant`]s and the last-message cursor so the
//!   inbox can be sorted by recency.
//!
//! - [`ChatMessage`] — one message in a room. M8 stores every
//!   message and keeps a read-receipt set so a client can render
//!   "seen by" ticks. The body is plain text for M8; M12+ can add
//!   attachments via the [`Storage`] port.
//!
//! The domain layer is **pure**: persistence and real-time
//! distribution live in `infra` (MongoDB + Redis pubsub). See
//! `AGENTS.md` § 6 and § 8.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::user::Role;

/// Stable room identifier (UUID v4).
pub type RoomId = Uuid;

/// Stable message identifier (UUID v4).
pub type MessageId = Uuid;

/// One participant in a room (ผู้เข้าร่วมห้องแชท).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct Participant {
    /// User UUID.
    pub user_id: Uuid,
    /// Role at the time the room was created (used to pick the
    /// room icon in the UI).
    pub role: Role,
}

/// Chat room (ห้องแชท).
///
/// A room is a stable container; participants may be added or
/// removed in a future revision. M8 keeps the participant set
/// immutable for simplicity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatRoom {
    /// Stable identifier.
    pub id: RoomId,
    /// All participants (>= 2).
    pub participants: Vec<Participant>,
    /// When the room was opened.
    pub created_at: DateTime<Utc>,
    /// Last activity (set to the latest message's `sent_at`).
    /// Drives the inbox sort order.
    pub last_msg_at: DateTime<Utc>,
    /// Opaque title for the UI; defaults to the counterparty's
    /// display name.
    pub title: Option<String>,
}

impl ChatRoom {
    /// `true` iff `user_id` is one of the participants.
    pub fn contains(&self, user_id: Uuid) -> bool {
        self.participants.iter().any(|p| p.user_id == user_id)
    }
}

/// One chat message (ข้อความแชทหนึ่งข้อความ).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    /// Stable identifier (also the dedup key in the
    /// `chat.persist` queue — see AGENTS.md § 10).
    pub id: MessageId,
    /// Room the message belongs to.
    pub room_id: RoomId,
    /// Sender (must be a participant of `room_id`).
    pub sender_id: Uuid,
    /// Plain-text body. M12+ can extend this to a discriminated
    /// union for images / files (see [`crate::storage::Storage`]).
    pub body: String,
    /// UTC timestamp of when the server accepted the message.
    pub sent_at: DateTime<Utc>,
    /// Read receipts (`user_id` -> when the message was read).
    /// Empty for the sender's own view.
    pub read_by: Vec<(Uuid, DateTime<Utc>)>,
}

impl ChatMessage {
    /// `true` iff the message is empty (used by the validation
    /// layer to reject blank sends).
    pub fn is_blank(&self) -> bool {
        self.body.trim().is_empty()
    }

    /// `true` iff `user_id` has a read receipt for this message.
    pub fn is_read_by(&self, user_id: Uuid) -> bool {
        self.read_by.iter().any(|(uid, _)| *uid == user_id)
    }
}

/// Room summary for an inbox (หน้ารายการห้อง — projection).
///
/// Stored as part of the [`ChatRepository::list_rooms_for_user`]
/// payload so the handler can render the inbox without an extra
/// round trip.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoomSummary {
    /// Room.
    pub room: ChatRoom,
    /// Latest message (None when the room was just created).
    pub last_message: Option<ChatMessage>,
    /// Count of messages not yet read by the requesting user.
    pub unread: u32,
}

/// Errors that the chat use case can return (ข้อผิดพลาดของแชท).
#[derive(Debug, thiserror::Error)]
pub enum ChatError {
    /// The sender is not a participant of the room.
    #[error("not a participant of room {0}")]
    NotParticipant(RoomId),

    /// The room does not exist.
    #[error("room {0} not found")]
    RoomNotFound(RoomId),

    /// Body was blank or too long.
    #[error("invalid body: {0}")]
    InvalidBody(String),

    /// Persistence / transport failure.
    #[error("chat backend error: {0}")]
    Backend(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user_id(byte: u8) -> Uuid {
        Uuid::from_bytes([byte; 16])
    }

    fn room() -> ChatRoom {
        ChatRoom {
            id: Uuid::new_v4(),
            participants: vec![
                Participant {
                    user_id: user_id(1),
                    role: Role::Customer,
                },
                Participant {
                    user_id: user_id(2),
                    role: Role::Technician,
                },
            ],
            created_at: Utc::now(),
            last_msg_at: Utc::now(),
            title: None,
        }
    }

    fn message(room_id: RoomId, sender: Uuid, body: &str) -> ChatMessage {
        ChatMessage {
            id: Uuid::new_v4(),
            room_id,
            sender_id: sender,
            body: body.into(),
            sent_at: Utc::now(),
            read_by: vec![],
        }
    }

    #[test]
    fn room_contains_membership_check() {
        let r = room();
        assert!(r.contains(user_id(1)));
        assert!(!r.contains(user_id(3)));
    }

    #[test]
    fn message_is_blank_only_for_whitespace() {
        let m = message(Uuid::new_v4(), user_id(1), "  \n\t ");
        assert!(m.is_blank());
        let m = message(Uuid::new_v4(), user_id(1), "hello");
        assert!(!m.is_blank());
    }

    #[test]
    fn message_read_receipts_track_per_user() {
        let m = message(Uuid::new_v4(), user_id(1), "x");
        assert!(!m.is_read_by(user_id(2)));
        let mut m = m;
        m.read_by.push((user_id(2), Utc::now()));
        assert!(m.is_read_by(user_id(2)));
        assert!(!m.is_read_by(user_id(3)));
    }

    #[test]
    fn chat_error_messages_are_stable() {
        let id = Uuid::nil();
        assert!(ChatError::NotParticipant(id)
            .to_string()
            .contains("not a participant"));
        assert!(ChatError::RoomNotFound(id)
            .to_string()
            .contains("not found"));
        assert!(ChatError::InvalidBody("x".into())
            .to_string()
            .contains("invalid body"));
    }
}
