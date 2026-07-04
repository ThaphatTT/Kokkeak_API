

use std::sync::Arc;

use async_trait::async_trait;
use kokkak_domain::{HealthCheck, HealthError};

use crate::db::mssql::{ping, MssqlPool};
use crate::db::topology::DatabaseTopology;

pub struct SqlServerHealthCheck {
    pool: Arc<MssqlPool>,
}

impl SqlServerHealthCheck {

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

pub struct MultiDbHealthCheck {
    topology: Arc<DatabaseTopology>,
}

impl MultiDbHealthCheck {

    pub fn new(topology: Arc<DatabaseTopology>) -> Self {
        Self { topology }
    }
}

#[async_trait]
impl HealthCheck for MultiDbHealthCheck {
    fn name(&self) -> &str {

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

        assert!(check.check().await.is_ok());
    }
}
