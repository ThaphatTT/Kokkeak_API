//! Kokkeak API library — exposes the public composition helpers
//! (router builder, state, adapters, repo factory) so
//! integration tests can drive the same routes the binary
//! serves.

pub mod adapters;
pub mod cert_watcher;
pub mod error;
pub mod extractors;
pub mod handlers;
pub mod middleware;
pub mod openapi;
pub mod redirect;
pub mod repo_factory;
pub mod router;
pub mod state;
pub mod tls;

pub use repo_factory::{from_settings as build_repos, RepoBackend, RepoBundle};
pub use router::build as build_router;
pub use state::{AppState, ChatHandle};

use std::sync::Arc;

use kokkak_application::audit::{AuditLogger, NoopAuditLogger};
use kokkak_application::auth::AuthService;
use kokkak_application::catalog::CatalogService;
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

use adapters::{JwtIssuerAdapter, PasswordHasherAdapter};

/// Build the `AppState` from a `RepoBundle` + JWT + health
/// registry + settings. Use this from `main` (and from
/// integration tests) so the wiring stays in one place.
///
/// `settings` is held as `Arc<Settings>` so the feature-flag
/// gates can read it without copying the full config on every
/// request (T-31).
///
/// Audit log path defaults to `logs/auth-audit.jsonl` (created on
/// demand). Override via `KOKKAK_AUDIT_LOG_PATH` for production —
/// point at a mounted volume that survives container restarts.
///
/// ponytail: the audit log + rate limiter here are fire-and-forget.
/// A broken audit file becomes a dropped-line warning, not a 500
/// on login. A failed rate-limit construction falls back to a
/// no-op limiter so a startup misconfiguration doesn't take down
/// the whole API.
#[allow(clippy::too_many_arguments)]
pub fn build_app_state_with(
    bundle: RepoBundle,
    jwt: Arc<JwtService>,
    registry: HealthRegistry,
    settings: Arc<kokkak_common::config::Settings>,
) -> AppState {
    // Audit sink: try FileAuditLogger, fall back to no-op so a
    // permission error on the log dir doesn't break the API.
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
    // Login rate limiter (per-username + IP, sliding window 5min,
    // 5 attempts). For HA production swap in a Redis-backed
    // implementation behind the same `LoginRateLimiter` trait.
    let login_rl: Arc<dyn LoginRateLimiter> = Arc::new(InMemoryLoginRateLimiter::new());

    let auth = Arc::new(AuthService::new(
        bundle.users.clone(),
        Arc::new(PasswordHasherAdapter::new()),
        Arc::new(JwtIssuerAdapter::new(jwt.clone())),
        audit,
        login_rl,
    ));
    let user = Arc::new(UserService::new(bundle.users.clone()));
    let catalog = Arc::new(CatalogService::new(bundle.services.clone()));
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
    AppState::new(
        auth,
        user,
        catalog,
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
    )
}

/// Resolve the audit-log path from env or fall back to
/// `logs/auth-audit.jsonl` under the current working directory.
/// Returned here so the caller can wrap the result with a clear
/// `WARN` log when the file can't be opened.
fn build_audit_logger(
) -> Result<FileAuditLogger, Box<dyn std::error::Error + Send + Sync + 'static>> {
    let path = std::env::var("KOKKAK_AUDIT_LOG_PATH")
        .unwrap_or_else(|_| "logs/auth-audit.jsonl".to_string());
    FileAuditLogger::new(&path)
}

/// Build the full `AppState` from concrete infra handles.
/// Kept for backwards-compat with the integration tests that
/// pre-date the M10 factory; new code should call
/// [`build_app_state_with`] with a `RepoBundle`.
// optional because build_app_state_with doesn't need them.
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
    // M14.5: backend is always Mssql; the pool/mssql_pool are
    // optional because build_app_state_with doesn't need them.
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
        mssql_pool: backend_marker,
        topology: None,
    };
    build_app_state_with(bundle, jwt, registry, settings)
}

/// Convenience builder for tests/dev: pass chat + payments already
/// built against MSSQL. The in-memory chat transport is wired up here.
/// M14.5: removed JSON-DB sim entirely.
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
        mssql_pool: backend_marker,
        topology: None,
    };
    build_app_state_with(bundle, jwt, registry, settings)
}
