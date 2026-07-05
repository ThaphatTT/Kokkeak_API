use async_trait::async_trait;

use super::user::RepoError;
use crate::category_job_service_main::{
    CategoryJobServiceMainCreateInput, CategoryJobServiceMainCreateResult,
    CategoryJobServiceMainDeleteResult, CategoryJobServiceMainRow,
    CategoryJobServiceMainUpdateInput, CategoryJobServiceMainUpdateResult,
};

#[async_trait]
pub trait CategoryJobServiceMainRepository: Send + Sync {
    async fn list(
        &self,
        category_job_main_guid: &str,
        keyword: Option<&str>,
        include_inactive: bool,
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
}
