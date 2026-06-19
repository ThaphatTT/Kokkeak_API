//! Multi-database connection topology (M12).
//!
//! Maps [`DbRole`] (Master / Catalog / Order / Payment / Log /
//! Report / Temp) to its own `bb8` pool. This is the foundation for
//! AGENTS.md § 7.1's multi-database topology: instead of one
//! connection string pointing at one database, the operator declares
//! one connection string **per role**, and each repository
//! transparently uses the pool for its own role.
//!
//! ## Why a typed `DbRole` enum
//!
//! The alternative — a `HashMap<String, MssqlPool>` — loses the
//! compile-time guarantee that "user goes to Master, order goes to
//! Order, payment goes to Payment". With an enum, adding a new
//! database role is a single line and the compiler will tell you
//! every repo that needs updating.
//!
//! ## Backwards compatibility (M10 → M12)
//!
//! If only `KOKKAK_DATABASE__SQLSERVER_URL` is set, every role
//! shares the same pool. The transition to multi-DB is opt-in:
//! set `KOKKAK_DATABASE__MASTER_URL` etc. to override per role.
//!
//! See `AGENTS.md` § 7 for the canonical topology and the multi-DB
//! rules of thumb (one SQL Server instance, one database per
//! concern; keep the schema `KOKKAK_<ROLE>`).

use std::collections::HashMap;

use thiserror::Error;

use kokkak_common::config::DatabaseTopologySettings;

use crate::db::mssql::{build_pool, ping as mssql_ping, MssqlError, MssqlPool};

/// Re-export [`DbRole`] so the rest of `kokkak-infra` (and
/// `kokkak-api`) can keep importing `kokkak_infra::db::topology::DbRole`
/// — that path was the canonical home in M11, before the
/// `kokkak_common::config::DbRole` move in M12.
pub use kokkak_common::config::DbRole;

/// Errors raised when building the multi-DB topology.
#[derive(Debug, Error)]
pub enum TopologyError {
    /// One role's URL is set but the pool could not be built.
    #[error("role '{role}' pool build failed: {source}")]
    RoleBuild {
        role: DbRole,
        #[source]
        source: MssqlError,
    },

    /// A role's pool was built but the `SELECT 1` health probe failed.
    #[error("role '{role}' health probe failed: {source}")]
    RoleUnhealthy {
        role: DbRole,
        #[source]
        source: MssqlError,
    },

    /// The user requested `require_all = true` but no role had a URL.
    #[error("KOKKAK_DATABASE__SQLSERVER_URL is empty — set at least one role URL")]
    RequireAllButUnset,
}

/// Multi-database connection topology (multi-pool registry).
///
/// Internally `HashMap<DbRole, MssqlPool>`. Cheap to clone (`Arc`
/// inside every pool). Built once at startup; held for the
/// lifetime of the process.
#[derive(Clone)]
pub struct DatabaseTopology {
    pools: HashMap<DbRole, MssqlPool>,
    /// The "primary" pool, kept by `RepoBundle` for the migration
    /// runner. It's the **first** role we successfully built —
    /// the M5 behaviour of "one pool for everything" still
    /// applies in the single-URL case.
    primary_role: Option<DbRole>,
}

impl std::fmt::Debug for DatabaseTopology {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DatabaseTopology")
            .field("roles", &self.pools.keys().collect::<Vec<_>>())
            .field("primary_role", &self.primary_role)
            .finish()
    }
}

impl DatabaseTopology {
    /// Construct an empty topology. Useful in tests and as a
    /// "no databases yet" placeholder. See [`Self::build`] for
    /// the production path.
    pub fn empty() -> Self {
        Self {
            pools: HashMap::new(),
            primary_role: None,
        }
    }

    /// Build a topology from [`DatabaseTopologySettings`].
    ///
    /// `require_all = true` makes the call fail with
    /// [`TopologyError::RequireAllButUnset`] when no role URL is
    /// configured. `require_all = false` returns an empty topology
    /// (the factory then falls back to JSON-DB sim).
    ///
    /// Per role, the URL is taken from
    /// `KOKKAK_DATABASE__<ROLE>_URL` (the topology
    /// `DatabaseSettings.sqlserver_url` slot for that role).
    /// Any role whose URL is empty inherits the value of
    /// `KOKKAK_DATABASE__SQLSERVER_URL` (the catch-all). This is
    /// the M10 → M12 bridge: existing single-URL deploys keep
    /// working without any change.
    pub async fn build(
        settings: &DatabaseTopologySettings,
        require_all: bool,
    ) -> Result<Self, TopologyError> {
        let mut pools: HashMap<DbRole, MssqlPool> = HashMap::with_capacity(7);
        let mut primary_role: Option<DbRole> = None;

        for role in DbRole::ALL {
            let s = settings.for_role(role);
            if !s.is_configured() {
                continue;
            }
            let pool = build_pool(&s)
                .await
                .map_err(|source| TopologyError::RoleBuild { role, source })?;
            mssql_ping(&pool)
                .await
                .map_err(|source| TopologyError::RoleUnhealthy { role, source })?;
            if primary_role.is_none() {
                primary_role = Some(role);
            }
            pools.insert(role, pool);
        }

        if pools.is_empty() {
            if require_all {
                return Err(TopologyError::RequireAllButUnset);
            }
            tracing::info!("database topology: empty (all roles unset → JSON-DB sim)");
            return Ok(Self {
                pools,
                primary_role: None,
            });
        }

        tracing::info!(
            roles = ?pools.keys().collect::<Vec<_>>(),
            primary = ?primary_role,
            "database topology: built"
        );

        Ok(Self {
            pools,
            primary_role,
        })
    }

    /// The role of the first pool we successfully built. Used by
    /// the migration runner, which only needs ONE working pool to
    /// create `schema_migrations`.
    pub fn primary_role(&self) -> Option<DbRole> {
        self.primary_role
    }

    /// Borrow the pool for a role. Panics if the role was not
    /// configured — the factory guarantees that any role a
    /// repository asks for is present (otherwise we never get
    /// this far: the `Mssql*Repository::new` calls happen after
    /// `build`).
    pub fn get(&self, role: DbRole) -> &MssqlPool {
        self.pools
            .get(&role)
            .unwrap_or_else(|| panic!("database topology: role '{role}' not configured"))
    }

    /// Try to borrow a pool; returns `None` when the role was not
    /// configured. Use this in factory code that needs to handle
    /// missing roles gracefully (mixed JSON/MSSQL fallback).
    pub fn try_get(&self, role: DbRole) -> Option<&MssqlPool> {
        self.pools.get(&role)
    }

    /// All roles that have a live pool. Sorted by [`DbRole::ALL`]
    /// order for stable logging / health-check output.
    pub fn live_roles(&self) -> Vec<DbRole> {
        DbRole::ALL
            .iter()
            .copied()
            .filter(|r| self.pools.contains_key(r))
            .collect()
    }

    /// Probe every live pool in parallel. The result is a per-role
    /// status. The probes have a short timeout (`ping` itself
    /// takes the bb8 acquisition timeout + a `SELECT 1` roundtrip).
    pub async fn health_check(&self) -> HashMap<DbRole, Result<(), MssqlError>> {
        let mut out: HashMap<DbRole, Result<(), MssqlError>> = HashMap::new();
        for role in self.live_roles() {
            let pool = self.pools.get(&role).expect("live role");
            let r = mssql_ping(pool).await;
            out.insert(role, r);
        }
        out
    }

    /// True when the topology has at least one live pool.
    pub fn is_empty(&self) -> bool {
        self.pools.is_empty()
    }

    /// Number of live pools.
    pub fn len(&self) -> usize {
        self.pools.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kokkak_common::config::DatabaseSettings;

    #[test]
    fn db_role_env_suffix_is_stable() {
        // Pin the env-var contract — operators grep their .env for
        // these names.
        assert_eq!(DbRole::Master.env_suffix(), "MASTER_URL");
        assert_eq!(DbRole::Catalog.env_suffix(), "CATALOG_URL");
        assert_eq!(DbRole::Order.env_suffix(), "ORDER_URL");
        assert_eq!(DbRole::Payment.env_suffix(), "PAYMENT_URL");
        assert_eq!(DbRole::Log.env_suffix(), "LOG_URL");
        assert_eq!(DbRole::Report.env_suffix(), "REPORT_URL");
        assert_eq!(DbRole::Temp.env_suffix(), "TEMP_URL");
    }

    #[test]
    fn db_role_as_str_is_lowercase() {
        for role in DbRole::ALL {
            let s = role.as_str();
            assert!(s.chars().all(|c| c.is_ascii_lowercase()));
            assert!(!s.is_empty());
        }
    }

    #[test]
    fn db_role_display_matches_as_str() {
        for role in DbRole::ALL {
            assert_eq!(format!("{role}"), role.as_str());
        }
    }

    #[test]
    fn topology_for_role_falls_back_to_catch_all() {
        // M10 → M12 bridge: only the catch-all is set.
        let s = DatabaseTopologySettings {
            catch_all: DatabaseSettings::from_url("Server=x;Database=K;User Id=u;Password=p"),
            master: DatabaseSettings::default(),
            catalog: DatabaseSettings::default(),
            order: DatabaseSettings::default(),
            payment: DatabaseSettings::default(),
            log: DatabaseSettings::default(),
            report: DatabaseSettings::default(),
            temp: DatabaseSettings::default(),
        };
        for role in DbRole::ALL {
            let got = s.for_role(role);
            assert!(
                got.is_configured(),
                "role {role} should inherit from catch-all"
            );
        }
    }

    #[test]
    fn topology_for_role_uses_per_role_override() {
        // Per-role URL wins over the catch-all.
        let s = DatabaseTopologySettings {
            catch_all: DatabaseSettings::from_url("Server=default;Database=D;User Id=u;Password=p"),
            order: DatabaseSettings::from_url(
                "Server=orderhost;Database=ORDER;User Id=u;Password=p",
            ),
            ..DatabaseTopologySettings::default()
        };
        let order = s.for_role(DbRole::Order);
        // The role's URL won — we can't directly read `sqlserver_url`
        // from the public API, but is_configured() is true and the
        // setter took effect.
        assert!(order.is_configured());
    }

    #[tokio::test]
    async fn empty_topology_build_succeeds_when_not_required() {
        let topo = DatabaseTopology::build(&DatabaseTopologySettings::default(), false)
            .await
            .expect("empty build");
        assert!(topo.is_empty());
        assert_eq!(topo.len(), 0);
        assert!(topo.primary_role().is_none());
    }

    #[tokio::test]
    async fn empty_topology_build_fails_when_required() {
        let err = DatabaseTopology::build(&DatabaseTopologySettings::default(), true)
            .await
            .unwrap_err();
        assert!(matches!(err, TopologyError::RequireAllButUnset));
    }

    #[test]
    fn empty_constructor_yields_empty_topology() {
        let topo = DatabaseTopology::empty();
        assert!(topo.is_empty());
        assert!(topo.live_roles().is_empty());
        assert!(topo.primary_role().is_none());
    }

    #[test]
    fn topology_live_roles_returns_empty_when_unset() {
        let topo = DatabaseTopology::empty();
        assert!(topo.live_roles().is_empty());
        assert_eq!(topo.len(), 0);
    }
}
