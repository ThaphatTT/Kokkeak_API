

use async_trait::async_trait;
use bytes::Bytes;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum QueueError {

    #[error("queue backend error: {0}")]
    Backend(String),

    #[error("queue not found: {0}")]
    NotFound(String),

    #[error("queue codec error: {0}")]
    Codec(String),
}

#[derive(Debug, Clone)]
pub struct QueueMessage {

    pub subject: String,

    pub payload: Bytes,
}

#[async_trait]
pub trait QueuePort: Send + Sync {

    async fn publish(&self, subject: &str, payload: &[u8]) -> Result<(), QueueError>;

    async fn publish_acked(&self, subject: &str, payload: &[u8]) -> Result<(), QueueError>;

    async fn ensure_stream(&self, stream_name: &str, subjects: &[&str]) -> Result<(), QueueError>;

    async fn health(&self) -> Result<(), QueueError>;
}
