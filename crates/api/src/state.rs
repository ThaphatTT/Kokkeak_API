//! API state (AppState) — DI container for handlers.

use std::sync::Arc;

use async_trait::async_trait;
use kokkak_application::auth::AuthService;
use kokkak_application::catalog::CatalogService;
use kokkak_application::chat::{BroadcastTransport, ChatService};
use kokkak_application::master::MasterDropdownService;
use kokkak_application::order::OrderService;
use kokkak_application::payment::PaymentService;
use kokkak_application::permission::PermissionUserService;
use kokkak_application::user::UserService;
use kokkak_application::user_role::UserRoleService;
use kokkak_common::config::Settings;
use kokkak_domain::{
    ChatMembership, ChatRepoError, ChatRepository, HealthRegistry, Storage, TranslationRepository,
};
use kokkak_infra::auth::jwt::JwtService;
use kokkak_infra::image_processor::ImageProcessor;

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
    /// M20-b: admin user use cases (`POST /api/v1/admin/users/full`).
    /// Kept separate from `AuthService::register` because the
    /// rich admin-provisioning flow wraps a different SP and has
    /// its own password-hashing + actor-lookup flow.
    pub admin_users: Arc<kokkak_application::admin_user::AdminUserService>,
    /// Catalog use cases.
    pub catalog: Arc<CatalogService>,
    /// M20: Master-data dropdown use cases (country dropdown first;
    /// provinces / banks / etc. land here as `list_provinces` etc.).
    /// Same endpoint serves mobile / customer web / admin web.
    pub master: Arc<MasterDropdownService>,
    /// Order use cases.
    pub orders: Arc<OrderService>,
    /// Chat use cases (M8).
    pub chat: ChatHandle,
    /// Payment use cases (M9).
    pub payments: Arc<PaymentService>,
    /// Permission-page use cases (`GET /api/v1/permission/...`).
    ///
    /// Strictly isolated from the admin user-management flow —
    /// separate route prefix + separate service + separate handler,
    /// even though both call the same SQL Server SPs today. See
    /// [`kokkak_application::permission`] for the rationale.
    pub permission: Arc<PermissionUserService>,
    /// M15-prep: role × permission use cases (`GET /api/v1/admin/permissions`).
    pub user_roles: Arc<UserRoleService>,
    /// JWT service (for extractor verification).
    pub jwt: Arc<JwtService>,
    /// Health registry for `/readyz`.
    pub health: HealthRegistry,
    /// `users` (UserService re-exposed for handlers that need
    /// a full `User` view from the auth session, like chat).
    pub users: Arc<UserService>,
    /// Per-tenant translation override store (M11). Looked up by
    /// every localized error response; the locale is set by
    /// [`crate::middleware::i18n::locale_middleware`].
    pub translation: Arc<dyn TranslationRepository>,
    /// T-31: feature flags + middleware config that the
    /// Strangler gates (`feature_gate::*`) read on every request.
    /// Wrapped in Arc so the same instance is shared with the
    /// runtime config + main loop without copying.
    pub settings: Arc<Settings>,

    /// M9 / T-16: object storage adapter. S3 in prod, local FS
    /// during the Strangler transition, in-memory for unit
    /// tests. Selected at startup from `KOKKAK_STORAGE__*`.
    pub storage: Arc<dyn Storage>,

    /// M9 / T-16 extra: image processor (decode → WebP → store).
    /// Constructed from [`Settings::storage`] +
    /// [`Settings::image`] in `build_app_state_with`. The admin
    /// user-full handler calls
    /// `state.image.process_and_store(...)` for each
    /// `*_img_b64` field the caller sent.
    pub image: Arc<ImageProcessor>,
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
    /// [`crate::build_app_state_with`] from `main` and tests; this
    /// constructor is kept public for advanced wiring (e.g. a
    /// test that wants to swap the chat transport).
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        auth: Arc<AuthService>,
        user: Arc<UserService>,
        admin_users: Arc<kokkak_application::admin_user::AdminUserService>,
        catalog: Arc<CatalogService>,
        master: Arc<MasterDropdownService>,
        orders: Arc<OrderService>,
        chat: ChatHandle,
        payments: Arc<PaymentService>,
        permission: Arc<PermissionUserService>,
        user_roles: Arc<UserRoleService>,
        jwt: Arc<JwtService>,
        health: HealthRegistry,
        translation: Arc<dyn TranslationRepository>,
        settings: Arc<Settings>,
        storage: Arc<dyn Storage>,
        image: Arc<ImageProcessor>,
    ) -> Self {
        Self {
            auth,
            user: user.clone(),
            admin_users,
            catalog,
            master,
            orders,
            chat,
            payments,
            permission,
            user_roles,
            jwt,
            health,
            users: user,
            translation,
            settings,
            storage,
            image,
        }
    }

    /// Backwards-compat: a state without chat / payments.
    /// M14.5: removed JSON-DB sim. `legacy()` now requires the caller
    /// to pass chat + payments already built against MSSQL.
    #[allow(clippy::too_many_arguments)]
    pub fn legacy(
        auth: Arc<AuthService>,
        user: Arc<UserService>,
        admin_users: Arc<kokkak_application::admin_user::AdminUserService>,
        catalog: Arc<CatalogService>,
        master: Arc<MasterDropdownService>,
        orders: Arc<OrderService>,
        chat: Arc<ChatService>,
        payments: Arc<PaymentService>,
        permission: Arc<PermissionUserService>,
        user_roles: Arc<UserRoleService>,
        jwt: Arc<JwtService>,
        health: HealthRegistry,
        translation: Arc<dyn TranslationRepository>,
        settings: Arc<Settings>,
        storage: Arc<dyn Storage>,
        image: Arc<ImageProcessor>,
    ) -> Self {
        let chat_handle = ChatHandle {
            service: chat,
            local: Arc::new(BroadcastTransport::default()),
        };
        Self {
            auth,
            user: user.clone(),
            admin_users,
            catalog,
            master,
            orders,
            chat: chat_handle,
            payments,
            permission,
            user_roles,
            jwt,
            health,
            users: user,
            translation,
            settings,
            storage,
            image,
        }
    }
}
