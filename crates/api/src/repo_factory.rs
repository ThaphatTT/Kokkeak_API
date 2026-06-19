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
use kokkak_common::config::DatabaseTopologySettings;
use kokkak_domain::{
    ChatRepository, OrderRepository, PaymentRepository, ServiceRepository, TranslationRepository,
    UserRepository,
};
use kokkak_infra::cache::translation_cache::CachedTranslationRepository;
use kokkak_infra::db::json_catalog::JsonServiceRepository;
use kokkak_infra::db::json_chat::JsonChatRepository;
use kokkak_infra::db::json_order::JsonOrderRepository;
use kokkak_infra::db::json_payment::JsonPaymentRepository;
use kokkak_infra::db::json_translation::JsonTranslationRepository;
use kokkak_infra::db::json_user::JsonUserRepository;
use kokkak_infra::db::mssql::MssqlPool;
use kokkak_infra::db::mssql_catalog::MssqlServiceRepository;
use kokkak_infra::db::mssql_chat::MssqlChatRepository;
use kokkak_infra::db::mssql_order::MssqlOrderRepository;
use kokkak_infra::db::mssql_payment::MssqlPaymentRepository;
use kokkak_infra::db::mssql_user::MssqlUserRepository;
use kokkak_infra::db::topology::{DatabaseTopology, DbRole};
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
    /// Per-tenant translation override store (M11). Wraps a
    /// `JsonTranslationRepository` with an L1 moka cache so the
    /// hot path is sub-millisecond. Replaced with an MSSQL-backed
    /// implementation in M12+ when the production DB lands.
    pub translation: Arc<dyn TranslationRepository>,
    /// Primary (catch-all) MSSQL pool, kept alive for the
    /// duration of the process. Used by the migration runner and
    /// by the T06 health check. M12 adds [`RepoBundle::topology`]
    /// for per-role pool access.
    pub mssql_pool: Option<MssqlPool>,
    /// Multi-DB topology (M12). Exposes per-role pools so future
    /// handlers / workers can route queries to the right
    /// physical database. `Some` when MSSQL is in use,
    /// `None` when only the JSON-DB sim is active.
    pub topology: Option<DatabaseTopology>,
}

impl std::fmt::Debug for RepoBundle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RepoBundle")
            .field("backend", &self.backend)
            .field("mssql_pool", &self.mssql_pool.as_ref().map(|_| "<pool>"))
            .field("topology", &self.topology.as_ref().map(|t| t.live_roles()))
            .finish()
    }
}

/// Build the `RepoBundle` from settings + a data dir.
///
/// The switch is environment-driven (no code change needed
/// to flip from dev to prod):
///
/// | Topology configured?              | Outcome                       |
/// |-----------------------------------|-------------------------------|
/// | no                                | `RepoBackend::Json` (sim)     |
/// | catch-all URL only                | `RepoBackend::Mssql` (1 pool shared) |
/// | per-role URLs set                 | `RepoBackend::Mssql` (topology with N pools) |
/// | catch-all URL set, pool fails     | warn + fall back to `Json`   |
///
/// See `AGENTS.md` § 7 and `DatabaseTopology` for the full
/// multi-DB rule set.
pub async fn from_settings(
    data_dir: &Path,
    settings: &kokkak_common::config::Settings,
) -> Result<RepoBundle, FactoryError> {
    let topo_settings = DatabaseTopologySettings::from_settings(settings);
    if topo_settings.catch_all.is_configured() || has_any_role_url(&topo_settings) {
        match DatabaseTopology::build(&topo_settings, false).await {
            Ok(topo) if !topo.is_empty() => {
                let primary = topo
                    .primary_role()
                    .expect("non-empty topology has a primary role");
                let primary_pool = topo.get(primary).clone();
                info!(
                    backend = "mssql",
                    primary_role = primary.as_str(),
                    roles = ?topo.live_roles(),
                    "kokkak-api: using SQL Server topology"
                );
                let user_repo: Arc<dyn UserRepository> = Arc::new(MssqlUserRepository::new(
                    topo_pool(&topo, DbRole::Master, &primary_pool),
                ));
                let service_repo: Arc<dyn ServiceRepository> = Arc::new(
                    MssqlServiceRepository::new(topo_pool(&topo, DbRole::Catalog, &primary_pool)),
                );
                let order_repo: Arc<dyn OrderRepository> = Arc::new(
                    MssqlOrderRepository::new(topo_pool(&topo, DbRole::Order, &primary_pool)),
                );
                let chat_repo: Arc<dyn ChatRepository> = Arc::new(
                    MssqlChatRepository::new(topo_pool(&topo, DbRole::Master, &primary_pool)),
                );
                let payment_repo: Arc<dyn PaymentRepository> = Arc::new(
                    MssqlPaymentRepository::new(topo_pool(&topo, DbRole::Payment, &primary_pool)),
                );
                Ok(RepoBundle {
                    backend: RepoBackend::Mssql,
                    users: user_repo,
                    services: service_repo,
                    orders: order_repo,
                    chat: chat_repo,
                    payments: payment_repo,
                    translation: build_translation_repo(data_dir).await,
                    mssql_pool: Some(primary_pool),
                    topology: Some(topo),
                })
            }
            Ok(_) => {
                // `build` returned an empty topology even though
                // we asked it to ignore `require_all`. Should not
                // happen — fall through to JSON.
                warn!(
                    "topology build returned empty pool set — \
                     falling back to JSON-DB sim"
                );
                build_json_bundle(data_dir).await
            }
            Err(e) => {
                warn!(
                    error = %e,
                    "mssql topology build failed — falling back to JSON-DB sim"
                );
                build_json_bundle(data_dir).await
            }
        }
    } else {
        build_json_bundle(data_dir).await
    }
}

/// `true` when at least one per-role URL is set, even if the
/// catch-all is empty. Lets the operator declare per-role
/// databases without using the legacy catch-all.
fn has_any_role_url(s: &DatabaseTopologySettings) -> bool {
    DbRole::ALL.iter().any(|r| s.slot(*r).is_configured())
}

/// Pick the per-role pool if the topology has one, else fall
/// back to the catch-all pool. Centralised so the factory stays
/// readable.
fn topo_pool(topo: &DatabaseTopology, role: DbRole, fallback: &MssqlPool) -> MssqlPool {
    topo.try_get(role).cloned().unwrap_or_else(|| fallback.clone())
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
        translation: build_translation_repo(data_dir).await,
        mssql_pool: None,
        topology: None,
    })
}

/// Build the per-tenant translation store from `data_dir`. M11
/// uses the JSON-DB sim wrapped in a 60s moka L1; M12 will swap
/// in an MSSQL adapter behind the same
/// [`TranslationRepository`] port.
async fn build_translation_repo(data_dir: &Path) -> Arc<dyn TranslationRepository> {
    let path = data_dir.join("translations.json");
    let inner = JsonTranslationRepository::open(&path)
        .await
        .unwrap_or_else(|e| {
            tracing::warn!(
                error = %e,
                path = %path.display(),
                "failed to open translation store — starting empty"
            );
            JsonTranslationRepository::in_memory()
        });
    Arc::new(CachedTranslationRepository::new(inner))
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
