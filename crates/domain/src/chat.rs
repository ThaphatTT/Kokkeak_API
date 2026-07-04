

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::user::Role;

pub type RoomId = Uuid;

pub type MessageId = Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Participant {

    pub user_id: Uuid,

    pub role: Role,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ChatRoom {

    pub id: RoomId,

    pub participants: Vec<Participant>,

    pub created_at: DateTime<Utc>,

    pub last_msg_at: DateTime<Utc>,

    pub title: Option<String>,
}

impl ChatRoom {

    pub fn contains(&self, user_id: Uuid) -> bool {
        self.participants.iter().any(|p| p.user_id == user_id)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ChatMessage {

    pub id: MessageId,

    pub room_id: RoomId,

    pub sender_id: Uuid,

    pub body: String,

    pub sent_at: DateTime<Utc>,

    pub read_by: Vec<(Uuid, DateTime<Utc>)>,
}

impl ChatMessage {

    pub fn is_blank(&self) -> bool {
        self.body.trim().is_empty()
    }

    pub fn is_read_by(&self, user_id: Uuid) -> bool {
        self.read_by.iter().any(|(uid, _)| *uid == user_id)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct RoomSummary {

    pub room: ChatRoom,

    pub last_message: Option<ChatMessage>,

    pub unread: u32,
}

#[derive(Debug, thiserror::Error)]
pub enum ChatError {

    #[error("not a participant of room {0}")]
    NotParticipant(RoomId),

    #[error("room {0} not found")]
    RoomNotFound(RoomId),

    #[error("invalid body: {0}")]
    InvalidBody(String),

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
