

use async_trait::async_trait;
use tiberius::ToSql;

use kokkak_domain::traits::master::MasterDropdownRepository;
use kokkak_domain::traits::user::RepoError;
use kokkak_domain::{
    MasterDropdownRow, MasterPositionAutocompleteRow, UserDepartmentTeamAutocompleteRow,
};

use crate::db::mssql::{exec_sp, read_i32, read_str, MssqlPool};

#[derive(Clone)]
pub struct MssqlMasterDropdownRepository {
    pool: MssqlPool,
}

impl MssqlMasterDropdownRepository {

    pub fn new(pool: MssqlPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl MasterDropdownRepository for MssqlMasterDropdownRepository {
    async fn list_countries(
        &self,
        keyword: Option<&str>,
        status: Option<i32>,
    ) -> Result<Vec<MasterDropdownRow>, RepoError> {

        let status_param: Option<i32> = Some(status.unwrap_or(1));
        let rows = exec_sp(
            &self.pool,
            "EXEC dbo.SP_MASTER_COUNTRY_DROPDOWN_GET \
                @p_keyword = @P1, @p_master_country_status = @P2",
            &[&keyword as &dyn ToSql, &status_param as &dyn ToSql],
        )
        .await?;
        Ok(rows.iter().map(row_to_master_dropdown_row).collect())
    }

    async fn autocomplete_user_department(
        &self,
        keyword: Option<&str>,
        take: Option<i32>,
    ) -> Result<Vec<MasterDropdownRow>, RepoError> {

        let take_param: Option<i32> = Some(take.unwrap_or(20).clamp(1, 100));
        let rows = exec_sp(
            &self.pool,
            "EXEC dbo.SP_AUTOCOMPLETE_USER_DEPARTMENT_GET \
                    @p_keyword = @P1, @p_take = @P2",
            &[&keyword as &dyn ToSql, &take_param as &dyn ToSql],
        )
        .await?;
        Ok(rows.iter().map(row_to_master_dropdown_row).collect())
    }

    async fn autocomplete_master_positions(
        &self,
        user_department_team_guid: Option<&str>,
        keyword: Option<&str>,
        take: Option<i32>,
    ) -> Result<Vec<MasterPositionAutocompleteRow>, RepoError> {

        let take_param: Option<i32> = Some(take.unwrap_or(20).clamp(1, 100));
        let rows = exec_sp(
            &self.pool,
            "EXEC dbo.SP_AUTOCOMPLETE_MASTER_POSITION_GET \
                    @p_department_team_guid = @P1, @p_keyword = @P2, @p_take = @P3",
            &[
                &user_department_team_guid as &dyn ToSql,
                &keyword as &dyn ToSql,
                &take_param as &dyn ToSql,
            ],
        )
        .await?;
        Ok(rows
            .iter()
            .map(row_to_master_position_autocomplete)
            .collect())
    }

    async fn autocomplete_user_department_team(
        &self,
        user_department_guid: Option<&str>,
        keyword: Option<&str>,
        take: Option<i32>,
    ) -> Result<Vec<UserDepartmentTeamAutocompleteRow>, RepoError> {

        let take_param: Option<i32> = Some(take.unwrap_or(20).clamp(1, 100));
        let rows = exec_sp(
            &self.pool,
            "EXEC dbo.SP_AUTOCOMPLETE_USER_DEPARTMENT_TEAM_GET \
                @p_user_department_guid = @P1, @p_keyword = @P2, @p_take = @P3",
            &[
                &user_department_guid as &dyn ToSql,
                &keyword as &dyn ToSql,
                &take_param as &dyn ToSql,
            ],
        )
        .await?;
        Ok(rows
            .iter()
            .map(row_to_user_department_team_autocomplete)
            .collect())
    }
}

fn row_to_master_dropdown_row(row: &tiberius::Row) -> MasterDropdownRow {
    MasterDropdownRow {
        value: read_str(row, "value").unwrap_or("").to_string(),
        label: read_str(row, "label").unwrap_or("").to_string(),
    }
}

fn row_to_master_position_autocomplete(row: &tiberius::Row) -> MasterPositionAutocompleteRow {
    MasterPositionAutocompleteRow {
        value: read_str(row, "value").unwrap_or("").to_string(),
        label: read_str(row, "label").unwrap_or("").to_string(),
        code: read_str(row, "master_position_code")
            .unwrap_or("")
            .to_string(),
        description: read_str(row, "master_position_description")
            .unwrap_or("")
            .to_string(),
        level: read_i32(row, "master_position_level").unwrap_or(0),
        status: read_i32(row, "master_position_status").unwrap_or(0),
        user_department_team_guid: read_str(row, "user_department_team_guid")
            .unwrap_or("")
            .to_string(),
        user_department_team_code: read_str(row, "user_department_team_code")
            .unwrap_or("")
            .to_string(),
        user_department_team_name: read_str(row, "user_department_team_name")
            .unwrap_or("")
            .to_string(),
        user_department_guid: read_str(row, "user_department_guid")
            .unwrap_or("")
            .to_string(),
        user_department_code: read_str(row, "user_department_code")
            .unwrap_or("")
            .to_string(),
        user_department_name: read_str(row, "user_department_name")
            .unwrap_or("")
            .to_string(),
    }
}

fn row_to_user_department_team_autocomplete(
    row: &tiberius::Row,
) -> UserDepartmentTeamAutocompleteRow {
    UserDepartmentTeamAutocompleteRow {
        value: read_str(row, "value").unwrap_or("").to_string(),
        label: read_str(row, "label").unwrap_or("").to_string(),
        user_department_team_guid: read_str(row, "user_department_team_guid")
            .unwrap_or("")
            .to_string(),
        user_department_team_code: read_str(row, "user_department_team_code")
            .unwrap_or("")
            .to_string(),
        user_department_team_name: read_str(row, "user_department_team_name")
            .unwrap_or("")
            .to_string(),
        user_department_team_status: read_i32(row, "user_department_team_status").unwrap_or(0),
        user_department_guid: read_str(row, "user_department_guid")
            .unwrap_or("")
            .to_string(),
        user_department_code: read_str(row, "user_department_code")
            .unwrap_or("")
            .to_string(),
        user_department_name: read_str(row, "user_department_name")
            .unwrap_or("")
            .to_string(),
    }
}
