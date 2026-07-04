

use async_trait::async_trait;

use crate::master::{
    MasterDropdownRow, MasterPositionAutocompleteRow, UserDepartmentTeamAutocompleteRow,
};
use crate::traits::user::RepoError;

#[async_trait]
pub trait MasterDropdownRepository: Send + Sync {

    async fn list_countries(
        &self,
        keyword: Option<&str>,
        status: Option<i32>,
    ) -> Result<Vec<MasterDropdownRow>, RepoError>;

    async fn autocomplete_user_department(
        &self,
        keyword: Option<&str>,
        take: Option<i32>,
    ) -> Result<Vec<MasterDropdownRow>, RepoError>;

    async fn autocomplete_master_positions(
        &self,
        user_department_team_guid: Option<&str>,
        keyword: Option<&str>,
        take: Option<i32>,
    ) -> Result<Vec<MasterPositionAutocompleteRow>, RepoError>;

    async fn autocomplete_user_department_team(
        &self,
        user_department_guid: Option<&str>,
        keyword: Option<&str>,
        take: Option<i32>,
    ) -> Result<Vec<UserDepartmentTeamAutocompleteRow>, RepoError>;
}
