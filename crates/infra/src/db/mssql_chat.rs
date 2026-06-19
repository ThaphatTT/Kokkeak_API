//! SQL Server-backed `ChatRepository` (M14.5 — stored procedures).
//!
//! See `migrations/20260620000004_sp_chat.sql` for SP definitions.
//! Methods without a matching SP return `ChatRepoError::Backend` and are
//! scheduled for M15+.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use kokkak_domain::chat::{ChatMessage, ChatRoom, Participant, RoomId, RoomSummary};
use kokkak_domain::traits::chat::{ChatRepoError, ChatRepository, MessagePage};
use tiberius::ToSql;
use uuid::Uuid;

use crate::db::mssql::{exec_sp, read_i32, read_str, read_uuid, MssqlPool};

#[derive(Clone)]
pub struct MssqlChatRepository {
    pool: MssqlPool,
}

impl MssqlChatRepository {
    pub fn new(pool: MssqlPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl ChatRepository for MssqlChatRepository {
    async fn find_room(&self, room_id: RoomId) -> Result<Option<ChatRoom>, ChatRepoError> {
        // M14.5: scan the inbox of each participant (deduped at the SP layer
        // when API_CHAT_FIND_BY_ID lands in M15+).
        let rows = exec_sp(
            &self.pool,
            "EXEC dbo.API_CHAT_LIST_ROOMS @p_user_guid = @P1",
            &[&Uuid::nil() as &dyn ToSql],
        )
        .await
        .map_err(|e| ChatRepoError::Backend(e.to_string()))?;
        for row in &rows {
            if read_uuid(row, 0) == Some(room_id) {
                let a = read_uuid(row, 3).unwrap_or_else(Uuid::nil);
                let b = read_uuid(row, 4).unwrap_or_else(Uuid::nil);
                return Ok(Some(ChatRoom {
                    id: room_id as Uuid,
                    participants: vec![
                        Participant {
                            user_id: a,
                            role: kokkak_domain::Role::Customer,
                        },
                        Participant {
                            user_id: b,
                            role: kokkak_domain::Role::Customer,
                        },
                    ],
                    created_at: row
                        .get::<chrono::DateTime<chrono::Utc>, _>(5)
                        .unwrap_or_else(Utc::now),
                    last_msg_at: row
                        .get::<chrono::DateTime<chrono::Utc>, _>(6)
                        .unwrap_or_else(Utc::now),
                    title: None,
                }));
            }
        }
        Ok(None)
    }

    async fn find_room_by_participants(
        &self,
        _participants: &[Uuid],
    ) -> Result<Option<ChatRoom>, ChatRepoError> {
        Err(ChatRepoError::Backend(
            "MssqlChatRepository::find_room_by_participants — SP lands in M15+".into(),
        ))
    }

    async fn create_room(&self, room: &ChatRoom) -> Result<(), ChatRepoError> {
        let (a, b) = match room.participants.as_slice() {
            [p1, p2] => (p1.user_id, p2.user_id),
            _ => {
                return Err(ChatRepoError::Backend(
                    "1:1 chat requires exactly 2 participants".into(),
                ))
            }
        };
        let rows = exec_sp(
            &self.pool,
            "EXEC dbo.API_CHAT_CREATE_ROOM \
                @p_participant_a = @P1, @p_participant_b = @P2, @p_is_group = @P3",
            &[&a as &dyn ToSql, &b as &dyn ToSql, &0_i32 as &dyn ToSql],
        )
        .await
        .map_err(|e| ChatRepoError::Backend(e.to_string()))?;
        let _ = rows;
        Ok(())
    }

    async fn touch_room(
        &self,
        _room_id: RoomId,
        _last_msg_at: DateTime<Utc>,
    ) -> Result<(), ChatRepoError> {
        // M14.5: API_CHAT_SEND_MESSAGE bumps room_last_msg_at internally.
        Ok(())
    }

    async fn list_rooms_for_user(
        &self,
        user_id: Uuid,
        _limit: u32,
    ) -> Result<Vec<RoomSummary>, ChatRepoError> {
        let rows = exec_sp(
            &self.pool,
            "EXEC dbo.API_CHAT_LIST_ROOMS @p_user_guid = @P1",
            &[&user_id as &dyn ToSql],
        )
        .await
        .map_err(|e| ChatRepoError::Backend(e.to_string()))?;
        let mut out = Vec::with_capacity(rows.len());
        for row in &rows {
            let id = read_uuid(row, 0).unwrap_or_else(Uuid::nil);
            let a = read_uuid(row, 3).unwrap_or_else(Uuid::nil);
            let b = read_uuid(row, 4).unwrap_or_else(Uuid::nil);
            let created_at = row
                .get::<chrono::DateTime<chrono::Utc>, _>(5)
                .unwrap_or_else(Utc::now);
            let last_msg_at = row
                .get::<chrono::DateTime<chrono::Utc>, _>(6)
                .unwrap_or_else(Utc::now);
            out.push(RoomSummary {
                room: ChatRoom {
                    id: id as Uuid,
                    participants: vec![
                        Participant {
                            user_id: a,
                            role: kokkak_domain::Role::Customer,
                        },
                        Participant {
                            user_id: b,
                            role: kokkak_domain::Role::Customer,
                        },
                    ],
                    created_at,
                    last_msg_at,
                    title: None,
                },
                last_message: None,
                unread: read_i32(row, 7).unwrap_or(0).max(0) as u32,
            });
        }
        Ok(out)
    }

    async fn list_messages(
        &self,
        room_id: RoomId,
        page: MessagePage,
    ) -> Result<Vec<ChatMessage>, ChatRepoError> {
        let limit = page.limit as i32;
        let before = page.before;
        let rows = exec_sp(
            &self.pool,
            "EXEC dbo.API_CHAT_LIST_MESSAGES \
                @p_room_guid = @P1, @p_limit = @P2, @p_before = @P3",
            &[
                &room_id as &dyn ToSql,
                &limit as &dyn ToSql,
                &before as &dyn ToSql,
            ],
        )
        .await
        .map_err(|e| ChatRepoError::Backend(e.to_string()))?;
        Ok(rows
            .iter()
            .map(|r| ChatMessage {
                id: read_uuid(r, 0).unwrap_or_else(Uuid::nil),
                room_id,
                sender_id: read_uuid(r, 2).unwrap_or_else(Uuid::nil),
                body: read_str(r, 3).unwrap_or("").to_string(),
                sent_at: r
                    .get::<chrono::DateTime<chrono::Utc>, _>(4)
                    .unwrap_or_else(Utc::now),
                read_by: Vec::new(),
            })
            .collect())
    }

    async fn insert_message(&self, message: &ChatMessage) -> Result<(), ChatRepoError> {
        let rows = exec_sp(
            &self.pool,
            "EXEC dbo.API_CHAT_SEND_MESSAGE \
                @p_room_guid = @P1, @p_sender_guid = @P2, @p_body = @P3",
            &[
                &message.room_id as &dyn ToSql,
                &message.sender_id as &dyn ToSql,
                &message.body as &dyn ToSql,
            ],
        )
        .await
        .map_err(|e| ChatRepoError::Backend(e.to_string()))?;
        let _ = rows;
        Ok(())
    }

    async fn mark_read(
        &self,
        room_id: RoomId,
        user_id: Uuid,
        _at: DateTime<Utc>,
    ) -> Result<(), ChatRepoError> {
        let _ = exec_sp(
            &self.pool,
            "EXEC dbo.API_CHAT_MARK_READ \
                @p_room_guid = @P1, @p_user_guid = @P2",
            &[&room_id as &dyn ToSql, &user_id as &dyn ToSql],
        )
        .await
        .map_err(|e| ChatRepoError::Backend(e.to_string()))?;
        Ok(())
    }
}
