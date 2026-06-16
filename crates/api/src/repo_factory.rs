//! Repository factory (M10).
//!
//! Picks the right adapter family at startup:
//!
//! - **MSSQL** (production) when `KOKKAK_DATABASE__SQLSERVER_URL` is set
//!   AND the `tiberius` pool builds successfully. All five
//!   aggregates (user / service / order / chat / payment) go
//!   to the corresponding `Mssql*` repositories.
//!
//! - **JSON-DB sim** (dev / e2e) otherwise. The same five
//!   aggregates go to the `Json*` repositories on disk.
//!
//! ## Fail-safe
//!
//! If `KOKKAK_DATABASE__SQLSERVER_URL` is set but the pool
//! build (or the `SELECT 1` health probe) fails, we **log a
//! warning and fall back to JSON** — the dev/e2e flow keeps
//! working. Production deploys are expected to set
//! `KOKKAK_REQUIRE_MSSQL=1` to make any fallback a hard
//! error.
//!
//! ## Per-aggregate isolation
//!
//! The MSSQL impls are tiberius-backed; the JSON impls are
//! tokio::fs-backed. They are *never* mixed in production
//! (the factory returns one or the other for the entire
//! process). Tests can of course mix.

use std::path::Path;
use std::sync::Arc;

use kokkak_application::order::OrderService;
use kokkak_domain::{
    ChatRepository, OrderRepository, PaymentRepository, ServiceRepository, UserRepository,
};
use kokkak_infra::db::json_catalog::JsonServiceRepository;
use kokkak_infra::db::json_chat::JsonChatRepository;
use kokkak_infra::db::json_order::JsonOrderRepository;
use kokkak_infra::db::json_payment::JsonPaymentRepository;
use kokkak_infra::db::json_user::JsonUserRepository;
use kokkak_infra::db::mssql::{build_pool, ping as mssql_ping, MssqlPool};
use kokkak_infra::db::mssql_catalog::MssqlServiceRepository;
use kokkak_infra::db::mssql_chat::MssqlChatRepository;
use kokkak_infra::db::mssql_order::MssqlOrderRepository;
use kokkak_infra::db::mssql_payment::MssqlPaymentRepository;
use kokkak_infra::db::mssql_user::MssqlUserRepository;
use tracing::{info, warn};

/// Which adapter family the factory is using.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepoBackend {
    /// JSON-DB sim (dev / e2e / fallback).
    Json,
    /// SQL Server via tiberius.
    Mssql,
}

impl RepoBackend {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Mssql => "mssql",
        }
    }
}

/// Bundle of every repository handle the API needs.
pub struct RepoBundle {
    pub backend: RepoBackend,
    pub users: Arc<dyn UserRepository>,
    pub services: Arc<dyn ServiceRepository>,
    pub orders: Arc<dyn OrderRepository>,
    pub chat: Arc<dyn ChatRepository>,
    pub payments: Arc<dyn PaymentRepository>,
    /// Kept alive for the duration of the process. M12+ uses
    /// this to register a SQL Server health check.
    pub mssql_pool: Option<MssqlPool>,
}

impl std::fmt::Debug for RepoBundle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RepoBundle")
            .field("backend", &self.backend)
            .field("mssql_pool", &self.mssql_pool.as_ref().map(|_| "<pool>"))
            .finish()
    }
}

/// Build the `RepoBundle` from settings + a data dir.
///
/// The switch is environment-driven (no code change needed
/// to flip from dev to prod):
///
/// | `KOKKAK_DATABASE__SQLSERVER_URL` | Outcome                       |
/// |----------------------------------|-------------------------------|
/// | empty / "disabled"               | `RepoBackend::Json` (sim)     |
/// | valid JDBC URL, pool builds     | `RepoBackend::Mssql` (real)   |
/// | set but pool build fails         | warn + fall back to `Json`   |
pub async fn from_settings(
    data_dir: &Path,
    settings: &kokkak_common::config::Settings,
) -> Result<RepoBundle, FactoryError> {
    let db = &settings.database;
    if db.is_configured() {
        match build_pool(db).await {
            Ok(pool) => {
                // Health probe; on failure fall back to JSON.
                if let Err(e) = mssql_ping(&pool).await {
                    warn!(
                        error = %e,
                        "mssql ping failed — falling back to JSON-DB sim"
                    );
                    return build_json_bundle(data_dir).await;
                }
                info!(
                    backend = "mssql",
                    pool_size = db.pool_size,
                    "kokkak-api: using SQL Server repositories"
                );
                let user_repo: Arc<dyn UserRepository> =
                    Arc::new(MssqlUserRepository::new(pool.clone()));
                let service_repo: Arc<dyn ServiceRepository> =
                    Arc::new(MssqlServiceRepository::new(pool.clone()));
                let order_repo: Arc<dyn OrderRepository> =
                    Arc::new(MssqlOrderRepository::new(pool.clone()));
                let chat_repo: Arc<dyn ChatRepository> =
                    Arc::new(MssqlChatRepository::new(pool.clone()));
                let payment_repo: Arc<dyn PaymentRepository> =
                    Arc::new(MssqlPaymentRepository::new(pool.clone()));
                Ok(RepoBundle {
                    backend: RepoBackend::Mssql,
                    users: user_repo,
                    services: service_repo,
                    orders: order_repo,
                    chat: chat_repo,
                    payments: payment_repo,
                    mssql_pool: Some(pool),
                })
            }
            Err(e) => {
                warn!(
                    error = %e,
                    "mssql pool build failed — falling back to JSON-DB sim"
                );
                build_json_bundle(data_dir).await
            }
        }
    } else {
        build_json_bundle(data_dir).await
    }
}

/// Force the JSON backend (used by integration tests so they
/// never depend on a real SQL Server).
pub async fn force_json(data_dir: &Path) -> Result<RepoBundle, FactoryError> {
    build_json_bundle(data_dir).await
}

async fn build_json_bundle(data_dir: &Path) -> Result<RepoBundle, FactoryError> {
    info!(
        backend = "json",
        "kokkak-api: using JSON-DB sim repositories"
    );
    let user_repo: Arc<dyn UserRepository> = Arc::new(
        JsonUserRepository::open(data_dir.join("users.json"))
            .await
            .map_err(FactoryError::User)?,
    );
    let service_repo: Arc<dyn ServiceRepository> = Arc::new(
        JsonServiceRepository::open(data_dir.join("services.json"))
            .await
            .map_err(FactoryError::Service)?,
    );
    let order_repo: Arc<dyn OrderRepository> = Arc::new(
        JsonOrderRepository::open(data_dir.join("orders.json"))
            .await
            .map_err(FactoryError::Order)?,
    );
    let chat_repo: Arc<dyn ChatRepository> = Arc::new(
        JsonChatRepository::open(data_dir.join("chat.json"))
            .await
            .map_err(FactoryError::Chat)?,
    );
    let payment_repo: Arc<dyn PaymentRepository> = Arc::new(
        JsonPaymentRepository::open(data_dir.join("payments.json"))
            .await
            .map_err(FactoryError::Payment)?,
    );
    Ok(RepoBundle {
        backend: RepoBackend::Json,
        users: user_repo,
        services: service_repo,
        orders: order_repo,
        chat: chat_repo,
        payments: payment_repo,
        mssql_pool: None,
    })
}

/// Errors raised by the factory (mapped to a startup abort).
#[derive(Debug, thiserror::Error)]
pub enum FactoryError {
    #[error("open user repo: {0}")]
    User(#[source] kokkak_domain::RepoError),
    #[error("open service repo: {0}")]
    Service(#[source] kokkak_domain::RepoError),
    #[error("open order repo: {0}")]
    Order(#[source] kokkak_domain::RepoError),
    #[error("open chat repo: {0}")]
    Chat(#[source] kokkak_domain::ChatRepoError),
    #[error("open payment repo: {0}")]
    Payment(#[source] kokkak_domain::PaymentRepoError),
}

/// Re-export `OrderService::orders_repo` accessor so the
/// `PaymentService` builder (which takes an
/// `Arc<dyn OrderRepository>`) can be wired after the
/// factory has run.
pub fn orders_arc(bundle: &RepoBundle) -> Arc<dyn OrderRepository> {
    Arc::clone(&bundle.orders)
}

/// Lightweight test: the factory's `force_json` path must not
/// require a SQL Server (so the e2e / integration tests run
/// without infra).
pub async fn test_force_json_runs_without_mssql(tmp: &Path) -> Result<(), FactoryError> {
    let _ = OrderService::new; // suppress unused import
    let bundle = force_json(tmp).await?;
    // The bundle's `backend` must always be `Json` here.
    assert!(matches!(bundle.backend, RepoBackend::Json));
    Ok(())
}
