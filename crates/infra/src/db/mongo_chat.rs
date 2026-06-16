//! MongoDB-backed `ChatRepository` (M8).
//!
//! Production adapter. Two collections, one transaction per
//! aggregate (M12+ uses MongoDB sessions when running against
//! a replica set; M8 ships the non-transactional path so the
//! adapter works against a single-node Mongo too).
//!
//! - `chat_rooms` — one document per room.
//! - `chat_messages` — one document per message; idempotent on
//!   `_id` so the worker's `chat.persist` re-deliveries are
//!   safe.

use async_trait::async_trait;
use bson::{doc, Document};
use chrono::{DateTime, Utc};
use futures::StreamExt;
use kokkak_domain::{
    ChatMessage, ChatRepoError, ChatRepository, ChatRoom, MessagePage, Participant, RoomId,
    RoomSummary, User,
};
use mongodb::options::FindOptions;
use mongodb::{Collection, Database};
use std::collections::BTreeSet;
use uuid::Uuid;

use crate::db::mongo::MongoClient;

const COLL_ROOMS: &str = "chat_rooms";
const COLL_MESSAGES: &str = "chat_messages";

/// Production chat repository backed by MongoDB.
#[derive(Clone)]
pub struct MongoChatRepository {
    rooms: Collection<Document>,
    messages: Collection<Document>,
}

impl MongoChatRepository {
    /// Build from a connected `MongoClient`.
    pub fn new(client: &MongoClient) -> Self {
        let db: &Database = client.database();
        Self {
            rooms: db.collection(COLL_ROOMS),
            messages: db.collection(COLL_MESSAGES),
        }
    }

    /// Build from a `Database` directly (used by tests with
    /// `testcontainers`).
    pub fn from_db(db: &Database) -> Self {
        Self {
            rooms: db.collection(COLL_ROOMS),
            messages: db.collection(COLL_MESSAGES),
        }
    }
}

fn room_to_doc(r: &ChatRoom) -> Document {
    let participants: Vec<Document> = r
        .participants
        .iter()
        .map(|p| {
            doc! {
                "user_id": p.user_id.to_string(),
                "role": p.role.as_str(),
            }
        })
        .collect();
    doc! {
        "_id": r.id.to_string(),
        "participants": participants,
        "created_at": bson::DateTime::from_chrono(r.created_at),
        "last_msg_at": bson::DateTime::from_chrono(r.last_msg_at),
        "title": r.title.clone(),
    }
}

fn room_from_doc(d: Document) -> Result<ChatRoom, ChatRepoError> {
    let id_s = d
        .get_str("_id")
        .map_err(|e| ChatRepoError::Backend(e.to_string()))?;
    let id: Uuid = id_s
        .parse()
        .map_err(|e| ChatRepoError::Backend(format!("bad uuid: {e}")))?;
    let participants: Vec<Participant> = d
        .get_array("participants")
        .map_err(|e| ChatRepoError::Backend(e.to_string()))?
        .iter()
        .filter_map(|v| v.as_document())
        .filter_map(|p| {
            let uid_s = p.get_str("user_id").ok()?;
            let uid = uid_s.parse().ok()?;
            let role = match p.get_str("role").ok()? {
                "customer" => kokkak_domain::Role::Customer,
                "technician" => kokkak_domain::Role::Technician,
                "admin" => kokkak_domain::Role::Admin,
                "super_admin" => kokkak_domain::Role::SuperAdmin,
                _ => return None,
            };
            Some(Participant { user_id: uid, role })
        })
        .collect();
    let created_at = d
        .get_datetime("created_at")
        .map_err(|e| ChatRepoError::Backend(e.to_string()))?
        .to_chrono();
    let last_msg_at = d
        .get_datetime("last_msg_at")
        .map_err(|e| ChatRepoError::Backend(e.to_string()))?
        .to_chrono();
    let title = d.get_str("title").ok().map(|s| s.to_string());
    Ok(ChatRoom {
        id,
        participants,
        created_at,
        last_msg_at,
        title,
    })
}

fn message_to_doc(m: &ChatMessage) -> Document {
    let read_by: Vec<Document> = m
        .read_by
        .iter()
        .map(|(uid, at)| {
            doc! {
                "user_id": uid.to_string(),
                "at": bson::DateTime::from_chrono(*at),
            }
        })
        .collect();
    doc! {
        "_id": m.id.to_string(),
        "room_id": m.room_id.to_string(),
        "sender_id": m.sender_id.to_string(),
        "body": &m.body,
        "sent_at": bson::DateTime::from_chrono(m.sent_at),
        "read_by": read_by,
    }
}

fn message_from_doc(d: Document) -> Result<ChatMessage, ChatRepoError> {
    let id_s = d
        .get_str("_id")
        .map_err(|e| ChatRepoError::Backend(e.to_string()))?;
    let id: Uuid = id_s
        .parse()
        .map_err(|e| ChatRepoError::Backend(format!("bad uuid: {e}")))?;
    let room_s = d
        .get_str("room_id")
        .map_err(|e| ChatRepoError::Backend(e.to_string()))?;
    let room_id: Uuid = room_s
        .parse()
        .map_err(|e| ChatRepoError::Backend(format!("bad uuid: {e}")))?;
    let sender_s = d
        .get_str("sender_id")
        .map_err(|e| ChatRepoError::Backend(e.to_string()))?;
    let sender_id: Uuid = sender_s
        .parse()
        .map_err(|e| ChatRepoError::Backend(format!("bad uuid: {e}")))?;
    let body = d
        .get_str("body")
        .map_err(|e| ChatRepoError::Backend(e.to_string()))?
        .to_string();
    let sent_at = d
        .get_datetime("sent_at")
        .map_err(|e| ChatRepoError::Backend(e.to_string()))?
        .to_chrono();
    let read_by: Vec<(Uuid, DateTime<Utc>)> = d
        .get_array("read_by")
        .map_err(|e| ChatRepoError::Backend(e.to_string()))?
        .iter()
        .filter_map(|v| v.as_document())
        .filter_map(|r| {
            let uid = r.get_str("user_id").ok()?.parse().ok()?;
            let at = r.get_datetime("at").ok()?.to_chrono();
            Some((uid, at))
        })
        .collect();
    Ok(ChatMessage {
        id,
        room_id,
        sender_id,
        body,
        sent_at,
        read_by,
    })
}

#[async_trait]
impl ChatRepository for MongoChatRepository {
    async fn find_room(&self, room_id: RoomId) -> Result<Option<ChatRoom>, ChatRepoError> {
        let d = self
            .rooms
            .find_one(doc! { "_id": room_id.to_string() })
            .await
            .map_err(|e| ChatRepoError::Backend(e.to_string()))?;
        match d {
            Some(doc) => room_from_doc(doc).map(Some),
            None => Ok(None),
        }
    }

    async fn find_room_by_participants(
        &self,
        participants: &[Uuid],
    ) -> Result<Option<ChatRoom>, ChatRepoError> {
        let want: BTreeSet<Uuid> = participants.iter().copied().collect();
        if want.len() < 2 {
            return Ok(None);
        }
        let uids: Vec<bson::Bson> = want.iter().map(|u| bson::Bson::String(u.to_string())).collect();
        // Find any room that contains all of these user_ids.
        let mut cur = self
            .rooms
            .find(doc! { "participants.user_id": { "$all": uids } })
            .await
            .map_err(|e| ChatRepoError::Backend(e.to_string()))?;
        while let Some(d) = cur.next().await {
            let d = d.map_err(|e| ChatRepoError::Backend(e.to_string()))?;
            let room = room_from_doc(d)?;
            let got: BTreeSet<Uuid> = room.participants.iter().map(|p| p.user_id).collect();
            if got == want {
                return Ok(Some(room));
            }
        }
        Ok(None)
    }

    async fn create_room(&self, room: &ChatRoom) -> Result<(), ChatRepoError> {
        self.rooms
            .insert_one(room_to_doc(room))
            .await
            .map_err(|e| ChatRepoError::Backend(e.to_string()))?;
        Ok(())
    }

    async fn touch_room(
        &self,
        room_id: RoomId,
        last_msg_at: DateTime<Utc>,
    ) -> Result<(), ChatRepoError> {
        self.rooms
            .update_one(
                doc! { "_id": room_id.to_string() },
                doc! { "$set": { "last_msg_at": bson::DateTime::from_chrono(last_msg_at) } },
            )
            .await
            .map_err(|e| ChatRepoError::Backend(e.to_string()))?;
        Ok(())
    }

    async fn list_rooms_for_user(
        &self,
        user_id: Uuid,
        limit: u32,
    ) -> Result<Vec<RoomSummary>, ChatRepoError> {
        let opts = FindOptions::builder()
            .sort(doc! { "last_msg_at": -1 })
            .limit(limit.clamp(1, 200) as i64)
            .build();
        let mut cur = self
            .rooms
            .find(doc! { "participants.user_id": user_id.to_string() })
            .with_options(opts)
            .await
            .map_err(|e| ChatRepoError::Backend(e.to_string()))?;
        let mut out = Vec::new();
        while let Some(d) = cur.next().await {
            let d = d.map_err(|e| ChatRepoError::Backend(e.to_string()))?;
            let room = room_from_doc(d.clone())?;
            // Latest message.
            let msg_opts = FindOptions::builder()
                .sort(doc! { "sent_at": -1 })
                .limit(1)
                .build();
            let mut msg_cur = self
                .messages
                .find(doc! { "room_id": room.id.to_string() })
                .with_options(msg_opts)
                .await
                .map_err(|e| ChatRepoError::Backend(e.to_string()))?;
            let last = match msg_cur.next().await {
                Some(Ok(d)) => Some(message_from_doc(d)?),
                _ => None,
            };
            // Unread count: messages whose sender is not me and
            // that have no read receipt for me.
            let unread_filter = doc! {
                "room_id": room.id.to_string(),
                "sender_id": { "$ne": user_id.to_string() },
                "read_by.user_id": { "$ne": user_id.to_string() },
            };
            let unread = self
                .messages
                .count_documents(unread_filter)
                .await
                .map_err(|e| ChatRepoError::Backend(e.to_string()))? as u32;
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
        let mut filter = doc! { "room_id": room_id.to_string() };
        if let Some(b) = page.before {
            filter.insert("sent_at", doc! { "$lt": bson::DateTime::from_chrono(b) });
        }
        let opts = FindOptions::builder()
            .sort(doc! { "sent_at": -1 })
            .limit(page.limit.clamp(1, 200) as i64)
            .build();
        let mut cur = self
            .messages
            .find(filter)
            .with_options(opts)
            .await
            .map_err(|e| ChatRepoError::Backend(e.to_string()))?;
        let mut out = Vec::new();
        while let Some(d) = cur.next().await {
            let d = d.map_err(|e| ChatRepoError::Backend(e.to_string()))?;
            out.push(message_from_doc(d)?);
        }
        Ok(out)
    }

    async fn insert_message(&self, message: &ChatMessage) -> Result<(), ChatRepoError> {
        // Idempotent on _id (MessageId).
        self.messages
            .insert_one(message_to_doc(message))
            .await
            .map_err(|e| ChatRepoError::Backend(e.to_string()))?;
        Ok(())
    }

    async fn mark_read(
        &self,
        room_id: RoomId,
        user_id: Uuid,
        at: DateTime<Utc>,
    ) -> Result<(), ChatRepoError> {
        // Append a read receipt only to messages that don't
        // already have one for this user.
        let filter = doc! {
            "room_id": room_id.to_string(),
            "sender_id": { "$ne": user_id.to_string() },
            "read_by.user_id": { "$ne": user_id.to_string() },
        };
        let update = doc! {
            "$push": { "read_by": { "user_id": user_id.to_string(), "at": bson::DateTime::from_chrono(at) } }
        };
        self.messages
            .update_many(filter, update)
            .await
            .map_err(|e| ChatRepoError::Backend(e.to_string()))?;
        Ok(())
    }
}

#[allow(dead_code)]
fn _user_touch(_: &User) {}
