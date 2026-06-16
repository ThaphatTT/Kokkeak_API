//! `HealthCheck` for SQL Server (เช็คสถานะ SQL Server).
//!
//! Runs `SELECT 1` through the existing pool. The check is cheap and
//! counts as a connection-acquisition stress test.

use std::sync::Arc;

use async_trait::async_trait;
use kokkak_domain::{HealthCheck, HealthError};

use crate::db::mssql::{ping, MssqlPool};

/// `HealthCheck` that runs `SELECT 1` on the given pool.
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
