//! Service-category repository port (พอร์ต catalog — M3).

use async_trait::async_trait;
use uuid::Uuid;

use crate::catalog::ServiceCategory;
use crate::pagination::Cursor;

use super::user::RepoError;

/// Service-category repository contract.
#[async_trait]
pub trait ServiceRepository: Send + Sync {
    /// Find by primary key.
    async fn find_by_id(&self, id: Uuid) -> Result<Option<ServiceCategory>, RepoError>;

    /// Find by the stable `code` (e.g. `"ac-not-cooling"`).
    async fn find_by_code(&self, code: &str) -> Result<Option<ServiceCategory>, RepoError>;

    /// List active categories, sorted by `sort_order` ascending.
    /// `limit` caps the page size; `after` is the keyset cursor
    /// (returns rows whose `sort_order > after`).
    async fn list_active(
        &self,
        after: Option<Cursor>,
        limit: u32,
    ) -> Result<Vec<ServiceCategory>, RepoError>;

    /// Persist a new category. Returns `Conflict` if the code is taken.
    async fn insert(&self, service: &ServiceCategory) -> Result<(), RepoError>;
}
