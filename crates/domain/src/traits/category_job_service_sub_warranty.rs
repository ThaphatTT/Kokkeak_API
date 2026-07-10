use async_trait::async_trait;

use super::user::RepoError;
use crate::category_job_service_sub_warranty::{
    CategoryJobServiceSubWarrantyAutocompleteInput, CategoryJobServiceSubWarrantyAutocompleteRow,
    CategoryJobServiceSubWarrantyCreateInput, CategoryJobServiceSubWarrantyCreateResult,
    CategoryJobServiceSubWarrantyDeleteInput, CategoryJobServiceSubWarrantyDeleteResult,
    CategoryJobServiceSubWarrantyFullDetailRow, CategoryJobServiceSubWarrantyListInput,
    CategoryJobServiceSubWarrantyPage, CategoryJobServiceSubWarrantyUpdateInput,
    CategoryJobServiceSubWarrantyUpdateResult,
};

#[async_trait]
pub trait CategoryJobServiceSubWarrantyRepository: Send + Sync {
    async fn list(
        &self,
        input: &CategoryJobServiceSubWarrantyListInput,
    ) -> Result<CategoryJobServiceSubWarrantyPage, RepoError>;

    async fn create(
        &self,
        input: &CategoryJobServiceSubWarrantyCreateInput,
    ) -> Result<CategoryJobServiceSubWarrantyCreateResult, RepoError>;

    async fn update(
        &self,
        input: &CategoryJobServiceSubWarrantyUpdateInput,
    ) -> Result<CategoryJobServiceSubWarrantyUpdateResult, RepoError>;

    async fn delete(
        &self,
        input: &CategoryJobServiceSubWarrantyDeleteInput,
    ) -> Result<CategoryJobServiceSubWarrantyDeleteResult, RepoError>;

    async fn autocomplete(
        &self,
        input: &CategoryJobServiceSubWarrantyAutocompleteInput,
    ) -> Result<Vec<CategoryJobServiceSubWarrantyAutocompleteRow>, RepoError>;

    async fn detail(
        &self,
        category_job_service_sub_warranty_guid: &str,
    ) -> Result<Option<CategoryJobServiceSubWarrantyFullDetailRow>, RepoError>;
}
