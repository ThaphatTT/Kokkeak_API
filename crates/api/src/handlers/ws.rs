//! WebSocket chat gateway (M8 + M11 i18n).
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
//!
//! Error responses carry localized messages via
//! `kokkak_common::i18n::tr_with_repo`; the locale is set
//! per request by the i18n middleware, so the WS upgrade
//! benefits from the same per-tenant override path as the
//! REST endpoints.

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
use kokkak_common::i18n::{current_locale, tr, tr_with_repo};
use kokkak_common::response::ApiResponse;
use kokkak_domain::{AuthError, LocalizedError};
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
    let locale = current_locale();
    // 1. Verify the JWT.
    let claims = match state.jwt.verify(&q.token) {
        Ok(c) => c,
        Err(e) => {
            let msg = match &e {
                AuthError::TokenExpired => tr("err_auth.token_expired", &locale, &[]),
                _ => {
                    let args: Vec<String> = e.l10n_args();
                    let args_ref: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
                    tr_with_repo(&*state.translation, &locale, e.l10n_key(), &args_ref).await
                }
            };
            return ws_error_response(axum::http::StatusCode::UNAUTHORIZED, "unauthorized", msg);
        }
    };
    if claims.kind != kokkak_domain::TokenKind::Access {
        let msg = tr("err_auth.not_access_token", &locale, &[]);
        return ws_error_response(axum::http::StatusCode::UNAUTHORIZED, "unauthorized", msg);
    }
    // 2. Load the user.
    let user = match state.users.get_user(claims.sub).await {
        Ok(u) => u,
        Err(e) => {
            let args: Vec<String> = e.l10n_args();
            let args_ref: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
            let msg = tr_with_repo(&*state.translation, &locale, e.l10n_key(), &args_ref).await;
            return ws_error_response(axum::http::StatusCode::UNAUTHORIZED, "unauthorized", msg);
        }
    };
    // 3. Membership check.
    let is_member = match state.chat.check_membership(room_id, user.id).await {
        Ok(b) => b,
        Err(e) => {
            let args: Vec<String> = e.l10n_args();
            let args_ref: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
            let msg = tr_with_repo(&*state.translation, &locale, e.l10n_key(), &args_ref).await;
            return ws_error_response(
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                msg,
            );
        }
    };
    if !is_member {
        let msg = tr("err_chat.not_participant_msg", &locale, &[]);
        return ws_error_response(axum::http::StatusCode::FORBIDDEN, "forbidden", msg);
    }
    // 4. All checks passed: actually upgrade.
    let local = state.chat.local_transport().clone();
    let room_id_str = room_id.to_string();
    ws.on_upgrade(move |socket| async move {
        handle_socket(socket, local, room_id_str).await;
    })
    .into_response()
}

/// Render a localized error response during the WS upgrade
/// handshake. The response body is a JSON envelope in the
/// standard `ApiResponse` shape; clients that can't read JSON
/// (browsers running on the WS handshake) will surface the
/// status code as the upgrade failure reason.
fn ws_error_response(status: axum::http::StatusCode, code: &str, message: String) -> Response {
    let envelope: ApiResponse<()> = ApiResponse {
        success: false,
        data: None,
        error: Some(kokkak_common::error::ApiErrorBody {
            code: code.into(),
            message,
        }),
        meta: None,
    };
    (status, axum::Json(envelope)).into_response()
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
