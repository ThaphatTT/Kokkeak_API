use std::sync::Arc;

use async_trait::async_trait;
use kokkak_application::auth::AuthService;
use kokkak_application::catalog::CatalogService;
use kokkak_application::category_job_main::CategoryJobMainService;
use kokkak_application::category_job_service_main::CategoryJobServiceMainService;
use kokkak_application::category_job_service_sub::CategoryJobServiceSubService;
use kokkak_application::category_job_service_sub_fee::CategoryJobServiceSubFeeService;
use kokkak_application::category_job_service_sub_warranty::CategoryJobServiceSubWarrantyService;
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
use kokkak_infra::permission_checker::PermissionChecker;

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

#[derive(Clone)]
pub struct AppState {
    pub auth: Arc<AuthService>,

    pub user: Arc<UserService>,

    pub admin_users: Arc<kokkak_application::admin_user::AdminUserService>,

    pub catalog: Arc<CatalogService>,

    pub category_job_main: Arc<CategoryJobMainService>,

    pub category_job_service_main: Arc<CategoryJobServiceMainService>,

    pub category_job_service_sub: Arc<CategoryJobServiceSubService>,

    pub category_job_service_sub_fee: Arc<CategoryJobServiceSubFeeService>,

    pub category_job_service_sub_warranty: Arc<CategoryJobServiceSubWarrantyService>,

    pub master: Arc<MasterDropdownService>,

    pub orders: Arc<OrderService>,

    pub chat: ChatHandle,

    pub payments: Arc<PaymentService>,

    pub permission: Arc<PermissionUserService>,

    pub user_roles: Arc<UserRoleService>,

    pub jwt: Arc<JwtService>,

    pub health: HealthRegistry,

    pub users: Arc<UserService>,

    pub translation: Arc<dyn TranslationRepository>,

    pub settings: Arc<Settings>,

    pub storage: Arc<dyn Storage>,

    pub image: Arc<ImageProcessor>,

    pub permission_checker: Arc<PermissionChecker>,

    pub public_base_url: Arc<str>,

    pub signed_url_secret: Arc<str>,

    pub signed_url_ttl_secs: u32,
}

#[derive(Clone)]
pub struct ChatHandle {
    pub service: Arc<ChatService>,

    pub local: Arc<BroadcastTransport>,
}

impl ChatHandle {
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

    pub fn repo(&self) -> &Arc<dyn ChatRepository> {
        self.service.repo()
    }

    pub async fn check_membership(
        &self,
        room_id: kokkak_domain::RoomId,
        user_id: uuid::Uuid,
    ) -> Result<bool, kokkak_domain::ChatRepoError> {
        let repo: &Arc<dyn ChatRepository> = self.service.repo();

        ChatMembershipBridge::is_participant(&**repo, room_id, user_id).await
    }

    pub fn local_transport(&self) -> &Arc<BroadcastTransport> {
        &self.local
    }
}

impl AppState {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        auth: Arc<AuthService>,
        user: Arc<UserService>,
        admin_users: Arc<kokkak_application::admin_user::AdminUserService>,
        catalog: Arc<CatalogService>,
        category_job_main: Arc<CategoryJobMainService>,
        category_job_service_main: Arc<CategoryJobServiceMainService>,
        category_job_service_sub: Arc<CategoryJobServiceSubService>,
        category_job_service_sub_fee: Arc<CategoryJobServiceSubFeeService>,
        category_job_service_sub_warranty: Arc<CategoryJobServiceSubWarrantyService>,
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
        permission_checker: Arc<PermissionChecker>,
        public_base_url: Arc<str>,
        signed_url_secret: Arc<str>,
        signed_url_ttl_secs: u32,
    ) -> Self {
        Self {
            auth,
            user: user.clone(),
            admin_users,
            catalog,
            category_job_main,
            category_job_service_main,
            category_job_service_sub,
            category_job_service_sub_fee,
            category_job_service_sub_warranty,
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
            permission_checker,
            public_base_url,
            signed_url_secret,
            signed_url_ttl_secs,
        }
    }

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
        permission_checker: Arc<PermissionChecker>,
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
            category_job_main: Arc::new(CategoryJobMainService::disabled()),
            category_job_service_main: Arc::new(CategoryJobServiceMainService::disabled()),
            category_job_service_sub: Arc::new(CategoryJobServiceSubService::disabled()),
            category_job_service_sub_fee: Arc::new(CategoryJobServiceSubFeeService::disabled()),
            category_job_service_sub_warranty: Arc::new(
                CategoryJobServiceSubWarrantyService::disabled(),
            ),
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
            permission_checker,

            public_base_url: Arc::from(""),
            signed_url_secret: Arc::from(""),
            signed_url_ttl_secs: 600,
        }
    }
}
