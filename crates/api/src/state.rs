//! API state (AppState) — DI container for handlers.

use std::sync::Arc;

use async_trait::async_trait;
use kokkak_application::auth::AuthService;
use kokkak_application::catalog::CatalogService;
use kokkak_application::chat::{BroadcastTransport, ChatService, ChatTransport};
use kokkak_application::order::OrderService;
use kokkak_application::payment::PaymentService;
use kokkak_application::user::UserService;
use kokkak_domain::{ChatMembership, ChatRepoError, ChatRepository, HealthRegistry};
use kokkak_infra::auth::jwt::JwtService;

/// Internal bridge: takes a `&dyn ChatRepository` and exposes
/// `is_participant` via the blanket `ChatMembership for T: ChatRepository`
/// impl. Avoids the trait-upcast limitation in current Rust.
struct ChatMembershipBridge;

#[async_trait]
impl ChatMembershipBridgeTrait for ChatMembershipBridge {
    async fn is_participant(
        repo: &dyn ChatRepository,
        room_id: kokkak_domain::RoomId,
        user_id: uuid::Uuid,
    ) -> Result<bool, ChatRepoError> {
        <dyn ChatRepository as ChatMembership>::is_participant(repo, room_id, user_id).await
    }
}

#[async_trait]
trait ChatMembershipBridgeTrait: Send + Sync {
    async fn is_participant(
        repo: &dyn ChatRepository,
        room_id: kokkak_domain::RoomId,
        user_id: uuid::Uuid,
    ) -> Result<bool, ChatRepoError>;
}

/// Shared application state injected into every handler.
#[derive(Clone)]
pub struct AppState {
    /// Auth use cases (register, login, refresh).
    pub auth: Arc<AuthService>,
    /// User use cases.
    pub user: Arc<UserService>,
    /// Catalog use cases.
    pub catalog: Arc<CatalogService>,
    /// Order use cases.
    pub orders: Arc<OrderService>,
    /// Chat use cases (M8).
    pub chat: ChatHandle,
    /// Payment use cases (M9).
    pub payments: Arc<PaymentService>,
    /// JWT service (for extractor verification).
    pub jwt: Arc<JwtService>,
    /// Health registry for `/readyz`.
    pub health: HealthRegistry,
    /// `users` (UserService re-exposed for handlers that need
    /// a full `User` view from the auth session, like chat).
    pub users: Arc<UserService>,
}

/// Chat state bundle — the service + the local broadcast
/// transport (for the WebSocket gateway).
#[derive(Clone)]
pub struct ChatHandle {
    /// Use case service.
    pub service: Arc<ChatService>,
    /// Local-only transport (for the WebSocket gateway). In
    /// production this is the same `BroadcastTransport` that
    /// the `RedisChatPubSub` bridge wraps.
    pub local: Arc<BroadcastTransport>,
}

impl ChatHandle {
    /// Forward to the service's `list_rooms_for` (handler-friendly).
    pub async fn list_rooms_for(
        &self,
        user: &kokkak_domain::User,
        limit: u32,
    ) -> Result<Vec<kokkak_domain::RoomSummary>, kokkak_domain::ChatError> {
        self.service.list_rooms_for(user, limit).await
    }

    pub async fn open_room(
        &self,
        participants: Vec<kokkak_domain::Participant>,
    ) -> Result<kokkak_domain::ChatRoom, kokkak_domain::ChatError> {
        self.service.open_room(participants).await
    }

    pub async fn list_messages(
        &self,
        room_id: kokkak_domain::RoomId,
        user: &kokkak_domain::User,
        before: Option<chrono::DateTime<chrono::Utc>>,
        limit: u32,
    ) -> Result<Vec<kokkak_domain::ChatMessage>, kokkak_domain::ChatError> {
        self.service
            .list_messages(room_id, user, before, limit)
            .await
    }

    pub async fn send_message(
        &self,
        room_id: kokkak_domain::RoomId,
        sender_id: uuid::Uuid,
        body: String,
    ) -> Result<kokkak_domain::ChatMessage, kokkak_domain::ChatError> {
        self.service.send_message(room_id, sender_id, body).await
    }

    pub async fn mark_read(
        &self,
        room_id: kokkak_domain::RoomId,
        user: &kokkak_domain::User,
    ) -> Result<(), kokkak_domain::ChatError> {
        self.service.mark_read(room_id, user).await
    }

    /// Borrow the chat repository (for the WebSocket gateway's
    /// membership check).
    pub fn repo(&self) -> &Arc<dyn ChatRepository> {
        self.service.repo()
    }

    /// Check if `user_id` is a participant of `room_id` via
    /// the underlying `ChatMembership` port.
    pub async fn check_membership(
        &self,
        room_id: kokkak_domain::RoomId,
        user_id: uuid::Uuid,
    ) -> Result<bool, kokkak_domain::ChatRepoError> {
        let repo: &Arc<dyn ChatRepository> = self.service.repo();
        // Re-route through a small helper that uses the
        // blanket `ChatMembership for T: ChatRepository` impl
        // (defined in `kokkak_domain::traits::chat`). Casting
        // `Arc<dyn ChatRepository>` to `&dyn ChatMembership`
        // would be a direct trait upcast; we sidestep that by
        // using a generic helper below.
        ChatMembershipBridge::is_participant(&**repo, room_id, user_id).await
    }

    /// Borrow the local broadcast transport (for the WebSocket
    /// gateway's per-connection subscriber).
    pub fn local_transport(&self) -> &Arc<BroadcastTransport> {
        &self.local
    }
}

impl AppState {
    /// Build the AppState from its parts. Use
    /// [`crate::build_app_state`] from `main` and tests; this
    /// constructor is kept public for advanced wiring (e.g. a
    /// test that wants to swap the chat transport).
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        auth: Arc<AuthService>,
        user: Arc<UserService>,
        catalog: Arc<CatalogService>,
        orders: Arc<OrderService>,
        chat: ChatHandle,
        payments: Arc<PaymentService>,
        jwt: Arc<JwtService>,
        health: HealthRegistry,
    ) -> Self {
        Self {
            auth,
            user: user.clone(),
            catalog,
            orders,
            chat,
            payments,
            jwt,
            health,
            users: user,
        }
    }

    /// Backwards-compat: a state without chat / payments.
    pub fn legacy(
        auth: Arc<AuthService>,
        user: Arc<UserService>,
        catalog: Arc<CatalogService>,
        orders: Arc<OrderService>,
        jwt: Arc<JwtService>,
        health: HealthRegistry,
    ) -> Self {
        let transport: Arc<dyn ChatTransport> = Arc::new(BroadcastTransport::default());
        let repo: Arc<dyn ChatRepository> = Arc::new(
            kokkak_infra::db::json_chat::JsonChatRepository::open_in_memory()
                .unwrap_or_else(|_| panic!("in-memory chat always works")),
        );
        let chat_svc = Arc::new(ChatService::new(repo, transport));
        let chat = ChatHandle {
            service: chat_svc,
            local: Arc::new(BroadcastTransport::default()),
        };
        let orders_for_payments = orders.clone();
        let payments = Arc::new(PaymentService::new(
            Arc::new(
                kokkak_infra::db::json_payment::JsonPaymentRepository::open_in_memory()
                    .expect("in-memory payment always works"),
            ),
            orders_for_payments.orders_repo(),
        ));
        Self {
            auth,
            user: user.clone(),
            catalog,
            orders,
            chat,
            payments,
            jwt,
            health,
            users: user,
        }
    }
}
