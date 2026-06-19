//! `HealthCheck` for SQL Server (เช็คสถานะ SQL Server).
//!
//! Two flavours:
//!
//! - [`SqlServerHealthCheck`] — pings a single pool. Used by
//!   M0-M11 callers that only know about one pool.
//! - [`MultiDbHealthCheck`] — pings every live role in a
//!   [`DatabaseTopology`]. Used by the M12 factory once the
//!   operator has declared per-role URLs. Returns the per-role
//!   status as a `name = "sqlserver[master+order+...]"` label
//!   so `/readyz` shows the failing role on a multi-DB outage.
//!
//! Both run `SELECT 1` through the pool. The check is cheap
//! and counts as a connection-acquisition stress test.

use std::sync::Arc;

use async_trait::async_trait;
use kokkak_domain::{HealthCheck, HealthError};

use crate::db::mssql::{ping, MssqlPool};
use crate::db::topology::DatabaseTopology;

/// `HealthCheck` that runs `SELECT 1` on a single pool.
pub struct SqlServerHealthCheck {
    pool: Arc<MssqlPool>,
}

impl SqlServerHealthCheck {
    /// Wrap an existing pool.
    pub fn new(pool: Arc<MssqlPool>) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl HealthCheck for SqlServerHealthCheck {
    fn name(&self) -> &str {
        "sqlserver"
    }

    async fn check(&self) -> Result<(), HealthError> {
        ping(&self.pool)
            .await
            .map_err(|e| HealthError::Failed(e.to_string()))
    }
}

/// `HealthCheck` that pings every live role in a
/// [`DatabaseTopology`]. The check passes when **all** live
/// roles pass; any single failure fails the whole check, with
/// the failing role(s) named in the error.
pub struct MultiDbHealthCheck {
    topology: Arc<DatabaseTopology>,
}

impl MultiDbHealthCheck {
    /// Wrap a topology. The check is cheap (O(live_roles) +
    /// one `SELECT 1` per role).
    pub fn new(topology: Arc<DatabaseTopology>) -> Self {
        Self { topology }
    }
}

#[async_trait]
impl HealthCheck for MultiDbHealthCheck {
    fn name(&self) -> &str {
        // Single label across the multi-DB health check. The
        // per-role status is in the body of the `Err`. Operators
        // grep `/readyz` output for `sqlserver` and read the
        // body for which role failed.
        "sqlserver"
    }

    async fn check(&self) -> Result<(), HealthError> {
        let statuses = self.topology.health_check().await;
        let mut failed: Vec<String> = Vec::new();
        let mut ok = 0;
        for (role, result) in &statuses {
            match result {
                Ok(()) => ok += 1,
                Err(e) => failed.push(format!("{role}={e}")),
            }
        }
        if failed.is_empty() {
            Ok(())
        } else {
            Err(HealthError::Failed(format!(
                "{ok} ok, {} failed: {}",
                failed.len(),
                failed.join(", ")
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::topology::DatabaseTopology;

    #[test]
    fn multi_db_health_check_name_is_stable() {
        let topo = DatabaseTopology::empty();
        let check = MultiDbHealthCheck::new(Arc::new(topo));
        assert_eq!(check.name(), "sqlserver");
    }

    #[tokio::test]
    async fn multi_db_health_check_with_empty_topology_is_noop() {
        let topo = DatabaseTopology::empty();
        let check = MultiDbHealthCheck::new(Arc::new(topo));
        // No live roles → trivially healthy.
        assert!(check.check().await.is_ok());
    }
}
