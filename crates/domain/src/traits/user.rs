

use async_trait::async_trait;
use thiserror::Error;
use uuid::Uuid;

use crate::admin_user::{
    AdminInsertUserError, AdminInsertUserRequest, AdminInsertUserResult, AdminUpdateUserError,
    AdminUpdateUserRequest, AdminUpdateUserResult, AdminUserDetail, AdminUserListPagingInput,
    AdminUserListPagingPage,
};
use crate::user::{User, UserListRow};

#[derive(Debug, Error)]
pub enum RepoError {

    #[error("not found: {0}")]
    NotFound(String),

    #[error("conflict: {0}")]
    Conflict(String),

    #[error("backend error: {0}")]
    Backend(String),
}

#[async_trait]
pub trait UserRepository: Send + Sync {

    async fn find_by_id(&self, id: Uuid) -> Result<Option<User>, RepoError>;

    async fn find_by_username(&self, username: &str) -> Result<Option<User>, RepoError>;

    async fn insert(&self, user: &User) -> Result<(), RepoError>;

    async fn update(&self, user: &User) -> Result<(), RepoError>;

    async fn list_with_permissions(&self, caller_guid: Uuid)
        -> Result<Vec<UserListRow>, RepoError>;

    async fn find_username_guid_by_user_guid(
        &self,
        user_guid: uuid::Uuid,
    ) -> Result<Option<String>, RepoError>;

    async fn admin_insert_full(
        &self,
        req: &AdminInsertUserRequest,
    ) -> Result<AdminInsertUserResult, AdminInsertUserError>;

    async fn list_users_paging(
        &self,
        input: &AdminUserListPagingInput,
        actor: Uuid,
    ) -> Result<AdminUserListPagingPage, RepoError> {
        let _ = (input, actor);
        Err(RepoError::Backend(
            "list_users_paging: not implemented by this repository adapter".into(),
        ))
    }

    async fn get_user_detail_full(
        &self,
        user_guid: Uuid,
        actor: Uuid,
    ) -> Result<Option<AdminUserDetail>, RepoError> {
        let _ = (user_guid, actor);
        Err(RepoError::Backend(
            "get_user_detail_full: not implemented by this repository adapter".into(),
        ))
    }

    async fn admin_update_full(
        &self,
        req: &AdminUpdateUserRequest,
    ) -> Result<AdminUpdateUserResult, AdminUpdateUserError> {
        let _ = req;
        Err(AdminUpdateUserError::new(
            "internal",
            "admin_update_full: not implemented by this repository adapter",
        ))
    }
}
