pub mod adapters;
pub mod cert_watcher;
pub mod error;
pub mod extractors;
pub mod files;
pub mod handlers;
pub mod middleware;
pub mod openapi;
pub mod redirect;
pub mod repo_factory;
pub mod router;
pub mod signed_url;
pub mod state;
pub mod tls;

pub use repo_factory::{from_settings as build_repos, RepoBackend, RepoBundle};
pub use router::build as build_router;
pub use state::{AppState, ChatHandle};

use std::sync::Arc;

use kokkak_application::admin_user::AdminUserService;
use kokkak_application::audit::{AuditLogger, NoopAuditLogger};
use kokkak_application::auth::AuthService;
use kokkak_application::catalog::CatalogService;
use kokkak_application::category_job_main::CategoryJobMainService;
use kokkak_application::category_job_service_main::CategoryJobServiceMainService;
use kokkak_application::category_job_service_sub::CategoryJobServiceSubService;
use kokkak_application::category_job_service_sub_fee::CategoryJobServiceSubFeeService;
use kokkak_application::chat::{BroadcastTransport, ChatService, ChatTransport};
use kokkak_application::master::MasterDropdownService;
use kokkak_application::order::OrderService;
use kokkak_application::payment::PaymentService;
use kokkak_application::permission::PermissionUserService;
use kokkak_application::rate_limit::LoginRateLimiter;
use kokkak_application::user::UserService;
use kokkak_application::user_role::UserRoleService;
use kokkak_domain::{HealthRegistry, TranslationRepository};
use kokkak_infra::audit::FileAuditLogger;
use kokkak_infra::auth::jwt::JwtService;
use kokkak_infra::auth::rate_limit::InMemoryLoginRateLimiter;
use kokkak_infra::db::mssql::MssqlPool;
use kokkak_infra::image_processor::{ImageProcessor, ImageProcessorConfig};
use kokkak_infra::storage::MemoryStorage;

use adapters::{JwtIssuerAdapter, PasswordHasherAdapter};

#[allow(dead_code)]
struct MssqlCategoryJobMainRepositoryNoop;

#[async_trait::async_trait]
impl kokkak_domain::CategoryJobMainRepository for MssqlCategoryJobMainRepositoryNoop {
    async fn list(
        &self,
        _input: &kokkak_domain::CategoryJobMainListInput,
    ) -> Result<kokkak_domain::CategoryJobMainPage, kokkak_domain::RepoError> {
        Err(kokkak_domain::RepoError::Backend(
            "category_job_main repo not wired in build_app_state (set KOKKAK_DATABASE__SQLSERVER_URL)"
                .into(),
        ))
    }
    async fn create(
        &self,
        _input: &kokkak_domain::CategoryJobMainCreateInput,
    ) -> Result<kokkak_domain::CategoryJobMainCreateResult, kokkak_domain::RepoError> {
        Err(kokkak_domain::RepoError::Backend(
            "category_job_main repo not wired in build_app_state".into(),
        ))
    }
    async fn update(
        &self,
        _input: &kokkak_domain::CategoryJobMainUpdateInput,
    ) -> Result<kokkak_domain::CategoryJobMainUpdateResult, kokkak_domain::RepoError> {
        Err(kokkak_domain::RepoError::Backend(
            "category_job_main repo not wired in build_app_state".into(),
        ))
    }
    async fn delete(
        &self,
        _category_guid: &str,
        _actor_user_guid: &str,
    ) -> Result<kokkak_domain::CategoryJobMainDeleteResult, kokkak_domain::RepoError> {
        Err(kokkak_domain::RepoError::Backend(
            "category_job_main repo not wired in build_app_state".into(),
        ))
    }
    async fn autocomplete(
        &self,
        _input: &kokkak_domain::CategoryJobMainAutocompleteInput,
    ) -> Result<Vec<kokkak_domain::CategoryJobMainAutocompleteRow>, kokkak_domain::RepoError> {
        Err(kokkak_domain::RepoError::Backend(
            "category_job_main repo not wired in build_app_state".into(),
        ))
    }
}

#[allow(dead_code)]
struct MssqlCategoryJobServiceMainRepositoryNoop;

#[async_trait::async_trait]
impl kokkak_domain::CategoryJobServiceMainRepository for MssqlCategoryJobServiceMainRepositoryNoop {
    async fn list(
        &self,
        _input: &kokkak_domain::CategoryJobServiceMainListInput,
    ) -> Result<Vec<kokkak_domain::CategoryJobServiceMainRow>, kokkak_domain::RepoError> {
        Err(kokkak_domain::RepoError::Backend(
            "category_job_service_main repo not wired in build_app_state (set KOKKAK_DATABASE__SQLSERVER_URL)"
                .into(),
        ))
    }
    async fn create(
        &self,
        _input: &kokkak_domain::CategoryJobServiceMainCreateInput,
    ) -> Result<kokkak_domain::CategoryJobServiceMainCreateResult, kokkak_domain::RepoError> {
        Err(kokkak_domain::RepoError::Backend(
            "category_job_service_main repo not wired in build_app_state".into(),
        ))
    }
    async fn update(
        &self,
        _input: &kokkak_domain::CategoryJobServiceMainUpdateInput,
    ) -> Result<kokkak_domain::CategoryJobServiceMainUpdateResult, kokkak_domain::RepoError> {
        Err(kokkak_domain::RepoError::Backend(
            "category_job_service_main repo not wired in build_app_state".into(),
        ))
    }
    async fn delete(
        &self,
        _service_guid: &str,
        _actor_user_guid: &str,
    ) -> Result<kokkak_domain::CategoryJobServiceMainDeleteResult, kokkak_domain::RepoError> {
        Err(kokkak_domain::RepoError::Backend(
            "category_job_service_main repo not wired in build_app_state".into(),
        ))
    }
    async fn autocomplete(
        &self,
        _input: &kokkak_domain::CategoryJobServiceMainAutocompleteInput,
    ) -> Result<Vec<kokkak_domain::CategoryJobServiceMainAutocompleteRow>, kokkak_domain::RepoError>
    {
        Err(kokkak_domain::RepoError::Backend(
            "category_job_service_main repo not wired in build_app_state".into(),
        ))
    }
}

#[allow(dead_code)]
struct MssqlCategoryJobServiceSubRepositoryNoop;

#[async_trait::async_trait]
impl kokkak_domain::CategoryJobServiceSubRepository for MssqlCategoryJobServiceSubRepositoryNoop {
    async fn list(
        &self,
        _category_job_service_guid: &str,
        _keyword: Option<&str>,
        _status: Option<i32>,
        _locale: &str,
        _include_deleted: bool,
    ) -> Result<Vec<kokkak_domain::CategoryJobServiceSubRow>, kokkak_domain::RepoError> {
        Err(kokkak_domain::RepoError::Backend(
            "category_job_service_sub repo not wired in build_app_state (set KOKKAK_DATABASE__SQLSERVER_URL)"
                .into(),
        ))
    }
    async fn detail(
        &self,
        _category_job_service_sub_guid: &str,
    ) -> Result<kokkak_domain::CategoryJobServiceSubDetailBundle, kokkak_domain::RepoError> {
        Err(kokkak_domain::RepoError::Backend(
            "category_job_service_sub repo not wired in build_app_state".into(),
        ))
    }
    async fn list_images(
        &self,
        _category_job_service_sub_guid: &str,
    ) -> Result<Vec<kokkak_domain::CategoryJobServiceSubImageRow>, kokkak_domain::RepoError> {
        Err(kokkak_domain::RepoError::Backend(
            "category_job_service_sub repo not wired in build_app_state".into(),
        ))
    }
    async fn create(
        &self,
        _input: &kokkak_domain::CategoryJobServiceSubCreateInput,
    ) -> Result<kokkak_domain::CategoryJobServiceSubCreateResult, kokkak_domain::RepoError> {
        Err(kokkak_domain::RepoError::Backend(
            "category_job_service_sub repo not wired in build_app_state".into(),
        ))
    }
    async fn update(
        &self,
        _input: &kokkak_domain::CategoryJobServiceSubUpdateInput,
    ) -> Result<kokkak_domain::CategoryJobServiceSubUpdateResult, kokkak_domain::RepoError> {
        Err(kokkak_domain::RepoError::Backend(
            "category_job_service_sub repo not wired in build_app_state".into(),
        ))
    }
    async fn delete(
        &self,
        _category_job_service_sub_guid: &str,
        _actor_user_guid: &str,
    ) -> Result<kokkak_domain::CategoryJobServiceSubDeleteResult, kokkak_domain::RepoError> {
        Err(kokkak_domain::RepoError::Backend(
            "category_job_service_sub repo not wired in build_app_state".into(),
        ))
    }
    async fn create_image(
        &self,
        _input: &kokkak_domain::CategoryJobServiceSubImageCreateInput,
    ) -> Result<kokkak_domain::CategoryJobServiceSubImageCreateResult, kokkak_domain::RepoError>
    {
        Err(kokkak_domain::RepoError::Backend(
            "category_job_service_sub repo not wired in build_app_state".into(),
        ))
    }
    async fn delete_image(
        &self,
        _input: &kokkak_domain::CategoryJobServiceSubImageDeleteInput,
    ) -> Result<kokkak_domain::CategoryJobServiceSubImageDeleteResult, kokkak_domain::RepoError>
    {
        Err(kokkak_domain::RepoError::Backend(
            "category_job_service_sub repo not wired in build_app_state".into(),
        ))
    }
    async fn create_with_images(
        &self,
        _input: &kokkak_domain::CategoryJobServiceSubCreateInput,
        _image_paths: &[kokkak_domain::traits::category_job_service_sub::SubImageForCreate],
    ) -> Result<kokkak_domain::CategoryJobServiceSubCreateResult, kokkak_domain::RepoError> {
        Err(kokkak_domain::RepoError::Backend(
            "category_job_service_sub repo not wired in build_app_state".into(),
        ))
    }
    async fn update_with_images(
        &self,
        _input: &kokkak_domain::CategoryJobServiceSubUpdateInput,
        _image_paths: &[kokkak_domain::traits::category_job_service_sub::SubImageForUpdate],
    ) -> Result<kokkak_domain::CategoryJobServiceSubUpdateResult, kokkak_domain::RepoError> {
        Err(kokkak_domain::RepoError::Backend(
            "category_job_service_sub repo not wired in build_app_state".into(),
        ))
    }
}

#[allow(dead_code)]
struct MssqlCategoryJobServiceSubFeeRepositoryNoop;

#[async_trait::async_trait]
impl kokkak_domain::CategoryJobServiceSubFeeRepository
    for MssqlCategoryJobServiceSubFeeRepositoryNoop
{
    async fn list(
        &self,
        _input: &kokkak_domain::CategoryJobServiceSubFeeListInput,
    ) -> Result<kokkak_domain::CategoryJobServiceSubFeePage, kokkak_domain::RepoError> {
        Err(kokkak_domain::RepoError::Backend(
            "category_job_service_sub_fee repo not wired in build_app_state (set KOKKAK_DATABASE__SQLSERVER_URL)"
                .into(),
        ))
    }
    async fn create(
        &self,
        _input: &kokkak_domain::CategoryJobServiceSubFeeCreateInput,
    ) -> Result<kokkak_domain::CategoryJobServiceSubFeeCreateResult, kokkak_domain::RepoError> {
        Err(kokkak_domain::RepoError::Backend(
                "category_job_service_sub_fee repo not wired in build_app_state (set KOKKAK_DATABASE__SQLSERVER_URL)"
                    .into(),
            ))
    }
    async fn update(
        &self,
        _input: &kokkak_domain::CategoryJobServiceSubFeeUpdateInput,
    ) -> Result<kokkak_domain::CategoryJobServiceSubFeeUpdateResult, kokkak_domain::RepoError> {
        Err(kokkak_domain::RepoError::Backend(
                "category_job_service_sub_fee repo not wired in build_app_state (set KOKKAK_DATABASE__SQLSERVER_URL)"
                    .into(),
            ))
    }
}

#[allow(clippy::too_many_arguments)]
pub fn build_app_state_with(
    bundle: RepoBundle,
    jwt: Arc<JwtService>,
    registry: HealthRegistry,
    settings: Arc<kokkak_common::config::Settings>,
    storage: Arc<dyn kokkak_domain::Storage>,
    public_base_url: Arc<str>,
    signed_url_secret: Arc<str>,
    signed_url_ttl_secs: u32,
) -> AppState {
    let audit: Arc<dyn AuditLogger> = match build_audit_logger() {
        Ok(l) => Arc::new(l),
        Err(e) => {
            tracing::error!(
                error = %e,
                "auth audit: FileAuditLogger init failed — login will run with no-op audit. \
                 Fix the path or permissions and restart to enable file-based auditing."
            );
            Arc::new(NoopAuditLogger)
        }
    };

    let login_rl: Arc<dyn LoginRateLimiter> = Arc::new(InMemoryLoginRateLimiter::new());

    let auth = Arc::new(AuthService::new(
        bundle.users.clone(),
        Arc::new(PasswordHasherAdapter::new()),
        Arc::new(JwtIssuerAdapter::new(jwt.clone())),
        audit,
        login_rl,
    ));
    let user = Arc::new(UserService::new(bundle.users.clone()));
    let hasher = Arc::new(PasswordHasherAdapter::new());
    let admin_users = Arc::new(AdminUserService::new(bundle.users.clone(), hasher));
    let catalog = Arc::new(CatalogService::new(bundle.services.clone()));
    let category_job_main = Arc::new(CategoryJobMainService::new(
        bundle.category_job_main.clone(),
    ));
    let category_job_service_main = Arc::new(CategoryJobServiceMainService::new(
        bundle.category_job_service_main.clone(),
    ));
    let category_job_service_sub = Arc::new(CategoryJobServiceSubService::new(
        bundle.category_job_service_sub.clone(),
    ));
    let category_job_service_sub_fee = Arc::new(CategoryJobServiceSubFeeService::new(
        bundle.category_job_service_sub_fee.clone(),
    ));
    let orders = Arc::new(OrderService::new(bundle.orders.clone()));
    let local: Arc<BroadcastTransport> = Arc::new(BroadcastTransport::default());
    let transport: Arc<dyn ChatTransport> = local.clone();
    let chat_service = Arc::new(ChatService::new(bundle.chat.clone(), transport));
    let chat = ChatHandle {
        service: chat_service,
        local,
    };
    let payments = Arc::new(PaymentService::new(
        bundle.payments.clone(),
        bundle.orders.clone(),
    ));
    let user_roles = Arc::new(UserRoleService::new(bundle.user_roles.clone()));
    let permission = Arc::new(PermissionUserService::new(bundle.permission_users.clone()));
    let master = Arc::new(MasterDropdownService::new(bundle.master.clone()));
    let translation: Arc<dyn TranslationRepository> = bundle.translation;

    let image_cfg = ImageProcessorConfig {
        max_input_bytes: settings.image.max_input_bytes,
        max_dimension_px: settings.image.max_dimension_px,
        webp_quality: settings.image.webp_quality,
    };
    let image: Arc<ImageProcessor> = Arc::new(ImageProcessor::new(storage.clone(), image_cfg));

    let pool_for_perm = bundle.mssql_pool.clone();
    let repo_for_perm = match pool_for_perm {
        Some(p) => Arc::new(kokkak_infra::db::mssql_permission::MssqlPermissionRepository::new(p)),
        None => Arc::new(kokkak_infra::db::mssql_permission::MssqlPermissionRepository::disabled()),
    };
    let cache_for_perm = if settings.redis.is_configured() {
        match kokkak_infra::cache::redis::RedisCache::new(&settings.redis) {
            Ok(rc) => Arc::new(
                kokkak_infra::cache::permission_cache::RedisPermissionCache::new(
                    rc.pool(),
                    settings.permission_cache.ttl_secs,
                ),
            ),
            Err(e) => {
                tracing::warn!(error = %e, "redis configured but pool build failed — permission cache disabled");
                Arc::new(
                    kokkak_infra::cache::permission_cache::RedisPermissionCache::disabled(
                        settings.permission_cache.ttl_secs,
                    ),
                )
            }
        }
    } else {
        Arc::new(
            kokkak_infra::cache::permission_cache::RedisPermissionCache::disabled(
                settings.permission_cache.ttl_secs,
            ),
        )
    };
    let permission_checker = Arc::new(kokkak_infra::permission_checker::PermissionChecker::new(
        repo_for_perm,
        cache_for_perm,
    ));

    AppState::new(
        auth,
        user,
        admin_users,
        catalog,
        category_job_main,
        category_job_service_main,
        category_job_service_sub,
        category_job_service_sub_fee,
        master,
        orders,
        chat,
        payments,
        permission,
        user_roles,
        jwt,
        registry,
        translation,
        settings,
        storage,
        image,
        permission_checker,
        public_base_url,
        signed_url_secret,
        signed_url_ttl_secs,
    )
}

fn build_audit_logger(
) -> Result<FileAuditLogger, Box<dyn std::error::Error + Send + Sync + 'static>> {
    let path = std::env::var("KOKKAK_AUDIT_LOG_PATH")
        .unwrap_or_else(|_| "logs/auth-audit.jsonl".to_string());
    FileAuditLogger::new(&path)
}

#[allow(clippy::too_many_arguments)]
pub fn build_app_state(
    user_repo: Arc<dyn kokkak_domain::UserRepository>,
    service_repo: Arc<dyn kokkak_domain::ServiceRepository>,
    order_repo: Arc<dyn kokkak_domain::OrderRepository>,
    chat_repo: Arc<dyn kokkak_domain::ChatRepository>,
    payment_repo: Arc<dyn kokkak_domain::PaymentRepository>,
    user_role_repo: Arc<dyn kokkak_domain::UserRoleRepository>,
    permission_user_repo: Arc<dyn kokkak_domain::PermissionUserRepository>,
    master_repo: Arc<dyn kokkak_domain::MasterDropdownRepository>,
    jwt: Arc<JwtService>,
    settings: Arc<kokkak_common::config::Settings>,
    registry: HealthRegistry,
    translation: Arc<dyn TranslationRepository>,
) -> AppState {
    let storage: Arc<dyn kokkak_domain::Storage> = Arc::new(MemoryStorage::new());

    let backend_marker: Option<MssqlPool> = None;
    let bundle = RepoBundle {
        backend: RepoBackend::Mssql,
        users: user_repo,
        services: service_repo,
        orders: order_repo,
        chat: chat_repo,
        payments: payment_repo,
        user_roles: user_role_repo,
        permission_users: permission_user_repo,
        translation,
        master: master_repo,
        category_job_main: Arc::new(MssqlCategoryJobMainRepositoryNoop),
        category_job_service_main: Arc::new(MssqlCategoryJobServiceMainRepositoryNoop),
        category_job_service_sub: Arc::new(MssqlCategoryJobServiceSubRepositoryNoop),
        category_job_service_sub_fee: Arc::new(MssqlCategoryJobServiceSubFeeRepositoryNoop),
        mssql_pool: backend_marker,
        topology: None,
    };
    build_app_state_with(
        bundle,
        jwt,
        registry,
        settings,
        storage,
        Arc::from(""),
        Arc::from(""),
        600,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn build_app_state_json(
    user_repo: Arc<dyn kokkak_domain::UserRepository>,
    service_repo: Arc<dyn kokkak_domain::ServiceRepository>,
    order_repo: Arc<dyn kokkak_domain::OrderRepository>,
    chat_repo: Arc<dyn kokkak_domain::ChatRepository>,
    payment_repo: Arc<dyn kokkak_domain::PaymentRepository>,
    user_role_repo: Arc<dyn kokkak_domain::UserRoleRepository>,
    permission_user_repo: Arc<dyn kokkak_domain::PermissionUserRepository>,
    master_repo: Arc<dyn kokkak_domain::MasterDropdownRepository>,
    jwt: Arc<JwtService>,
    registry: HealthRegistry,
    translation: Arc<dyn TranslationRepository>,
    settings: Arc<kokkak_common::config::Settings>,
) -> AppState {
    let backend_marker: Option<MssqlPool> = None;
    let bundle = RepoBundle {
        backend: RepoBackend::Mssql,
        users: user_repo,
        services: service_repo,
        orders: order_repo,
        chat: chat_repo,
        payments: payment_repo,
        user_roles: user_role_repo,
        permission_users: permission_user_repo,
        translation,
        master: master_repo,
        category_job_main: Arc::new(MssqlCategoryJobMainRepositoryNoop),
        category_job_service_main: Arc::new(MssqlCategoryJobServiceMainRepositoryNoop),
        category_job_service_sub: Arc::new(MssqlCategoryJobServiceSubRepositoryNoop),
        category_job_service_sub_fee: Arc::new(MssqlCategoryJobServiceSubFeeRepositoryNoop),
        mssql_pool: backend_marker,
        topology: None,
    };

    let storage: Arc<dyn kokkak_domain::Storage> = Arc::new(MemoryStorage::new());
    build_app_state_with(
        bundle,
        jwt,
        registry,
        settings,
        storage,
        Arc::from(""),
        Arc::from(""),
        600,
    )
}
