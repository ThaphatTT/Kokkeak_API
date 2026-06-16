//! Queue port (พอร์ตคิว — T08).
//!
//! Trait every async-message-bus adapter (NATS JetStream today, possibly
//! Kafka in the future) must implement. Application code depends on
//! the trait only — concrete clients live in `infra::queue`.

use async_trait::async_trait;
use bytes::Bytes;
use thiserror::Error;

/// Queue operation errors (ข้อผิดพลาดของคิว).
#[derive(Debug, Error)]
pub enum QueueError {
    /// Underlying bus / connection failure.
    #[error("queue backend error: {0}")]
    Backend(String),

    /// Subject / stream does not exist.
    #[error("queue not found: {0}")]
    NotFound(String),

    /// Serialization / deserialization failure.
    #[error("queue codec error: {0}")]
    Codec(String),
}

/// One message in the queue (ข้อความเดียวในคิว).
#[derive(Debug, Clone)]
pub struct QueueMessage {
    /// Subject the message was published to (e.g. `noti.push`).
    pub subject: String,
    /// Raw payload (encoded by the publisher).
    pub payload: Bytes,
}

/// Port every queue adapter must satisfy.
#[async_trait]
pub trait QueuePort: Send + Sync {
    /// Publish a single message to `subject`.
    async fn publish(&self, subject: &str, payload: &[u8]) -> Result<(), QueueError>;

    /// Publish and **await** an ack from the server (JetStream semantics).
    /// Adapters that do not support ack'd publish can fall back to
    /// fire-and-forget [`Self::publish`].
    async fn publish_acked(&self, subject: &str, payload: &[u8]) -> Result<(), QueueError>;

    /// Ensure the stream backing `subject` exists. Idempotent.
    /// Concrete adapters translate this into the bus-native concept
    /// (JetStream stream, Kafka topic, ...).
    async fn ensure_stream(&self, stream_name: &str, subjects: &[&str]) -> Result<(), QueueError>;

    /// Health check: verify the connection to the bus is live.
    /// Returns `Ok(())` when pinged, `Err(QueueError::Backend(_))` otherwise.
    async fn health(&self) -> Result<(), QueueError>;
}
