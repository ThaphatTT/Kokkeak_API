

use async_trait::async_trait;
use uuid::Uuid;

use crate::permission::{PermissionUpdateRow, UserRolePermissionRow};
use crate::traits::user::RepoError;

#[async_trait]
pub trait UserRoleRepository: Send + Sync {

    async fn list_permissions(
        &self,
        mode: &str,
        caller_guid: Uuid,
    ) -> Result<Vec<UserRolePermissionRow>, RepoError>;

    async fn update_role_permission(
        &self,
        role_guid: &str,
        permission_guid: &str,
        status: i32,
        update_by: Option<&str>,
    ) -> Result<PermissionUpdateRow, RepoError>;
}
