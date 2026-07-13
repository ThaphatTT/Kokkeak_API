use async_trait::async_trait;

use super::user::RepoError;
use crate::category_job_main::{
    CategoryJobMainAutocompleteInput, CategoryJobMainAutocompleteRow, CategoryJobMainCreateInput,
    CategoryJobMainCreateResult, CategoryJobMainDeleteResult, CategoryJobMainDetailRow,
    CategoryJobMainListInput, CategoryJobMainPage, CategoryJobMainUpdateInput,
    CategoryJobMainUpdateResult,
};

#[async_trait]
pub trait CategoryJobMainRepository: Send + Sync {
    async fn list(
        &self,
        input: &CategoryJobMainListInput,
    ) -> Result<CategoryJobMainPage, RepoError>;

    async fn create(
        &self,
        input: &CategoryJobMainCreateInput,
    ) -> Result<CategoryJobMainCreateResult, RepoError>;

    async fn update(
        &self,
        input: &CategoryJobMainUpdateInput,
    ) -> Result<CategoryJobMainUpdateResult, RepoError>;

    async fn delete(
        &self,
        category_guid: &str,
        actor_user_guid: &str,
    ) -> Result<CategoryJobMainDeleteResult, RepoError>;

    async fn autocomplete(
        &self,
        input: &CategoryJobMainAutocompleteInput,
    ) -> Result<Vec<CategoryJobMainAutocompleteRow>, RepoError>;

    async fn detail(
        &self,
        category_guid: &str,
    ) -> Result<Option<CategoryJobMainDetailRow>, RepoError>;
}
