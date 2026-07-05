use async_trait::async_trait;

use super::user::RepoError;
use crate::category_job_service_sub::{
    CategoryJobServiceSubCreateInput, CategoryJobServiceSubCreateResult,
    CategoryJobServiceSubDeleteResult, CategoryJobServiceSubDetailBundle,
    CategoryJobServiceSubImageCreateInput, CategoryJobServiceSubImageCreateResult,
    CategoryJobServiceSubImageDeleteInput, CategoryJobServiceSubImageDeleteResult,
    CategoryJobServiceSubImageInput, CategoryJobServiceSubImageRow, CategoryJobServiceSubRow,
    CategoryJobServiceSubUpdateInput, CategoryJobServiceSubUpdateResult,
};

#[derive(Debug, Clone)]
pub struct SubImageForCreate {
    pub img_type: i32,

    pub img_priority: i32,

    pub img_path: String,
}

#[derive(Debug, Clone)]
pub struct SubImageForUpdate {
    pub img_type: i32,

    pub img_priority: i32,

    pub img_path: String,
}

#[async_trait]
pub trait CategoryJobServiceSubRepository: Send + Sync {
    async fn list(
        &self,
        category_job_service_guid: &str,
        keyword: Option<&str>,
        include_inactive: bool,
    ) -> Result<Vec<CategoryJobServiceSubRow>, RepoError>;

    async fn detail(
        &self,
        category_job_service_sub_guid: &str,
    ) -> Result<CategoryJobServiceSubDetailBundle, RepoError>;

    async fn list_images(
        &self,
        category_job_service_sub_guid: &str,
    ) -> Result<Vec<CategoryJobServiceSubImageRow>, RepoError>;

    async fn create(
        &self,
        input: &CategoryJobServiceSubCreateInput,
    ) -> Result<CategoryJobServiceSubCreateResult, RepoError>;

    async fn update(
        &self,
        input: &CategoryJobServiceSubUpdateInput,
    ) -> Result<CategoryJobServiceSubUpdateResult, RepoError>;

    async fn delete(
        &self,
        category_job_service_sub_guid: &str,
        actor_user_guid: &str,
    ) -> Result<CategoryJobServiceSubDeleteResult, RepoError>;

    async fn create_image(
        &self,
        input: &CategoryJobServiceSubImageCreateInput,
    ) -> Result<CategoryJobServiceSubImageCreateResult, RepoError>;

    async fn delete_image(
        &self,
        input: &CategoryJobServiceSubImageDeleteInput,
    ) -> Result<CategoryJobServiceSubImageDeleteResult, RepoError>;

    async fn create_with_images(
        &self,
        input: &CategoryJobServiceSubCreateInput,
        image_paths: &[SubImageForCreate],
    ) -> Result<CategoryJobServiceSubCreateResult, RepoError>;

    async fn update_with_images(
        &self,
        input: &CategoryJobServiceSubUpdateInput,
        image_paths: &[SubImageForUpdate],
    ) -> Result<CategoryJobServiceSubUpdateResult, RepoError>;
}

#[allow(dead_code)]
pub fn _suppress(_img: &CategoryJobServiceSubImageInput) {}
