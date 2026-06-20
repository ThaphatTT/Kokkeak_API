//! Chat HTTP handlers (M8 + M11 i18n).
//!
//! - `GET  /api/v1/chat/rooms` — inbox (list rooms for user).
//! - `POST /api/v1/chat/rooms` — open (or return existing) 1:1 room.
//! - `GET  /api/v1/chat/rooms/:id/messages?after=&limit=` — list messages.
//! - `POST /api/v1/chat/rooms/:id/messages` — send a message.
//! - `POST /api/v1/chat/rooms/:id/read` — append a read receipt.
//!
//! The WebSocket gateway is in `super::ws`. User-visible error
//! strings are rendered via `kokkak_common::i18n::tr_with_repo`
//! against the per-tenant `TranslationRepository`; the
//! file-based catalog is the fallback.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use chrono::{DateTime, Utc};
use kokkak_common::i18n::{current_locale, tr, tr_with_repo};
use kokkak_common::response::{paginated, ApiResponse, PageMeta};
use kokkak_domain::{
    ChatError, ChatMessage, ChatRoom, LocalizedError, Participant, RoomId, RoomSummary,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::middleware::auth::AuthnUser;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct ListRoomsQuery {
    pub limit: Option<u32>,
}

/// Room summary response item.
#[derive(Debug, Serialize)]
pub struct RoomSummaryDto {
    pub room: ChatRoom,
    pub last_message: Option<ChatMessage>,
    pub unread: u32,
}

impl From<RoomSummary> for RoomSummaryDto {
    fn from(s: RoomSummary) -> Self {
        Self {
            room: s.room,
            last_message: s.last_message,
            unread: s.unread,
        }
    }
}

/// GET /api/v1/chat/rooms — list the user's inbox.
pub async fn list_rooms(
    State(state): State<AppState>,
    user: AuthnUser,
    Query(q): Query<ListRoomsQuery>,
) -> Result<Response, Response> {
    // We do not require any specific role: customers +
    // technicians + admin can all chat.
    let limit = q.limit.unwrap_or(20);
    let user = match state.users.get_user(user.id()).await {
        Ok(u) => u,
        Err(e) => {
            let locale = current_locale();
            let args: Vec<String> = e.l10n_args();
            let args_ref: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
            let msg = tr_with_repo(&*state.translation, &locale, e.l10n_key(), &args_ref).await;
            return Ok(err_envelope(StatusCode::NOT_FOUND, "not_found", msg));
        }
    };
    let rooms = match state.chat.list_rooms_for(&user, limit).await {
        Ok(r) => r,
        Err(e) => return Err(chat_err_to_response(e, &state).await),
    };
    let items: Vec<RoomSummaryDto> = rooms.into_iter().map(RoomSummaryDto::from).collect();
    let meta = PageMeta {
        limit: limit as usize,
        has_next: items.len() as u32 == limit,
        next_cursor: None,
    };
    Ok((StatusCode::OK, paginated(items, meta)).into_response())
}

#[derive(Debug, Deserialize)]
pub struct OpenRoomRequest {
    pub other_user_id: Uuid,
    pub other_role: String,
}

/// POST /api/v1/chat/rooms — find-or-create a 1:1 room.
pub async fn open_room(
    State(state): State<AppState>,
    user: AuthnUser,
    Json(req): Json<OpenRoomRequest>,
) -> Result<Response, Response> {
    let my_id = user.id();
    let role_str = req.other_role.to_lowercase();
    let other_role = match role_str.as_str() {
        "customer" => kokkak_domain::Role::Customer,
        "technician" => kokkak_domain::Role::Technician,
        "admin" => kokkak_domain::Role::Admin,
        "super_admin" | "superadmin" => kokkak_domain::Role::SuperAdmin,
        _ => {
            let locale = current_locale();
            let msg = tr("err_chat.bad_other_role", &locale, &[]);
            return Ok(err_envelope(StatusCode::BAD_REQUEST, "bad_request", msg));
        }
    };
    let participants = vec![
        Participant {
            user_id: my_id,
            role: my_role(&user),
        },
        Participant {
            user_id: req.other_user_id,
            role: other_role,
        },
    ];
    let room = match state.chat.open_room(participants).await {
        Ok(r) => r,
        Err(e) => return Err(chat_err_to_response(e, &state).await),
    };
    Ok((
        StatusCode::CREATED,
        Json(ApiResponse {
            success: true,
            data: Some(room),
            error: None,
            meta: None,
        }),
    )
        .into_response())
}

#[derive(Debug, Deserialize)]
pub struct ListMessagesQuery {
    pub after: Option<String>,
    pub limit: Option<u32>,
}

#[derive(Debug, Serialize)]
pub struct MessageDto {
    pub id: Uuid,
    pub room_id: Uuid,
    pub sender_id: Uuid,
    pub body: String,
    pub sent_at: DateTime<Utc>,
    pub read_by: Vec<(Uuid, DateTime<Utc>)>,
}

impl From<ChatMessage> for MessageDto {
    fn from(m: ChatMessage) -> Self {
        Self {
            id: m.id,
            room_id: m.room_id,
            sender_id: m.sender_id,
            body: m.body,
            sent_at: m.sent_at,
            read_by: m.read_by,
        }
    }
}

/// GET /api/v1/chat/rooms/:id/messages — list messages in a room.
pub async fn list_messages(
    State(state): State<AppState>,
    user: AuthnUser,
    Path(room_id): Path<Uuid>,
    Query(q): Query<ListMessagesQuery>,
) -> Result<Response, Response> {
    let limit = q.limit.unwrap_or(50);
    let before = match q.after.as_deref() {
        Some(s) => match DateTime::parse_from_rfc3339(s) {
            Ok(t) => Some(t.with_timezone(&Utc)),
            Err(_) => {
                let locale = current_locale();
                let msg = tr("err_request.bad_timestamp", &locale, &[]);
                return Ok(err_envelope(StatusCode::BAD_REQUEST, "bad_request", msg));
            }
        },
        None => None,
    };
    let user = match state.users.get_user(user.id()).await {
        Ok(u) => u,
        Err(e) => {
            let locale = current_locale();
            let args: Vec<String> = e.l10n_args();
            let args_ref: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
            let msg = tr_with_repo(&*state.translation, &locale, e.l10n_key(), &args_ref).await;
            return Ok(err_envelope(StatusCode::NOT_FOUND, "not_found", msg));
        }
    };
    let msgs = match state
        .chat
        .list_messages(room_id, &user, before, limit)
        .await
    {
        Ok(m) => m,
        Err(e) => return Err(chat_err_to_response(e, &state).await),
    };
    let items: Vec<MessageDto> = msgs.into_iter().map(MessageDto::from).collect();
    let meta = PageMeta {
        limit: limit as usize,
        has_next: items.len() as u32 == limit,
        next_cursor: items.last().map(|m| m.sent_at.to_rfc3339()),
    };
    Ok((StatusCode::OK, paginated(items, meta)).into_response())
}

#[derive(Debug, Deserialize)]
pub struct SendMessageRequest {
    pub body: String,
}

/// POST /api/v1/chat/rooms/:id/messages — send a message.
pub async fn send_message(
    State(state): State<AppState>,
    user: AuthnUser,
    Path(room_id): Path<Uuid>,
    Json(req): Json<SendMessageRequest>,
) -> Result<Response, Response> {
    let msg = match state.chat.send_message(room_id, user.id(), req.body).await {
        Ok(m) => m,
        Err(e) => return Err(chat_err_to_response(e, &state).await),
    };
    Ok((
        StatusCode::CREATED,
        Json(ApiResponse {
            success: true,
            data: Some(MessageDto::from(msg)),
            error: None,
            meta: None,
        }),
    )
        .into_response())
}

/// POST /api/v1/chat/rooms/:id/read — append a read receipt.
pub async fn mark_read(
    State(state): State<AppState>,
    user: AuthnUser,
    Path(room_id): Path<Uuid>,
) -> Result<Response, Response> {
    let user = match state.users.get_user(user.id()).await {
        Ok(u) => u,
        Err(e) => {
            let locale = current_locale();
            let args: Vec<String> = e.l10n_args();
            let args_ref: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
            let msg = tr_with_repo(&*state.translation, &locale, e.l10n_key(), &args_ref).await;
            return Ok(err_envelope(StatusCode::NOT_FOUND, "not_found", msg));
        }
    };
    if let Err(e) = state.chat.mark_read(room_id, &user).await {
        return Err(chat_err_to_response(e, &state).await);
    }
    Ok((
        StatusCode::OK,
        Json(ApiResponse::<()> {
            success: true,
            data: None,
            error: None,
            meta: None,
        }),
    )
        .into_response())
}

fn my_role(user: &AuthnUser) -> kokkak_domain::Role {
    if user.has_role(kokkak_domain::Role::SuperAdmin) {
        kokkak_domain::Role::SuperAdmin
    } else if user.has_role(kokkak_domain::Role::Admin) {
        kokkak_domain::Role::Admin
    } else if user.has_role(kokkak_domain::Role::Technician) {
        kokkak_domain::Role::Technician
    } else {
        kokkak_domain::Role::Customer
    }
}

fn err_envelope(status: StatusCode, code: &str, message: String) -> Response {
    let envelope: ApiResponse<()> = ApiResponse {
        success: false,
        data: None,
        error: Some(kokkak_common::error::ApiErrorBody {
            code: code.into(),
            message,
        }),
        meta: None,
    };
    (status, Json(envelope)).into_response()
}

async fn chat_err_to_response(e: ChatError, state: &AppState) -> Response {
    use ChatError::*;
    let (status, code) = match &e {
        NotParticipant(_) => (StatusCode::FORBIDDEN, "forbidden"),
        RoomNotFound(_) => (StatusCode::NOT_FOUND, "not_found"),
        InvalidBody(_) => (StatusCode::UNPROCESSABLE_ENTITY, "validation"),
        Backend(_) => (StatusCode::INTERNAL_SERVER_ERROR, "internal"),
    };
    let locale = current_locale();
    let args: Vec<String> = e.l10n_args();
    let args_ref: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let message = tr_with_repo(&*state.translation, &locale, e.l10n_key(), &args_ref).await;
    err_envelope(status, code, message)
}

/// Borrow the room id type.
pub type RoomIdAlias = RoomId;
