//! Kokkeak API library — exposes the public composition helpers
//! (router builder, state, adapters, repo factory) so
//! integration tests can drive the same routes the binary
//! serves.

pub mod adapters;
pub mod cert_watcher;
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

use kokkak_application::auth::AuthService;
use kokkak_application::catalog::CatalogService;
use kokkak_application::chat::{BroadcastTransport, ChatService, ChatTransport};
use kokkak_application::order::OrderService;
use kokkak_application::payment::PaymentService;
use kokkak_application::user::UserService;
use kokkak_domain::{HealthRegistry, TranslationRepository};
use kokkak_infra::auth::jwt::JwtService;
use kokkak_infra::db::mssql::MssqlPool;

use adapters::{JwtIssuerAdapter, PasswordHasherAdapter};

/// Build the `AppState` from a `RepoBundle` + JWT + health
/// registry. Use this from `main` (and from integration
/// tests) so the wiring stays in one place.
#[allow(clippy::too_many_arguments)]
pub fn build_app_state_with(
    bundle: RepoBundle,
    jwt: Arc<JwtService>,
    registry: HealthRegistry,
) -> AppState {
    let auth = Arc::new(AuthService::new(
        bundle.users.clone(),
        Arc::new(PasswordHasherAdapter::new()),
        Arc::new(JwtIssuerAdapter::new(jwt.clone())),
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
    let translation: Arc<dyn TranslationRepository> = bundle.translation;
    AppState::new(
        auth,
        user,
        catalog,
        orders,
        chat,
        payments,
        jwt,
        registry,
        translation,
    )
}

/// Build the full `AppState` from concrete infra handles.
/// Kept for backwards-compat with the integration tests that
/// pre-date the M10 factory; new code should call
/// [`build_app_state_with`] with a `RepoBundle`.
#[allow(clippy::too_many_arguments)]
pub fn build_app_state(
    user_repo: Arc<dyn kokkak_domain::UserRepository>,
    service_repo: Arc<dyn kokkak_domain::ServiceRepository>,
    order_repo: Arc<dyn kokkak_domain::OrderRepository>,
    chat_repo: Arc<dyn kokkak_domain::ChatRepository>,
    payment_repo: Arc<dyn kokkak_domain::PaymentRepository>,
    jwt: Arc<JwtService>,
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
        translation,
        mssql_pool: backend_marker,
        topology: None,
    };
    build_app_state_with(bundle, jwt, registry)
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
    jwt: Arc<JwtService>,
    registry: HealthRegistry,
    translation: Arc<dyn TranslationRepository>,
) -> AppState {
    let backend_marker: Option<MssqlPool> = None;
    let bundle = RepoBundle {
        backend: RepoBackend::Mssql,
        users: user_repo,
        services: service_repo,
        orders: order_repo,
        chat: chat_repo,
        payments: payment_repo,
        translation,
        mssql_pool: backend_marker,
        topology: None,
    };
    build_app_state_with(bundle, jwt, registry)
}
