

use std::sync::Arc;

use chrono::Utc;
use kokkak_domain::{
    ChatError, ChatMembership, ChatMessage, ChatRepoError, ChatRepository, ChatRoom, MessageId,
    MessagePage, Participant, RoomId, RoomSummary, User,
};
use tokio::sync::broadcast;
use uuid::Uuid;

const MAX_BODY_LEN: usize = 4_000;

#[derive(Debug, Clone)]
pub struct ChatEvent {

    pub room_id: RoomId,

    pub message: ChatMessage,
}

#[async_trait::async_trait]
pub trait ChatTransport: Send + Sync {

    async fn broadcast_message(&self, event: ChatEvent) -> Result<(), ChatError>;
}

pub struct BroadcastTransport {

    pub tx: broadcast::Sender<ChatEvent>,
}

impl BroadcastTransport {

    pub fn new(capacity: usize) -> Self {
        let (tx, _rx) = broadcast::channel(capacity);
        Self { tx }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<ChatEvent> {
        self.tx.subscribe()
    }
}

#[async_trait::async_trait]
impl ChatTransport for BroadcastTransport {
    async fn broadcast_message(&self, event: ChatEvent) -> Result<(), ChatError> {

        let _ = self.tx.send(event);
        Ok(())
    }
}

impl Default for BroadcastTransport {
    fn default() -> Self {
        Self::new(256)
    }
}

pub struct ChatService {
    repo: Arc<dyn ChatRepository>,
    transport: Arc<dyn ChatTransport>,
}

impl ChatService {

    pub fn new(repo: Arc<dyn ChatRepository>, transport: Arc<dyn ChatTransport>) -> Self {
        Self { repo, transport }
    }

    pub fn repo(&self) -> &Arc<dyn ChatRepository> {
        &self.repo
    }

    pub async fn open_room(
        &self,
        mut participants: Vec<Participant>,
    ) -> Result<ChatRoom, ChatError> {
        if participants.len() < 2 {
            return Err(ChatError::InvalidBody(
                "room needs at least 2 participants".into(),
            ));
        }

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

    pub async fn list_messages(
        &self,
        room_id: RoomId,
        user: &User,
        before: Option<chrono::DateTime<Utc>>,
        limit: u32,
    ) -> Result<Vec<ChatMessage>, ChatError> {

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

pub type ServerMessageId = MessageId;

pub use kokkak_domain::ChatError as ChatUseCaseError;

#[allow(dead_code)]
fn _ensure_used() {
    let _ = ChatRepoError::NotFound;
}
