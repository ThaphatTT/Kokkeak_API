

use async_trait::async_trait;
use uuid::Uuid;

use crate::permission::{
    PermissionOverrideUpdateItem, PermissionOverrideUpdateResult, PermissionUserDetailRow,
    PermissionUserListRow,
};
use crate::traits::user::RepoError;

#[async_trait]
pub trait PermissionUserRepository: Send + Sync {

    async fn list_permission_users(
        &self,
        caller_guid: Uuid,
    ) -> Result<Vec<PermissionUserListRow>, RepoError>;

    async fn find_permission_user_detail(
        &self,
        user_guid: Uuid,
        caller_guid: Uuid,
    ) -> Result<Vec<PermissionUserDetailRow>, RepoError>;

    async fn update_permission_overrides(
        &self,
        items: &[PermissionOverrideUpdateItem],
        update_by: &str,
    ) -> Result<Vec<PermissionOverrideUpdateResult>, RepoError>;
}
