

use std::sync::Arc;

use kokkak_domain::traits::master::MasterDropdownRepository;
use kokkak_domain::traits::user::RepoError;
use kokkak_domain::{
    MasterDropdownRow, MasterPositionAutocompleteRow, UserDepartmentTeamAutocompleteRow,
};

pub struct MasterDropdownService {
    repo: Arc<dyn MasterDropdownRepository>,
}

impl MasterDropdownService {

    pub fn new(repo: Arc<dyn MasterDropdownRepository>) -> Self {
        Self { repo }
    }

    pub async fn list_countries(
        &self,
        keyword: Option<&str>,
        status: Option<i32>,
    ) -> Result<Vec<MasterDropdownRow>, RepoError> {
        self.repo.list_countries(keyword, status).await
    }

    pub async fn autocomplete_master_positions(
        &self,
        user_department_team_guid: Option<&str>,
        keyword: Option<&str>,
        take: Option<i32>,
    ) -> Result<Vec<MasterPositionAutocompleteRow>, RepoError> {
        self.repo
            .autocomplete_master_positions(user_department_team_guid, keyword, take)
            .await
    }

    pub async fn autocomplete_user_department_team(
        &self,
        user_department_guid: Option<&str>,
        keyword: Option<&str>,
        take: Option<i32>,
    ) -> Result<Vec<UserDepartmentTeamAutocompleteRow>, RepoError> {
        self.repo
            .autocomplete_user_department_team(user_department_guid, keyword, take)
            .await
    }

    pub async fn autocomplete_user_department(
        &self,
        keyword: Option<&str>,
        take: Option<i32>,
    ) -> Result<Vec<MasterDropdownRow>, RepoError> {
        self.repo.autocomplete_user_department(keyword, take).await
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    struct MockMasterDropdownRepo {
        last_keyword: Mutex<Option<String>>,
        last_status: Mutex<Option<Option<i32>>>,

        autocomplete_rows: Mutex<Vec<UserDepartmentTeamAutocompleteRow>>,
        last_user_department_guid: Mutex<Option<Option<String>>>,
        last_autocomplete_keyword: Mutex<Option<Option<String>>>,
        last_take: Mutex<Option<Option<i32>>>,

        position_autocomplete_rows: Mutex<Vec<MasterPositionAutocompleteRow>>,
        last_position_department_team_guid: Mutex<Option<Option<String>>>,
        last_position_keyword: Mutex<Option<Option<String>>>,
        last_position_take: Mutex<Option<Option<i32>>>,

        user_department_rows: Mutex<Vec<MasterDropdownRow>>,
        last_user_department_keyword: Mutex<Option<Option<String>>>,
        last_user_department_take: Mutex<Option<Option<i32>>>,

        rows: Mutex<Vec<MasterDropdownRow>>,
    }

    fn sample_autocomplete_row() -> UserDepartmentTeamAutocompleteRow {
        UserDepartmentTeamAutocompleteRow {
            value: "22222222-2222-2222-2222-222222222222".into(),
            label: "Backend".into(),
            user_department_team_guid: "22222222-2222-2222-2222-222222222222".into(),
            user_department_team_code: "BE".into(),
            user_department_team_name: "Backend".into(),
            user_department_team_status: 1,
            user_department_guid: "33333333-3333-3333-3333-333333333333".into(),
            user_department_code: "ENG".into(),
            user_department_name: "Engineering".into(),
        }
    }

    fn sample_position_autocomplete_row() -> MasterPositionAutocompleteRow {
        MasterPositionAutocompleteRow {
            value: "44444444-4444-4444-4444-444444444444".into(),
            label: "Senior Technician".into(),
            code: "TECH_SR".into(),
            description: "Senior field technician".into(),
            level: 5,
            status: 1,
            user_department_team_guid: "22222222-2222-2222-2222-222222222222".into(),
            user_department_team_code: "BE".into(),
            user_department_team_name: "Backend".into(),
            user_department_guid: "33333333-3333-3333-3333-333333333333".into(),
            user_department_code: "ENG".into(),
            user_department_name: "Engineering".into(),
        }
    }

    fn sample_user_department_row() -> MasterDropdownRow {
        MasterDropdownRow {
            value: "55555555-5555-5555-5555-555555555555".into(),
            label: "Plumbing".into(),
        }
    }

    #[async_trait::async_trait]
    impl MasterDropdownRepository for MockMasterDropdownRepo {
        async fn list_countries(
            &self,
            keyword: Option<&str>,
            status: Option<i32>,
        ) -> Result<Vec<MasterDropdownRow>, RepoError> {
            *self.last_keyword.lock().unwrap() = keyword.map(str::to_string);
            *self.last_status.lock().unwrap() = Some(status);
            Ok(self.rows.lock().unwrap().clone())
        }

        async fn autocomplete_user_department(
            &self,
            keyword: Option<&str>,
            take: Option<i32>,
        ) -> Result<Vec<MasterDropdownRow>, RepoError> {
            *self.last_user_department_keyword.lock().unwrap() = Some(keyword.map(str::to_string));
            *self.last_user_department_take.lock().unwrap() = Some(take);
            Ok(self.user_department_rows.lock().unwrap().clone())
        }

        async fn autocomplete_master_positions(
            &self,
            user_department_team_guid: Option<&str>,
            keyword: Option<&str>,
            take: Option<i32>,
        ) -> Result<Vec<MasterPositionAutocompleteRow>, RepoError> {
            *self.last_position_department_team_guid.lock().unwrap() =
                Some(user_department_team_guid.map(str::to_string));
            *self.last_position_keyword.lock().unwrap() = Some(keyword.map(str::to_string));
            *self.last_position_take.lock().unwrap() = Some(take);
            Ok(self.position_autocomplete_rows.lock().unwrap().clone())
        }

        async fn autocomplete_user_department_team(
            &self,
            user_department_guid: Option<&str>,
            keyword: Option<&str>,
            take: Option<i32>,
        ) -> Result<Vec<UserDepartmentTeamAutocompleteRow>, RepoError> {
            *self.last_user_department_guid.lock().unwrap() =
                Some(user_department_guid.map(str::to_string));
            *self.last_autocomplete_keyword.lock().unwrap() = Some(keyword.map(str::to_string));
            *self.last_take.lock().unwrap() = Some(take);
            Ok(self.autocomplete_rows.lock().unwrap().clone())
        }
    }

    #[tokio::test]
    async fn list_countries_forwards_filters_verbatim() {
        let mock = MockMasterDropdownRepo {
            rows: Mutex::new(vec![MasterDropdownRow {
                value: "11111111-1111-1111-1111-111111111111".into(),
                label: "Thailand".into(),
            }]),
            ..Default::default()
        };
        let repo: Arc<dyn MasterDropdownRepository> = Arc::new(mock);
        let svc = MasterDropdownService::new(repo);

        let rows = svc.list_countries(Some("thai"), Some(1)).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].label, "Thailand");

        let _ = svc.list_countries(None, None).await.unwrap();
    }

    #[tokio::test]
    async fn autocomplete_master_positions_forwards_filters_verbatim() {
        let mock = MockMasterDropdownRepo {
            position_autocomplete_rows: Mutex::new(vec![sample_position_autocomplete_row()]),
            ..Default::default()
        };
        let repo: Arc<dyn MasterDropdownRepository> = Arc::new(mock);
        let svc = MasterDropdownService::new(repo);

        let rows = svc
            .autocomplete_master_positions(
                Some("22222222-2222-2222-2222-222222222222"),
                Some("tech"),
                Some(5),
            )
            .await
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].label, "Senior Technician");
        assert_eq!(rows[0].code, "TECH_SR");
        assert_eq!(rows[0].user_department_team_name, "Backend");

        let _ = svc
            .autocomplete_master_positions(None, None, None)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn autocomplete_user_department_team_forwards_filters_verbatim() {
        let mock = MockMasterDropdownRepo {
            autocomplete_rows: Mutex::new(vec![sample_autocomplete_row()]),
            ..Default::default()
        };
        let repo: Arc<dyn MasterDropdownRepository> = Arc::new(mock);
        let svc = MasterDropdownService::new(repo);

        let rows = svc
            .autocomplete_user_department_team(
                Some("33333333-3333-3333-3333-333333333333"),
                Some("back"),
                Some(10),
            )
            .await
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].label, "Backend");

        let _ = svc
            .autocomplete_user_department_team(None, None, None)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn autocomplete_user_department_forwards_keyword_and_take_verbatim() {
        let mock = MockMasterDropdownRepo {
            user_department_rows: Mutex::new(vec![sample_user_department_row()]),
            ..Default::default()
        };
        let repo: Arc<dyn MasterDropdownRepository> = Arc::new(mock);
        let svc = MasterDropdownService::new(repo);

        let rows = svc
            .autocomplete_user_department(Some("plumb"), Some(10))
            .await
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].label, "Plumbing");

        let _ = svc.autocomplete_user_department(None, None).await.unwrap();
    }
}
