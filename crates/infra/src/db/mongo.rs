

use std::time::Duration;

use bson::Document;
use kokkak_common::config::MongoSettings;
use mongodb::options::ClientOptions;
use mongodb::{Client, Collection, Database};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum MongoError {

    #[error("mongodb driver error: {0}")]
    Driver(#[from] mongodb::error::Error),

    #[error("mongodb not configured (set KOKKAK_MONGO__URL)")]
    NotConfigured,
}

#[derive(Clone)]
pub struct MongoClient {
    inner: Client,
    db: Database,
}

impl MongoClient {

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

    pub fn database(&self) -> &Database {
        &self.db
    }

    pub fn collection<T: Send + Sync>(&self, name: &str) -> Collection<T> {
        self.db.collection(name)
    }

    pub async fn ping(&self) -> Result<(), MongoError> {
        self.inner
            .database("admin")
            .run_command(Document::from_iter([("ping".into(), 1.into())]))
            .await?;
        Ok(())
    }
}
