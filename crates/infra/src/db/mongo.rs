//! MongoDB client (T09 part a).
//!
//! Wraps the official `mongodb` driver into a thin helper that exposes
//! collection access + a liveness probe. Migration runner lives in
//! the sibling `migrate` module.

use std::time::Duration;

use bson::Document;
use kokkak_common::config::MongoSettings;
use mongodb::options::ClientOptions;
use mongodb::{Client, Collection, Database};
use thiserror::Error;

/// Errors raised by the Mongo client (ข้อผิดพลาดของ Mongo client).
#[derive(Debug, Error)]
pub enum MongoError {
    /// Driver-level error (connection, options, ...).
    #[error("mongodb driver error: {0}")]
    Driver(#[from] mongodb::error::Error),

    /// Mongo is not configured.
    #[error("mongodb not configured (set KOKKAK_MONGO__URL)")]
    NotConfigured,
}

/// Connected Mongo client handle
/// (handle ของ Mongo client ที่เชื่อมต่อแล้ว).
#[derive(Clone)]
pub struct MongoClient {
    inner: Client,
    db: Database,
}

impl MongoClient {
    /// Connect to MongoDB using the configured URL + database name.
    pub async fn connect(settings: &MongoSettings) -> Result<Self, MongoError> {
        if !settings.is_configured() {
            return Err(MongoError::NotConfigured);
        }

        let mut opts = ClientOptions::parse(&settings.url).await?;
        opts.app_name = Some("kokkak-api".to_string());
        opts.connect_timeout = Some(Duration::from_secs(5));
        opts.server_selection_timeout = Some(Duration::from_secs(5));

        let client = Client::with_options(opts)?;
        let db = client.database(&settings.database);

        tracing::info!(
            url = %settings.url,
            database = %settings.database,
            "mongodb client built"
        );

        Ok(Self { inner: client, db })
    }

    /// Access the underlying database handle (advanced callers only).
    pub fn database(&self) -> &Database {
        &self.db
    }

    /// Access a typed collection by name.
    pub fn collection<T: Send + Sync>(&self, name: &str) -> Collection<T> {
        self.db.collection(name)
    }

    /// Liveness probe: sends `{ping: 1}` to the admin database.
    pub async fn ping(&self) -> Result<(), MongoError> {
        self.inner
            .database("admin")
            .run_command(Document::from_iter([("ping".into(), 1.into())]))
            .await?;
        Ok(())
    }
}
