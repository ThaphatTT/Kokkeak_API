

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::chat::{ChatMessage, ChatRoom, RoomId, RoomSummary};

#[derive(Debug, Clone, Copy)]
pub struct MessagePage {

    pub limit: u32,

    pub before: Option<DateTime<Utc>>,
}

#[derive(Debug, thiserror::Error)]
pub enum ChatRepoError {

    #[error("not found: {0}")]
    NotFound(String),

    #[error("chat backend error: {0}")]
    Backend(String),
}

#[async_trait]
pub trait ChatRepository: Send + Sync {

    async fn find_room(&self, room_id: RoomId) -> Result<Option<ChatRoom>, ChatRepoError>;

    async fn find_room_by_participants(
        &self,
        participants: &[Uuid],
    ) -> Result<Option<ChatRoom>, ChatRepoError>;

    async fn create_room(&self, room: &ChatRoom) -> Result<(), ChatRepoError>;

    async fn touch_room(
        &self,
        room_id: RoomId,
        last_msg_at: DateTime<Utc>,
    ) -> Result<(), ChatRepoError>;

    async fn list_rooms_for_user(
        &self,
        user_id: Uuid,
        limit: u32,
    ) -> Result<Vec<RoomSummary>, ChatRepoError>;

    async fn list_messages(
        &self,
        room_id: RoomId,
        page: MessagePage,
    ) -> Result<Vec<ChatMessage>, ChatRepoError>;

    async fn insert_message(&self, message: &ChatMessage) -> Result<(), ChatRepoError>;

    async fn mark_read(
        &self,
        room_id: RoomId,
        user_id: Uuid,
        at: DateTime<Utc>,
    ) -> Result<(), ChatRepoError>;
}

#[async_trait]
pub trait ChatMembership: Send + Sync {

    async fn is_participant(&self, room_id: RoomId, user_id: Uuid) -> Result<bool, ChatRepoError>;
}

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
