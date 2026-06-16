//! WebSocket chat gateway (M8).
//!
//! `GET /api/v1/chat/ws/:room_id` upgrades the connection to a
//! WebSocket and pipes `ChatEvent`s from the local
//! `BroadcastTransport` to the client.
//!
//! Authentication: a `?token=<jwt>` query parameter is
//! required (browsers cannot set the `Authorization` header on
//! a WebSocket upgrade). The token is verified once at
//! upgrade time; subsequent frames are accepted unconditionally.
//!
//! Membership: the user must be a participant of the room.
//! Otherwise the upgrade is rejected with 403 *before* the
//! WS handshake — by returning a plain error response.

use std::sync::Arc;

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Path, Query, State,
    },
    response::{IntoResponse, Response},
};
use futures::{SinkExt, StreamExt};
use kokkak_application::BroadcastTransport;
use serde::Deserialize;
use uuid::Uuid;

use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct WsQuery {
    pub token: String,
}

/// GET /api/v1/chat/ws/:room_id — WebSocket upgrade.
pub async fn ws_upgrade(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    Path(room_id): Path<Uuid>,
    Query(q): Query<WsQuery>,
) -> Response {
    // 1. Verify the JWT.
    let claims = match state.jwt.verify(&q.token) {
        Ok(c) => c,
        Err(e) => {
            return (
                axum::http::StatusCode::UNAUTHORIZED,
                format!("invalid token: {e}"),
            )
                .into_response();
        }
    };
    if claims.kind != kokkak_domain::TokenKind::Access {
        return (axum::http::StatusCode::UNAUTHORIZED, "not an access token").into_response();
    }
    // 2. Load the user.
    let user = match state.users.get_user(claims.sub).await {
        Ok(u) => u,
        Err(e) => {
            return (
                axum::http::StatusCode::UNAUTHORIZED,
                format!("user not found: {e}"),
            )
                .into_response();
        }
    };
    // 3. Membership check.
    let is_member = match state.chat.check_membership(room_id.into(), user.id).await {
        Ok(b) => b,
        Err(e) => {
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                format!("membership check failed: {e}"),
            )
                .into_response();
        }
    };
    if !is_member {
        return (
            axum::http::StatusCode::FORBIDDEN,
            "not a participant of this room",
        )
            .into_response();
    }
    // 4. All checks passed: actually upgrade.
    let local = state.chat.local_transport().clone();
    let room_id_str = room_id.to_string();
    ws.on_upgrade(move |socket| async move {
        handle_socket(socket, local, room_id_str).await;
    })
    .into_response()
}

async fn handle_socket(socket: WebSocket, local: Arc<BroadcastTransport>, room_id_str: String) {
    let mut rx = local.subscribe();
    let (mut sink, mut stream) = socket.split();
    let send = async move {
        while let Ok(ev) = rx.recv().await {
            if ev.room_id.to_string() != room_id_str {
                continue;
            }
            let payload = match serde_json::to_string(&ev.message) {
                Ok(s) => s,
                Err(_) => continue,
            };
            if sink.send(Message::Text(payload)).await.is_err() {
                break;
            }
        }
    };
    let recv = async move {
        while let Some(msg) = stream.next().await {
            match msg {
                Ok(Message::Close(_)) | Err(_) => break,
                _ => {}
            }
        }
    };
    tokio::select! {
        _ = send => {}
        _ = recv => {}
    }
}
