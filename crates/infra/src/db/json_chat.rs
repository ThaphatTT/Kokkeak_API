//! JSON-file-backed `ChatRepository` (M8).
//!
//! Stores rooms and messages in two files (rooms.json +
//! messages.json). The aggregate is small enough for the JSON-DB
//! sim; production swaps this for the MongoDB-backed
//! `MongoChatRepository` in the same module.
//!
//! `insert_message` is idempotent on `MessageId` (the worker's
//! `chat.persist` queue re-delivers on retry) so the inbox +
//! message timeline stay consistent.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use kokkak_domain::{
    ChatMessage, ChatRepoError, ChatRepository, ChatRoom, MessageId, MessagePage, RoomId,
    RoomSummary,
};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use uuid::Uuid;

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
struct ChatDb {
    rooms: Vec<ChatRoom>,
    messages: Vec<ChatMessage>,
}

struct Inner {
    db: ChatDb,
    path: PathBuf,
    room_index: HashMap<RoomId, usize>,
    msg_index: HashMap<MessageId, usize>,
    /// `room_id -> sorted indices into `db.messages` (newest first).
    /// Rebuilt lazily on first read after a mutation.
    msg_by_room_dirty: bool,
    msg_by_room: HashMap<RoomId, Vec<usize>>,
}

#[derive(Clone)]
pub struct JsonChatRepository {
    inner: Arc<Mutex<Inner>>,
}

impl JsonChatRepository {
    /// Build a fresh in-memory chat DB. The persistence layer
    /// is a no-op (writes are dropped) — the `Arc<Mutex<Inner>>`
    /// state still works, so this is safe for the dev / e2e
    /// flow.
    pub fn open_in_memory() -> Result<Self, ChatRepoError> {
        Ok(Self {
            inner: Arc::new(Mutex::new(Inner {
                db: ChatDb::default(),
                path: std::env::temp_dir()
                    .join(format!("kokkak_chat_inmem-{}.json", Uuid::new_v4())),
                room_index: HashMap::new(),
                msg_index: HashMap::new(),
                msg_by_room: HashMap::new(),
                msg_by_room_dirty: true,
            })),
        })
    }

    /// Open (or create) the chat DB at `path`.
    pub async fn open(path: impl AsRef<Path>) -> Result<Self, ChatRepoError> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| ChatRepoError::Backend(e.to_string()))?;
        }
        let db = if path.exists() {
            let bytes = tokio::fs::read(&path)
                .await
                .map_err(|e| ChatRepoError::Backend(e.to_string()))?;
            if bytes.is_empty() {
                ChatDb::default()
            } else {
                serde_json::from_slice(&bytes).unwrap_or_default()
            }
        } else {
            ChatDb::default()
        };
        let mut room_index = HashMap::new();
        for (i, r) in db.rooms.iter().enumerate() {
            room_index.insert(r.id, i);
        }
        let mut msg_index = HashMap::new();
        for (i, m) in db.messages.iter().enumerate() {
            msg_index.insert(m.id, i);
        }
        let inner = Inner {
            db,
            path,
            room_index,
            msg_index,
            msg_by_room: HashMap::new(),
            msg_by_room_dirty: true,
        };
        Ok(Self {
            inner: Arc::new(Mutex::new(inner)),
        })
    }

    async fn persist(inner: &Inner) -> Result<(), ChatRepoError> {
        let tmp = inner.path.with_extension("json.tmp");
        let bytes = serde_json::to_vec_pretty(&inner.db)
            .map_err(|e| ChatRepoError::Backend(e.to_string()))?;
        tokio::fs::write(&tmp, &bytes)
            .await
            .map_err(|e| ChatRepoError::Backend(e.to_string()))?;
        tokio::fs::rename(&tmp, &inner.path)
            .await
            .map_err(|e| ChatRepoError::Backend(e.to_string()))?;
        Ok(())
    }

    fn rebuild_msg_index(inner: &mut Inner) {
        inner.msg_by_room.clear();
        for (i, m) in inner.db.messages.iter().enumerate() {
            inner.msg_by_room.entry(m.room_id).or_default().push(i);
        }
        for v in inner.msg_by_room.values_mut() {
            v.sort_by(|&a, &b| {
                inner.db.messages[b]
                    .sent_at
                    .cmp(&inner.db.messages[a].sent_at)
            });
        }
        inner.msg_by_room_dirty = false;
    }
}

#[async_trait]
impl ChatRepository for JsonChatRepository {
    async fn find_room(&self, room_id: RoomId) -> Result<Option<ChatRoom>, ChatRepoError> {
        let g = self.inner.lock().await;
        Ok(g.room_index
            .get(&room_id)
            .and_then(|&i| g.db.rooms.get(i))
            .cloned())
    }

    async fn find_room_by_participants(
        &self,
        participants: &[Uuid],
    ) -> Result<Option<ChatRoom>, ChatRepoError> {
        let mut want: std::collections::BTreeSet<Uuid> = participants.iter().copied().collect();
        want.remove(&Uuid::nil());
        if want.len() < 2 {
            return Ok(None);
        }
        let g = self.inner.lock().await;
        for r in &g.db.rooms {
            let mut got: std::collections::BTreeSet<Uuid> =
                r.participants.iter().map(|p| p.user_id).collect();
            got.remove(&Uuid::nil());
            if got == want {
                return Ok(Some(r.clone()));
            }
        }
        Ok(None)
    }

    async fn create_room(&self, room: &ChatRoom) -> Result<(), ChatRepoError> {
        let mut g = self.inner.lock().await;
        if g.room_index.contains_key(&room.id) {
            return Err(ChatRepoError::Backend(format!("room {} exists", room.id)));
        }
        g.db.rooms.push(room.clone());
        let i = g.db.rooms.len() - 1;
        g.room_index.insert(room.id, i);
        Self::persist(&g).await
    }

    async fn touch_room(
        &self,
        room_id: RoomId,
        last_msg_at: DateTime<Utc>,
    ) -> Result<(), ChatRepoError> {
        let mut g = self.inner.lock().await;
        let Some(&i) = g.room_index.get(&room_id) else {
            return Err(ChatRepoError::NotFound(format!("room {room_id}")));
        };
        g.db.rooms[i].last_msg_at = last_msg_at;
        Self::persist(&g).await
    }

    async fn list_rooms_for_user(
        &self,
        user_id: Uuid,
        limit: u32,
    ) -> Result<Vec<RoomSummary>, ChatRepoError> {
        let mut g = self.inner.lock().await;
        if g.msg_by_room_dirty {
            Self::rebuild_msg_index(&mut g);
        }
        let limit = limit.clamp(1, 200) as usize;
        let mut out: Vec<(DateTime<Utc>, RoomSummary)> =
            g.db.rooms
                .iter()
                .filter(|r| r.contains(user_id))
                .map(|r| {
                    let last = g
                        .msg_by_room
                        .get(&r.id)
                        .and_then(|v| v.first())
                        .and_then(|&i| g.db.messages.get(i))
                        .cloned();
                    let unread = g
                        .msg_by_room
                        .get(&r.id)
                        .map(|v| {
                            v.iter()
                                .filter_map(|&i| g.db.messages.get(i))
                                .filter(|m| m.sender_id != user_id && !m.is_read_by(user_id))
                                .count() as u32
                        })
                        .unwrap_or(0);
                    (
                        r.last_msg_at,
                        RoomSummary {
                            room: r.clone(),
                            last_message: last,
                            unread,
                        },
                    )
                })
                .collect();
        out.sort_by(|a, b| b.0.cmp(&a.0));
        Ok(out.into_iter().take(limit).map(|(_, s)| s).collect())
    }

    async fn list_messages(
        &self,
        room_id: RoomId,
        page: MessagePage,
    ) -> Result<Vec<ChatMessage>, ChatRepoError> {
        let mut g = self.inner.lock().await;
        if g.msg_by_room_dirty {
            Self::rebuild_msg_index(&mut g);
        }
        let limit = page.limit.clamp(1, 200) as usize;
        let Some(indices) = g.msg_by_room.get(&room_id) else {
            return Ok(vec![]);
        };
        let before = page.before;
        let out: Vec<ChatMessage> = indices
            .iter()
            .filter_map(|&i| g.db.messages.get(i))
            .filter(|m| match before {
                Some(t) => m.sent_at < t,
                None => true,
            })
            .take(limit)
            .cloned()
            .collect();
        Ok(out)
    }

    async fn insert_message(&self, message: &ChatMessage) -> Result<(), ChatRepoError> {
        let mut g = self.inner.lock().await;
        if g.msg_index.contains_key(&message.id) {
            return Ok(());
        }
        g.db.messages.push(message.clone());
        let i = g.db.messages.len() - 1;
        g.msg_index.insert(message.id, i);
        g.msg_by_room_dirty = true;
        Self::persist(&g).await
    }

    async fn mark_read(
        &self,
        room_id: RoomId,
        user_id: Uuid,
        at: DateTime<Utc>,
    ) -> Result<(), ChatRepoError> {
        let mut g = self.inner.lock().await;
        let mut touched = false;
        for m in g.db.messages.iter_mut() {
            if m.room_id == room_id && m.sender_id != user_id && !m.is_read_by(user_id) {
                m.read_by.push((user_id, at));
                touched = true;
            }
        }
        if touched {
            Self::persist(&g).await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use kokkak_domain::{Participant, Role};

    fn tmp(name: &str) -> PathBuf {
        std::env::temp_dir()
            .join("kokkak_chat_repo_test")
            .join(name)
    }

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

    #[tokio::test]
    async fn open_in_memory_works() {
        let repo = JsonChatRepository::open_in_memory().unwrap();
        assert!(repo.find_room(Uuid::new_v4()).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn open_creates_empty_db() {
        let path = tmp("a.json");
        let _ = std::fs::remove_file(&path);
        let repo = JsonChatRepository::open(&path).await.unwrap();
        assert!(repo.find_room(Uuid::new_v4()).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn create_and_find_room() {
        let path = tmp("b.json");
        let _ = std::fs::remove_file(&path);
        let repo = JsonChatRepository::open(&path).await.unwrap();
        let r = room();
        let id = r.id;
        repo.create_room(&r).await.unwrap();
        let got = repo.find_room(id).await.unwrap().unwrap();
        assert_eq!(got.participants.len(), 2);
    }

    #[tokio::test]
    async fn find_room_by_participants_dedupes() {
        let path = tmp("c.json");
        let _ = std::fs::remove_file(&path);
        let repo = JsonChatRepository::open(&path).await.unwrap();
        let r = room();
        repo.create_room(&r).await.unwrap();
        let got = repo
            .find_room_by_participants(&[user_id(2), user_id(1)])
            .await
            .unwrap();
        assert_eq!(got.unwrap().id, r.id);
    }

    #[tokio::test]
    async fn insert_message_is_idempotent() {
        let path = tmp("d.json");
        let _ = std::fs::remove_file(&path);
        let repo = JsonChatRepository::open(&path).await.unwrap();
        let r = room();
        repo.create_room(&r).await.unwrap();
        let m = ChatMessage {
            id: Uuid::new_v4(),
            room_id: r.id,
            sender_id: user_id(1),
            body: "hi".into(),
            sent_at: Utc::now(),
            read_by: vec![],
        };
        repo.insert_message(&m).await.unwrap();
        repo.insert_message(&m).await.unwrap();
        let page = MessagePage {
            limit: 10,
            before: None,
        };
        let got = repo.list_messages(r.id, page).await.unwrap();
        assert_eq!(got.len(), 1);
    }

    #[tokio::test]
    async fn list_rooms_for_user_unread_count() {
        let path = tmp("e.json");
        let _ = std::fs::remove_file(&path);
        let repo = JsonChatRepository::open(&path).await.unwrap();
        let r = room();
        repo.create_room(&r).await.unwrap();
        for _ in 0..2 {
            repo.insert_message(&ChatMessage {
                id: Uuid::new_v4(),
                room_id: r.id,
                sender_id: user_id(1),
                body: "x".into(),
                sent_at: Utc::now(),
                read_by: vec![],
            })
            .await
            .unwrap();
        }
        let inbox = repo.list_rooms_for_user(user_id(2), 10).await.unwrap();
        assert_eq!(inbox.len(), 1);
        assert_eq!(inbox[0].unread, 2);
        repo.mark_read(r.id, user_id(2), Utc::now()).await.unwrap();
        let inbox = repo.list_rooms_for_user(user_id(2), 10).await.unwrap();
        assert_eq!(inbox[0].unread, 0);
    }

    #[tokio::test]
    async fn touch_room_updates_last_msg_at() {
        let path = tmp("f.json");
        let _ = std::fs::remove_file(&path);
        let repo = JsonChatRepository::open(&path).await.unwrap();
        let r = room();
        let id = r.id;
        repo.create_room(&r).await.unwrap();
        let new_t = Utc::now() + chrono::Duration::seconds(60);
        repo.touch_room(id, new_t).await.unwrap();
        let got = repo.find_room(id).await.unwrap().unwrap();
        assert_eq!(got.last_msg_at.timestamp(), new_t.timestamp());
    }

    #[allow(dead_code)]
    fn _ensure_message_id_used(_: MessageId) {}
}
