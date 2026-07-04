

use async_trait::async_trait;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum TranslationError {

    #[error("translation backend error: {0}")]
    Backend(String),
}

#[async_trait]
pub trait TranslationRepository: Send + Sync {

    async fn get(&self, locale: &str, key: &str) -> Result<Option<String>, TranslationError>;

    async fn put(&self, locale: &str, key: &str, value: &str) -> Result<(), TranslationError>;

    async fn count(&self) -> Result<usize, TranslationError> {
        Ok(0)
    }
}
