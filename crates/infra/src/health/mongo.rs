//! `HealthCheck` for MongoDB (เช็คสถานะ MongoDB).

use std::sync::Arc;

use async_trait::async_trait;
use kokkak_domain::{HealthCheck, HealthError};

use crate::db::mongo::MongoClient;

/// `HealthCheck` that runs `{ping: 1}` against the admin DB.
pub struct MongoHealthCheck {
    client: Arc<MongoClient>,
}

impl MongoHealthCheck {
    /// Wrap an existing Mongo client.
    pub fn new(client: Arc<MongoClient>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl HealthCheck for MongoHealthCheck {
    fn name(&self) -> &str {
        "mongo"
    }

    async fn check(&self) -> Result<(), HealthError> {
        self.client
            .ping()
            .await
            .map_err(|e| HealthError::Failed(e.to_string()))
    }
}
