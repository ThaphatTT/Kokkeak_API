//! SQL Server-backed `ChatRepository` (M10).
//!
//! Production target. Two tables in the `KOKKAK_CHAT` database:
//! ```sql
//! CREATE TABLE chat_room (
//!     id           UNIQUEIDENTIFIER NOT NULL PRIMARY KEY,
//!     participants NVARCHAR(MAX)    NOT NULL,  -- JSON array
//!     created_at   DATETIME2(7)     NOT NULL,
//!     last_msg_at  DATETIME2(7)     NOT NULL,
//!     title        NVARCHAR(255)    NULL
//! );
//! CREATE TABLE chat_message (
//!     id         UNIQUEIDENTIFIER NOT NULL PRIMARY KEY,
//!     room_id    UNIQUEIDENTIFIER NOT NULL,
//!     sender_id  UNIQUEIDENTIFIER NOT NULL,
//!     body       NVARCHAR(MAX)    NOT NULL,
//!     sent_at    DATETIME2(7)     NOT NULL,
//!     read_by    NVARCHAR(MAX)    NOT NULL DEFAULT '[]'  -- JSON
//! );
//! CREATE INDEX ix_chat_msg_room ON chat_message (room_id, sent_at DESC);
//! ```
//!
//! `insert_message` is idempotent on `id` — a duplicate PK
//! insert returns the unique-violation error code; we map
//! that to `Ok(())` so the worker's `chat.persist` re-delivery
//! is safe.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use futures::TryStreamExt;
use tiberius::ToSql;

use kokkak_domain::{
    ChatMessage, ChatRepoError, ChatRepository, ChatRoom, MessageId, MessagePage, Participant,
    RoomId, RoomSummary,
};
use uuid::Uuid;

use crate::db::mssql::MssqlPool;

#[derive(Clone)]
pub struct MssqlChatRepository {
    pool: MssqlPool,
}

impl MssqlChatRepository {
    pub fn new(pool: MssqlPool) -> Self {
        Self { pool }
    }
}

fn err(e: impl std::fmt::Display) -> ChatRepoError {
    ChatRepoError::Backend(e.to_string())
}

fn participants_to_json(parts: &[Participant]) -> String {
    let arr: Vec<serde_json::Value> = parts
        .iter()
        .map(|p| {
            serde_json::json!({
                "user_id": p.user_id.to_string(),
                "role": p.role.as_str(),
            })
        })
        .collect();
    serde_json::to_string(&arr).unwrap_or_else(|_| "[]".into())
}

fn participants_from_json(s: &str) -> Result<Vec<Participant>, ChatRepoError> {
    let arr: Vec<serde_json::Value> =
        serde_json::from_str(s).map_err(|e| err(format!("participants json: {e}")))?;
    let mut out = Vec::with_capacity(arr.len());
    for v in arr {
        let uid_s = v
            .get("user_id")
            .and_then(|x| x.as_str())
            .ok_or_else(|| err("missing user_id"))?;
        let uid: Uuid = uid_s.parse().map_err(|e| err(format!("bad uuid: {e}")))?;
        let role = match v.get("role").and_then(|x| x.as_str()).unwrap_or("customer") {
            "customer" => kokkak_domain::Role::Customer,
            "technician" => kokkak_domain::Role::Technician,
            "admin" => kokkak_domain::Role::Admin,
            _ => kokkak_domain::Role::SuperAdmin,
        };
        out.push(Participant { user_id: uid, role });
    }
    Ok(out)
}

fn read_by_to_json(rb: &[(Uuid, DateTime<Utc>)]) -> String {
    let arr: Vec<serde_json::Value> = rb
        .iter()
        .map(|(u, t)| serde_json::json!({ "user_id": u.to_string(), "at": t.to_rfc3339() }))
        .collect();
    serde_json::to_string(&arr).unwrap_or_else(|_| "[]".into())
}

fn row_to_room(row: &tiberius::Row) -> Result<ChatRoom, ChatRepoError> {
    let id: Uuid = row
        .get::<Uuid, _>(0)
        .ok_or_else(|| err("missing room id"))?;
    let parts: &str = row
        .get::<&str, _>(1)
        .ok_or_else(|| err("missing participants"))?;
    let created_at = row
        .get::<chrono::DateTime<chrono::Utc>, _>(2)
        .ok_or_else(|| err("missing created_at"))?;
    let last_msg_at = row
        .get::<chrono::DateTime<chrono::Utc>, _>(3)
        .ok_or_else(|| err("missing last_msg_at"))?;
    let title = row.get::<&str, _>(4).map(|s| s.to_string());
    Ok(ChatRoom {
        id,
        participants: participants_from_json(parts)?,
        created_at,
        last_msg_at,
        title,
    })
}

fn row_to_message(row: &tiberius::Row) -> Result<ChatMessage, ChatRepoError> {
    let id: Uuid = row.get::<Uuid, _>(0).ok_or_else(|| err("missing id"))?;
    let room_id: Uuid = row
        .get::<Uuid, _>(1)
        .ok_or_else(|| err("missing room_id"))?;
    let sender_id: Uuid = row
        .get::<Uuid, _>(2)
        .ok_or_else(|| err("missing sender_id"))?;
    let body: &str = row.get::<&str, _>(3).ok_or_else(|| err("missing body"))?;
    let sent_at = row
        .get::<chrono::DateTime<chrono::Utc>, _>(4)
        .ok_or_else(|| err("missing sent_at"))?;
    let read_by_json: &str = row.get::<&str, _>(5).unwrap_or("[]");
    let read_by_arr: Vec<serde_json::Value> =
        serde_json::from_str(read_by_json).map_err(|e| err(format!("read_by json: {e}")))?;
    let mut read_by = Vec::new();
    for v in read_by_arr {
        let uid_s = v.get("user_id").and_then(|x| x.as_str());
        let at_s = v.get("at").and_then(|x| x.as_str());
        if let (Some(uid_s), Some(at_s)) = (uid_s, at_s) {
            if let (Ok(uid), Ok(at)) = (
                uid_s.parse::<Uuid>(),
                at_s.parse::<chrono::DateTime<chrono::Utc>>(),
            ) {
                read_by.push((uid, at));
            }
        }
    }
    Ok(ChatMessage {
        id,
        room_id,
        sender_id,
        body: body.to_string(),
        sent_at,
        read_by,
    })
}

/// Helper: collect a `QueryStream` into a `Vec<Row>`. The
/// `BoxStream<'a, _>` borrows `&mut conn`; this helper consumes
/// the stream so the caller can re-borrow `conn` for the next
/// query.
async fn collect_rows(
    stream: tiberius::QueryStream<'_>,
) -> Result<Vec<tiberius::Row>, ChatRepoError> {
    let mut s = stream.into_row_stream();
    let mut out = Vec::new();
    while let Some(row) = s.try_next().await.map_err(err)? {
        out.push(row);
    }
    Ok(out)
}

#[async_trait]
impl ChatRepository for MssqlChatRepository {
    async fn find_room(&self, room_id: RoomId) -> Result<Option<ChatRoom>, ChatRepoError> {
        let mut conn = self.pool.get().await.map_err(err)?;
        let rows = conn
            .query(
                "SELECT id, participants, created_at, last_msg_at, title FROM chat_room WHERE id = @P1",
                &[&room_id as &dyn ToSql],
            )
            .await
            .map_err(err)?;
        let collected = collect_rows(rows).await?;
        if let Some(row) = collected.into_iter().next() {
            return Ok(Some(row_to_room(&row)?));
        }
        Ok(None)
    }

    async fn find_room_by_participants(
        &self,
        participants: &[Uuid],
    ) -> Result<Option<ChatRoom>, ChatRepoError> {
        if participants.len() < 2 {
            return Ok(None);
        }
        let mut ids: Vec<Uuid> = participants.to_vec();
        ids.sort();
        ids.dedup();
        let want: std::collections::BTreeSet<Uuid> = participants.iter().copied().collect();
        let seed = ids[0].to_string();
        let pattern = format!("%{seed}%");
        let mut conn = self.pool.get().await.map_err(err)?;
        let rows = conn
            .query(
                "SELECT id, participants, created_at, last_msg_at, title FROM chat_room WHERE participants LIKE @P1",
                &[&pattern as &dyn ToSql],
            )
            .await
            .map_err(err)?;
        let collected = collect_rows(rows).await?;
        for row in collected {
            let room = row_to_room(&row)?;
            let got: std::collections::BTreeSet<Uuid> =
                room.participants.iter().map(|p| p.user_id).collect();
            if got == want {
                return Ok(Some(room));
            }
        }
        Ok(None)
    }

    async fn create_room(&self, room: &ChatRoom) -> Result<(), ChatRepoError> {
        let mut conn = self.pool.get().await.map_err(err)?;
        let parts = participants_to_json(&room.participants);
        let title = room.title.clone();
        conn.execute(
            "INSERT INTO chat_room(id, participants, created_at, last_msg_at, title) VALUES (@P1, @P2, @P3, @P4, @P5)",
            &[
                &room.id as &dyn ToSql,
                &parts as &dyn ToSql,
                &room.created_at as &dyn ToSql,
                &room.last_msg_at as &dyn ToSql,
                &title as &dyn ToSql,
            ],
        )
        .await
        .map_err(err)?;
        Ok(())
    }

    async fn touch_room(
        &self,
        room_id: RoomId,
        last_msg_at: DateTime<Utc>,
    ) -> Result<(), ChatRepoError> {
        let mut conn = self.pool.get().await.map_err(err)?;
        conn.execute(
            "UPDATE chat_room SET last_msg_at = @P1 WHERE id = @P2",
            &[&last_msg_at as &dyn ToSql, &room_id as &dyn ToSql],
        )
        .await
        .map_err(err)?;
        Ok(())
    }

    async fn list_rooms_for_user(
        &self,
        user_id: Uuid,
        limit: u32,
    ) -> Result<Vec<RoomSummary>, ChatRepoError> {
        let uid = user_id.to_string();
        let limit_i64 = limit.clamp(1, 200) as i64;
        // Phase 1: fetch rooms (release the `&mut conn` borrow
        // by collecting to a Vec).
        let rooms: Vec<ChatRoom> = {
            let mut conn = self.pool.get().await.map_err(err)?;
            let rows = conn
                .query(
                    "SELECT TOP (@P1) id, participants, created_at, last_msg_at, title FROM chat_room \
                     WHERE participants LIKE @P2 ORDER BY last_msg_at DESC",
                    &[&limit_i64 as &dyn ToSql, &format!("%{uid}%") as &dyn ToSql],
                )
                .await
                .map_err(err)?;
            let collected = collect_rows(rows).await?;
            collected
                .iter()
                .map(row_to_room)
                .collect::<Result<Vec<_>, _>>()?
        };
        // Phase 2: per-room latest message + unread count.
        let mut out = Vec::with_capacity(rooms.len());
        for room in rooms {
            let last: Option<ChatMessage> = {
                let mut conn = self.pool.get().await.map_err(err)?;
                let rows = conn
                    .query(
                        "SELECT TOP 1 id, room_id, sender_id, body, sent_at, read_by FROM chat_message \
                         WHERE room_id = @P1 ORDER BY sent_at DESC",
                        &[&room.id as &dyn ToSql],
                    )
                    .await
                    .map_err(err)?;
                let collected = collect_rows(rows).await?;
                collected
                    .into_iter()
                    .next()
                    .map(|r| row_to_message(&r))
                    .transpose()?
            };
            let unread: u32 = {
                let mut conn = self.pool.get().await.map_err(err)?;
                let unread_filter = format!("%\"user_id\":\"{}\"%", user_id);
                let rows = conn
                    .query(
                        "SELECT COUNT(*) AS n FROM chat_message \
                         WHERE room_id = @P1 AND sender_id <> @P2 \
                         AND (read_by IS NULL OR read_by NOT LIKE @P3)",
                        &[
                            &room.id as &dyn ToSql,
                            &user_id as &dyn ToSql,
                            &unread_filter as &dyn ToSql,
                        ],
                    )
                    .await
                    .map_err(err)?;
                let collected = collect_rows(rows).await?;
                if let Some(r) = collected.into_iter().next() {
                    let n: i32 = r.get::<i32, _>(0).unwrap_or(0);
                    n.max(0) as u32
                } else {
                    0
                }
            };
            out.push(RoomSummary {
                room,
                last_message: last,
                unread,
            });
        }
        Ok(out)
    }

    async fn list_messages(
        &self,
        room_id: RoomId,
        page: MessagePage,
    ) -> Result<Vec<ChatMessage>, ChatRepoError> {
        let limit_i64 = page.limit.clamp(1, 200) as i64;
        let mut conn = self.pool.get().await.map_err(err)?;
        let rows = if let Some(before) = page.before {
            conn.query(
                "SELECT TOP (@P1) id, room_id, sender_id, body, sent_at, read_by \
                 FROM chat_message WHERE room_id = @P2 AND sent_at < @P3 \
                 ORDER BY sent_at DESC",
                &[
                    &limit_i64 as &dyn ToSql,
                    &room_id as &dyn ToSql,
                    &before as &dyn ToSql,
                ],
            )
            .await
            .map_err(err)?
        } else {
            conn.query(
                "SELECT TOP (@P1) id, room_id, sender_id, body, sent_at, read_by \
                 FROM chat_message WHERE room_id = @P2 ORDER BY sent_at DESC",
                &[&limit_i64 as &dyn ToSql, &room_id as &dyn ToSql],
            )
            .await
            .map_err(err)?
        };
        let collected = collect_rows(rows).await?;
        collected.iter().map(row_to_message).collect()
    }

    async fn insert_message(&self, message: &ChatMessage) -> Result<(), ChatRepoError> {
        let mut conn = self.pool.get().await.map_err(err)?;
        let read_by = read_by_to_json(&message.read_by);
        match conn
            .execute(
                "INSERT INTO chat_message(id, room_id, sender_id, body, sent_at, read_by) \
                 VALUES (@P1, @P2, @P3, @P4, @P5, @P6)",
                &[
                    &message.id as &dyn ToSql,
                    &message.room_id as &dyn ToSql,
                    &message.sender_id as &dyn ToSql,
                    &message.body as &dyn ToSql,
                    &message.sent_at as &dyn ToSql,
                    &read_by as &dyn ToSql,
                ],
            )
            .await
        {
            Ok(_) => Ok(()),
            Err(e) => {
                let s = e.to_string();
                if s.contains("2627") || s.contains("duplicate") || s.contains("PRIMARY KEY") {
                    Ok(())
                } else {
                    Err(err(s))
                }
            }
        }
    }

    async fn mark_read(
        &self,
        room_id: RoomId,
        user_id: Uuid,
        at: DateTime<Utc>,
    ) -> Result<(), ChatRepoError> {
        // Phase 1: find unread message ids in the room.
        let ids: Vec<Uuid> = {
            let mut conn = self.pool.get().await.map_err(err)?;
            let skip_filter = format!("%\"user_id\":\"{}\"%", user_id);
            let rows = conn
                .query(
                    "SELECT id FROM chat_message \
                     WHERE room_id = @P1 AND sender_id <> @P2 \
                     AND (read_by IS NULL OR read_by NOT LIKE @P3)",
                    &[
                        &room_id as &dyn ToSql,
                        &user_id as &dyn ToSql,
                        &skip_filter as &dyn ToSql,
                    ],
                )
                .await
                .map_err(err)?;
            let collected = collect_rows(rows).await?;
            collected
                .iter()
                .map(|r| r.get::<Uuid, _>(0).ok_or_else(|| err("missing id")))
                .collect::<Result<Vec<_>, _>>()?
        };
        // Phase 2: append a read receipt to each id. M10 stores
        // the receipt as a *new* JSON array (replacing the old
        // one). M12+ switches to JSON_MODIFY for in-place append.
        let read_entry = serde_json::json!({
            "user_id": user_id.to_string(),
            "at": at.to_rfc3339(),
        });
        for id in ids {
            let mut conn = self.pool.get().await.map_err(err)?;
            conn.execute(
                "UPDATE chat_message SET read_by = @P1 WHERE id = @P2",
                &[&read_entry.to_string() as &dyn ToSql, &id as &dyn ToSql],
            )
            .await
            .map_err(err)?;
        }
        Ok(())
    }
}

#[allow(dead_code)]
fn _ensure_types(_: RoomId, _: MessageId) {}
