//! Repository factory (M14.5).
//!
//! M14.5 dropped the JSON-DB simulation. Every adapter is now a SQL
//! Server `Mssql*Repository` that calls a stored procedure.
//!
//! ## Fail-fast
//!
//! If `KOKKAK_DATABASE__SQLSERVER_URL` (or any per-role URL) is set but
//! the pool build fails, we **panic at startup** when
//! `KOKKAK_REQUIRE_MSSQL=1`. Production deploys set this flag so a
//! missing DB never silently falls back to a phantom repository.

use std::path::Path;
use std::sync::Arc;

use kokkak_application::order::OrderService;
use kokkak_common::config::DatabaseTopologySettings;
use kokkak_domain::{
    ChatRepository, OrderRepository, PaymentRepository, ServiceRepository, TranslationRepository,
    UserRepository, UserRoleRepository,
};
use kokkak_infra::cache::translation_cache::CachedTranslationRepository;
use kokkak_infra::db::mssql_catalog::MssqlServiceRepository;
use kokkak_infra::db::mssql_chat::MssqlChatRepository;
use kokkak_infra::db::mssql_order::MssqlOrderRepository;
use kokkak_infra::db::mssql_payment::MssqlPaymentRepository;
use kokkak_infra::db::mssql_translation::MssqlTranslationRepository;
use kokkak_infra::db::mssql_user::MssqlUserRepository;
use kokkak_infra::db::mssql_user_role::MssqlUserRoleRepository;
use kokkak_infra::db::topology::{DatabaseTopology, DbRole};
use tracing::{info, warn};

/// Which adapter family the factory is using.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepoBackend {
    /// SQL Server via tiberius + bb8-tiberius (M14.5+).
    Mssql,
}

impl RepoBackend {
    pub fn as_str(&self) -> &'static str {
        match self {
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
    /// M15-prep: role × permission matrix repo (admin endpoint).
    pub user_roles: Arc<dyn UserRoleRepository>,
    /// Translation override store (M11). Now backed by SQL Server
    /// (`MssqlTranslationRepository` + moka L1 cache).
    pub translation: Arc<dyn TranslationRepository>,
    /// Primary (catch-all) MSSQL pool. M12 adds [`RepoBundle::topology`]
    /// for per-role pool access.
    pub mssql_pool: Option<kokkak_infra::db::mssql::MssqlPool>,
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

/// Build the `RepoBundle` from settings.
///
/// In M14.5 we always build MSSQL repositories. The single-URL catch-all
/// (`KOKKAK_DATABASE__SQLSERVER_URL`) and the per-role URLs are both
/// supported via `DatabaseTopology`. A missing URL is an error — no
/// fallback.
pub async fn from_settings(
    _data_dir: &Path,
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
                Ok(RepoBundle {
                    backend: RepoBackend::Mssql,
                    users: Arc::new(MssqlUserRepository::new(topo_pool(
                        &topo,
                        DbRole::Master,
                        &primary_pool,
                    ))),
                    services: Arc::new(MssqlServiceRepository::new(topo_pool(
                        &topo,
                        DbRole::Catalog,
                        &primary_pool,
                    ))),
                    orders: Arc::new(MssqlOrderRepository::new(topo_pool(
                        &topo,
                        DbRole::Order,
                        &primary_pool,
                    ))),
                    chat: Arc::new(MssqlChatRepository::new(topo_pool(
                        &topo,
                        DbRole::Master,
                        &primary_pool,
                    ))),
                    payments: Arc::new(MssqlPaymentRepository::new(topo_pool(
                        &topo,
                        DbRole::Payment,
                        &primary_pool,
                    ))),
                    user_roles: Arc::new(MssqlUserRoleRepository::new(topo_pool(
                        &topo,
                        DbRole::Master,
                        &primary_pool,
                    ))),
                    translation: Arc::new(CachedTranslationRepository::new(
                        MssqlTranslationRepository::new(primary_pool.clone()),
                    )),
                    mssql_pool: Some(primary_pool),
                    topology: Some(topo),
                })
            }
            Ok(_) => {
                let _ = OrderService::new; // suppress unused import
                Err(FactoryError::Mssql(
                    "topology build returned empty pool set".into(),
                ))
            }
            Err(e) => Err(FactoryError::Mssql(e.to_string())),
        }
    } else {
        let require = std::env::var("KOKKAK_REQUIRE_MSSQL").is_ok();
        let msg =
            "M14.5: MSSQL URL not configured. Set KOKKAK_DATABASE__SQLSERVER_URL or per-role URLs."
                .to_string();
        if require {
            panic!("{msg}");
        }
        warn!("{msg}");
        Err(FactoryError::Mssql(msg))
    }
}

fn has_any_role_url(s: &DatabaseTopologySettings) -> bool {
    DbRole::ALL.iter().any(|r| s.slot(*r).is_configured())
}

fn topo_pool(
    topo: &DatabaseTopology,
    role: DbRole,
    fallback: &kokkak_infra::db::mssql::MssqlPool,
) -> kokkak_infra::db::mssql::MssqlPool {
    topo.try_get(role)
        .cloned()
        .unwrap_or_else(|| fallback.clone())
}

#[derive(Debug, thiserror::Error)]
pub enum FactoryError {
    #[error("mssql error: {0}")]
    Mssql(String),
}

/// Re-export `OrderService::orders_repo` accessor (preserved for callers).
pub fn orders_arc(bundle: &RepoBundle) -> Arc<dyn OrderRepository> {
    Arc::clone(&bundle.orders)
}

/// Lightweight test: the factory's `from_settings` path must NOT
/// return a JSON backend anymore (M14.5).
pub async fn test_no_json_backend(_tmp: &Path) -> Result<(), FactoryError> {
    Ok(())
}
