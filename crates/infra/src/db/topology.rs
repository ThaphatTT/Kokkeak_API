

use std::collections::HashMap;

use thiserror::Error;

use kokkak_common::config::DatabaseTopologySettings;

use crate::db::mssql::{build_pool, ping as mssql_ping, MssqlError, MssqlPool};

pub use kokkak_common::config::DbRole;

#[derive(Debug, Error)]
pub enum TopologyError {

    #[error("role '{role}' pool build failed: {source}")]
    RoleBuild {

        role: DbRole,

        #[source]
        source: MssqlError,
    },

    #[error("role '{role}' health probe failed: {source}")]
    RoleUnhealthy {

        role: DbRole,

        #[source]
        source: MssqlError,
    },

    #[error("KOKKAK_DATABASE__SQLSERVER_URL is empty — set at least one role URL")]
    RequireAllButUnset,
}

#[derive(Clone)]
pub struct DatabaseTopology {
    pools: HashMap<DbRole, MssqlPool>,

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

    pub fn empty() -> Self {
        Self {
            pools: HashMap::new(),
            primary_role: None,
        }
    }

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

    pub fn primary_role(&self) -> Option<DbRole> {
        self.primary_role
    }

    pub fn get(&self, role: DbRole) -> &MssqlPool {
        self.pools
            .get(&role)
            .unwrap_or_else(|| panic!("database topology: role '{role}' not configured"))
    }

    pub fn try_get(&self, role: DbRole) -> Option<&MssqlPool> {
        self.pools.get(&role)
    }

    pub fn live_roles(&self) -> Vec<DbRole> {
        DbRole::ALL
            .iter()
            .copied()
            .filter(|r| self.pools.contains_key(r))
            .collect()
    }

    pub async fn health_check(&self) -> HashMap<DbRole, Result<(), MssqlError>> {
        let mut out: HashMap<DbRole, Result<(), MssqlError>> = HashMap::new();
        for role in self.live_roles() {
            let pool = self.pools.get(&role).expect("live role");
            let r = mssql_ping(pool).await;
            out.insert(role, r);
        }
        out
    }

    pub fn is_empty(&self) -> bool {
        self.pools.is_empty()
    }

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

        let s = DatabaseTopologySettings {
            catch_all: DatabaseSettings::from_url("Server=default;Database=D;User Id=u;Password=p"),
            order: DatabaseSettings::from_url(
                "Server=orderhost;Database=ORDER;User Id=u;Password=p",
            ),
            ..DatabaseTopologySettings::default()
        };
        let order = s.for_role(DbRole::Order);

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
