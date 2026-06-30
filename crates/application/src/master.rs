//! Master-data dropdown use cases (M20+).
//!
//! Shared reference-data lookups consumed by every client
//! (mobile / customer web / admin web). The service is a thin
//! pass-through to [`MasterDropdownRepository`] — there is no
//! cross-aggregate orchestration or business rule to enforce.
//!
//! ponytail: the read methods here add nothing over the trait.
//! They're kept as services so the handler / future tests have
//! one stable seam (per AGENTS.md § 6 — handlers depend on
//! services, services depend on ports). Ceiling: when a master
//! dataset needs caching (e.g. country list with a 24h TTL per
//! AGENTS.md §9), the cache layer slots here, between the
//! handler and the repository.

use std::sync::Arc;

use kokkak_domain::traits::master::MasterDropdownRepository;
use kokkak_domain::traits::user::RepoError;
use kokkak_domain::{
    MasterDropdownRow, MasterPositionAutocompleteRow, UserDepartmentTeamAutocompleteRow,
};

/// Master-data dropdown use case bundle (M20+).
pub struct MasterDropdownService {
    repo: Arc<dyn MasterDropdownRepository>,
}

impl MasterDropdownService {
    /// Construct the service with a [`MasterDropdownRepository`] port.
    pub fn new(repo: Arc<dyn MasterDropdownRepository>) -> Self {
        Self { repo }
    }

    /// Look up the country dropdown (label / value).
    ///
    /// - `keyword`: `None` or blank returns all matching rows.
    /// - `status`:  `None` → active-only (`1`); `Some(0/1/2)` to scope;
    ///   `Some(3)` (deleted) is hard-excluded by the SP.
    pub async fn list_countries(
        &self,
        keyword: Option<&str>,
        status: Option<i32>,
    ) -> Result<Vec<MasterDropdownRow>, RepoError> {
        self.repo.list_countries(keyword, status).await
    }

    /// Autocomplete lookup for the admin user-form's
    /// `master_position` picker.
    ///
    /// Thin pass-through to the repo; defaults (`take = 20`,
    /// active-only `status = 1`) live in the SP. See
    /// [`MasterDropdownRepository::autocomplete_master_positions`]
    /// for the filter semantics.
    pub async fn autocomplete_master_positions(
        &self,
        keyword: Option<&str>,
        take: Option<i32>,
    ) -> Result<Vec<MasterPositionAutocompleteRow>, RepoError> {
        self.repo.autocomplete_master_positions(keyword, take).await
    }

    /// Autocomplete lookup for the admin user-form's
    /// `user_department_team` picker.
    ///
    /// Thin pass-through to the repo; defaults (`take = 20`,
    /// `status = 1` active-only) live in the SP. See
    /// [`MasterDropdownRepository::autocomplete_user_department_team`]
    /// for the filter semantics.
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

    /// User-department autocomplete (label / value).
    ///
    /// Thin pass-through to the repo; defaults (`take = 20`, capped
    /// at `100`, hard-coded `status = 1` active-only) live in the SP.
    /// The infra adapter re-clamps `take` to `[1, 100]` so the trait
    /// contract is self-documenting.
    ///
    /// - `keyword`: `None` or blank returns top `take` rows.
    ///   `Some(text)` → SP applies prefix-LIKE on `name` + `code`.
    /// - `take`:    `None` → default (20); `Some(n <= 0)` → 20;
    ///   `Some(n > 100)` → 100.
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
    //! The service is a pass-through today; the cache layer (when it
    //! lands) will earn the test surface. For now we keep one
    //! mock-based test that exercises the `None` / `Some(...)`
    //! forwarding to lock the wire shape against accidental logic
    //! additions later.
    use super::*;
    use std::sync::Mutex;

    /// Mock repo records the `(keyword, status)` pair of the last
    /// call so the test asserts forwarding behaviour exactly.
    #[derive(Default)]
    struct MockMasterDropdownRepo {
        last_keyword: Mutex<Option<String>>,
        last_status: Mutex<Option<Option<i32>>>,
        // Autocomplete path uses a separate row buffer + a record of the
        // last `(user_department_guid, keyword, take)` triple.
        autocomplete_rows: Mutex<Vec<UserDepartmentTeamAutocompleteRow>>,
        last_user_department_guid: Mutex<Option<Option<String>>>,
        last_autocomplete_keyword: Mutex<Option<Option<String>>>,
        last_take: Mutex<Option<Option<i32>>>,
        // Master-position autocomplete uses its own row buffer + records
        // `(keyword, take)` so the test can verify forwarding in isolation.
        position_autocomplete_rows: Mutex<Vec<MasterPositionAutocompleteRow>>,
        last_position_keyword: Mutex<Option<Option<String>>>,
        last_position_take: Mutex<Option<Option<i32>>>,
        // User-department autocomplete dropdown uses the dropdown row buffer
        // + records `(keyword, take)` so the test can verify forwarding.
        user_department_rows: Mutex<Vec<MasterDropdownRow>>,
        last_user_department_keyword: Mutex<Option<Option<String>>>,
        last_user_department_take: Mutex<Option<Option<i32>>>,
        /// Pre-canned rows the service returns verbatim.
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
            keyword: Option<&str>,
            take: Option<i32>,
        ) -> Result<Vec<MasterPositionAutocompleteRow>, RepoError> {
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

        // Some keyword + Some status — both passed through.
        let rows = svc.list_countries(Some("thai"), Some(1)).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].label, "Thailand");

        // None / None — both passed through as None (the layer that
        // applies "active-only by default" lives in the infra
        // adapter, not the service, so the service stays a pure
        // pass-through).
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

        // Some keyword + Some take — both passed through unchanged.
        let rows = svc
            .autocomplete_master_positions(Some("tech"), Some(5))
            .await
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].label, "Senior Technician");
        assert_eq!(rows[0].code, "TECH_SR");

        // None / None — defaults live in the SP / infra adapter, not
        // the service.
        let _ = svc.autocomplete_master_positions(None, None).await.unwrap();
    }

    #[tokio::test]
    async fn autocomplete_user_department_team_forwards_filters_verbatim() {
        let mock = MockMasterDropdownRepo {
            autocomplete_rows: Mutex::new(vec![sample_autocomplete_row()]),
            ..Default::default()
        };
        let repo: Arc<dyn MasterDropdownRepository> = Arc::new(mock);
        let svc = MasterDropdownService::new(repo);

        // All three filters populated — every value passes through.
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

        // None / None / None — same wire shape (the defaults live in
        // the SP / infra adapter, not the service).
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

        // Some keyword + Some take — both passed through verbatim.
        let rows = svc
            .autocomplete_user_department(Some("plumb"), Some(10))
            .await
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].label, "Plumbing");

        // None / None — both passed through as None (SP / infra
        // adapter own the defaults: empty keyword + take=20).
        let _ = svc.autocomplete_user_department(None, None).await.unwrap();
    }
}
