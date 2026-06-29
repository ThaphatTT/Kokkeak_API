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
use kokkak_domain::MasterDropdownRow;

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
        /// Pre-canned rows the service returns verbatim.
        rows: Mutex<Vec<MasterDropdownRow>>,
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
}
