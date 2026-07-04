

use std::time::Duration;

use async_nats::jetstream::Context;
use async_nats::Client;
use async_trait::async_trait;
use bytes::Bytes;
use kokkak_common::config::NatsSettings;
use kokkak_domain::{QueueError, QueuePort};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum NatsError {

    #[error("nats error: {0}")]
    Nats(String),

    #[error("jetstream error: {0}")]
    JetStream(String),

    #[error("nats not configured (set KOKKAK_NATS__URL)")]
    NotConfigured,
}

impl From<NatsError> for QueueError {
    fn from(err: NatsError) -> Self {
        QueueError::Backend(err.to_string())
    }
}

#[derive(Clone)]
pub struct NatsQueue {
    client: Client,
    jetstream: Context,

    prefix: String,
}

impl NatsQueue {

    pub async fn connect(settings: &NatsSettings) -> Result<Self, NatsError> {
        if !settings.is_configured() {
            return Err(NatsError::NotConfigured);
        }

        let client = async_nats::connect(&settings.url)
            .await
            .map_err(|e| NatsError::Nats(e.to_string()))?;
        let jetstream = async_nats::jetstream::new(client.clone());

        tracing::info!(
            url = %settings.url,
            prefix = %settings.stream_prefix,
            "nats jetstream client built"
        );

        Ok(Self {
            client,
            jetstream,
            prefix: settings.stream_prefix.clone(),
        })
    }

    pub fn stream_name(&self, base: &str) -> String {
        format!("{}.{}", self.prefix, base)
    }

    pub fn client(&self) -> &Client {
        &self.client
    }

    pub fn jetstream(&self) -> &Context {
        &self.jetstream
    }
}

#[async_trait]
impl QueuePort for NatsQueue {
    async fn publish(&self, subject: &str, payload: &[u8]) -> Result<(), QueueError> {
        self.client
            .publish(subject.to_string(), Bytes::copy_from_slice(payload))
            .await
            .map_err(|e| QueueError::Backend(e.to_string()))?;

        Ok(())
    }

    async fn publish_acked(&self, subject: &str, payload: &[u8]) -> Result<(), QueueError> {

        let ack_fut = self
            .jetstream
            .publish(subject.to_string(), Bytes::copy_from_slice(payload))
            .await
            .map_err(|e| QueueError::Backend(e.to_string()))?;
        ack_fut
            .await
            .map_err(|e| QueueError::Backend(e.to_string()))?;
        Ok(())
    }

    async fn ensure_stream(&self, stream_name: &str, subjects: &[&str]) -> Result<(), QueueError> {
        let full_name = self.stream_name(stream_name);

        let cfg = async_nats::jetstream::stream::Config {
            name: full_name,
            subjects: subjects.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        };

        self.jetstream
            .get_or_create_stream(cfg)
            .await
            .map_err(|e| NatsError::JetStream(e.to_string()))?;
        Ok(())
    }

    async fn health(&self) -> Result<(), QueueError> {

        use std::time::Duration;
        match tokio::time::timeout(Duration::from_secs(2), self.client.flush()).await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(e)) => Err(QueueError::Backend(e.to_string())),
            Err(_) => Err(QueueError::Backend("nats flush timeout".into())),
        }
    }
}

pub async fn ping(_q: &NatsQueue) -> Result<(), NatsError> {

    Ok(())
}

#[allow(dead_code)]
const _DURATION_KEEP: Duration = Duration::from_secs(0);
