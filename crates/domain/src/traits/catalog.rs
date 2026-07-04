

use async_trait::async_trait;
use uuid::Uuid;

use crate::catalog::ServiceCategory;
use crate::pagination::Cursor;

use super::user::RepoError;

#[async_trait]
pub trait ServiceRepository: Send + Sync {

    async fn find_by_id(&self, id: Uuid) -> Result<Option<ServiceCategory>, RepoError>;

    async fn find_by_code(&self, code: &str) -> Result<Option<ServiceCategory>, RepoError>;

    async fn list_active(
        &self,
        after: Option<Cursor>,
        limit: u32,
    ) -> Result<Vec<ServiceCategory>, RepoError>;

    async fn insert(&self, service: &ServiceCategory) -> Result<(), RepoError>;
}
