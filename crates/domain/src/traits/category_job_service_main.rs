use async_trait::async_trait;

use super::user::RepoError;
use crate::category_job_service_main::{
    CategoryJobServiceMainAutocompleteInput, CategoryJobServiceMainAutocompleteRow,
    CategoryJobServiceMainCreateInput, CategoryJobServiceMainCreateResult,
    CategoryJobServiceMainDeleteResult, CategoryJobServiceMainListInput, CategoryJobServiceMainRow,
    CategoryJobServiceMainUpdateInput, CategoryJobServiceMainUpdateResult,
};

#[async_trait]
pub trait CategoryJobServiceMainRepository: Send + Sync {
    async fn list(
        &self,
        input: &CategoryJobServiceMainListInput,
    ) -> Result<Vec<CategoryJobServiceMainRow>, RepoError>;

    async fn create(
        &self,
        input: &CategoryJobServiceMainCreateInput,
    ) -> Result<CategoryJobServiceMainCreateResult, RepoError>;

    async fn update(
        &self,
        input: &CategoryJobServiceMainUpdateInput,
    ) -> Result<CategoryJobServiceMainUpdateResult, RepoError>;

    async fn delete(
        &self,
        service_guid: &str,
        actor_user_guid: &str,
    ) -> Result<CategoryJobServiceMainDeleteResult, RepoError>;

    async fn autocomplete(
        &self,
        input: &CategoryJobServiceMainAutocompleteInput,
    ) -> Result<Vec<CategoryJobServiceMainAutocompleteRow>, RepoError>;
}
