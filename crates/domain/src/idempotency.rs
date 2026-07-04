

use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachedResponse {

    pub status: u16,

    pub content_type: String,

    pub body: Vec<u8>,
}

#[async_trait::async_trait]
pub trait IdempotencyStore: Send + Sync {

    async fn get(&self, key: &str) -> Option<CachedResponse>;

    async fn put(&self, key: &str, response: CachedResponse, ttl: Duration);

    fn len(&self) -> usize;

    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
