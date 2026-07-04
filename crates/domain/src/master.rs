

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct MasterDropdownRow {

    pub value: String,

    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct MasterPositionAutocompleteRow {

    pub value: String,

    pub label: String,

    pub code: String,

    pub description: String,

    pub level: i32,

    pub status: i32,

    pub user_department_team_guid: String,

    pub user_department_team_code: String,

    pub user_department_team_name: String,

    pub user_department_guid: String,

    pub user_department_code: String,

    pub user_department_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct UserDepartmentTeamAutocompleteRow {

    pub value: String,

    pub label: String,

    pub user_department_team_guid: String,

    pub user_department_team_code: String,

    pub user_department_team_name: String,

    pub user_department_team_status: i32,

    pub user_department_guid: String,

    pub user_department_code: String,

    pub user_department_name: String,
}
