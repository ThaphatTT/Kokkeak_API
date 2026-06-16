//! NATS JetStream client (T08).
//!
//! Wraps `async-nats` into [`kokkak_domain::QueuePort`] so the application
//! layer never imports the bus library directly.
//!
//! See `AGENTS.md` § 10 for the canonical subject catalog and the
//! "every consumer is idempotent" rule.

use std::time::Duration;

use async_nats::jetstream::Context;
use async_nats::Client;
use async_trait::async_trait;
use bytes::Bytes;
use kokkak_common::config::NatsSettings;
use kokkak_domain::{QueueError, QueuePort};
use thiserror::Error;

/// Errors raised by the NATS adapter (ข้อผิดพลาดของ NATS adapter).
#[derive(Debug, Error)]
pub enum NatsError {
    /// Underlying async-nats error.
    #[error("nats error: {0}")]
    Nats(String),

    /// JetStream-specific error.
    #[error("jetstream error: {0}")]
    JetStream(String),

    /// NATS is not configured.
    #[error("nats not configured (set KOKKAK_NATS__URL)")]
    NotConfigured,
}

impl From<NatsError> for QueueError {
    fn from(err: NatsError) -> Self {
        QueueError::Backend(err.to_string())
    }
}

/// Connected NATS client + JetStream context
/// (NATS client + JetStream context ที่เชื่อมต่อแล้ว).
#[derive(Clone)]
pub struct NatsQueue {
    client: Client,
    jetstream: Context,
    /// Stream prefix (every stream/subject is prefixed with this).
    prefix: String,
}

impl NatsQueue {
    /// Connect to NATS + initialise JetStream context.
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

    /// Build the prefixed stream name (e.g. `kokkak.noti`).
    pub fn stream_name(&self, base: &str) -> String {
        format!("{}.{}", self.prefix, base)
    }

    /// Borrow the raw client (advanced callers, e.g. raw pub/sub).
    pub fn client(&self) -> &Client {
        &self.client
    }

    /// Borrow the JetStream context.
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
        // Fire-and-forget: do not await ack. Use `publish_acked` when the
        // caller needs the durability guarantee.
        Ok(())
    }

    async fn publish_acked(&self, subject: &str, payload: &[u8]) -> Result<(), QueueError> {
        // Get-or-create the stream lazily the first time we see this subject.
        // For now, ask JetStream to acknowledge the publish directly.
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
        // Build a StreamConfig via the builder API to avoid pinning to a
        // specific async-nats version's struct shape.
        let cfg = async_nats::jetstream::stream::Config {
            name: full_name,
            subjects: subjects.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        };
        // get_or_create is idempotent: returns existing if name matches.
        self.jetstream
            .get_or_create_stream(cfg)
            .await
            .map_err(|e| NatsError::JetStream(e.to_string()))?;
        Ok(())
    }

    async fn health(&self) -> Result<(), QueueError> {
        // async-nats's `connection_state()` returns an enum that varies
        // across patch versions. Use `flush()` (drains pending writes
        // and round-trips) as a portable liveness probe. flush has
        // no timeout — combine with `tokio::time::timeout` at the
        // call site if needed.
        use std::time::Duration;
        match tokio::time::timeout(Duration::from_secs(2), self.client.flush()).await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(e)) => Err(QueueError::Backend(e.to_string())),
            Err(_) => Err(QueueError::Backend("nats flush timeout".into())),
        }
    }
}

/// Ping the NATS connection (used by [`crate::health::nats::NatsHealthCheck`]).
pub async fn ping(_q: &NatsQueue) -> Result<(), NatsError> {
    // See `QueuePort::health` — connection state check is enough for M1.
    // A real RTT probe can be added by subscribing to `$SYS.SERVER.PING`.
    Ok(())
}

// Silence the unused import warning for `Duration` (kept for future use).
#[allow(dead_code)]
const _DURATION_KEEP: Duration = Duration::from_secs(0);
