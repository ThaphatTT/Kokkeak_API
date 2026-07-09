use async_trait::async_trait;

use super::user::RepoError;
use crate::category_job_service_sub_fee::{
    CategoryJobServiceSubFeeAutocompleteInput, CategoryJobServiceSubFeeAutocompleteRow,
    CategoryJobServiceSubFeeCreateInput, CategoryJobServiceSubFeeCreateResult,
    CategoryJobServiceSubFeeDeleteInput, CategoryJobServiceSubFeeDeleteResult,
    CategoryJobServiceSubFeeListInput, CategoryJobServiceSubFeePage,
    CategoryJobServiceSubFeeUpdateInput, CategoryJobServiceSubFeeUpdateResult,
};

#[async_trait]
pub trait CategoryJobServiceSubFeeRepository: Send + Sync {
    async fn list(
        &self,
        input: &CategoryJobServiceSubFeeListInput,
    ) -> Result<CategoryJobServiceSubFeePage, RepoError>;

    async fn create(
        &self,
        input: &CategoryJobServiceSubFeeCreateInput,
    ) -> Result<CategoryJobServiceSubFeeCreateResult, RepoError>;

    async fn update(
        &self,
        input: &CategoryJobServiceSubFeeUpdateInput,
    ) -> Result<CategoryJobServiceSubFeeUpdateResult, RepoError>;

    async fn delete(
        &self,
        input: &CategoryJobServiceSubFeeDeleteInput,
    ) -> Result<CategoryJobServiceSubFeeDeleteResult, RepoError>;

    async fn autocomplete(
        &self,
        input: &CategoryJobServiceSubFeeAutocompleteInput,
    ) -> Result<Vec<CategoryJobServiceSubFeeAutocompleteRow>, RepoError>;
}
