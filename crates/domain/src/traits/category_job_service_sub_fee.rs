use async_trait::async_trait;

use super::user::RepoError;
use crate::category_job_service_sub_fee::{
    CategoryJobServiceSubFeeCreateInput, CategoryJobServiceSubFeeCreateResult,
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
}
