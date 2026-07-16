use std::sync::Arc;

use kokkak_domain::traits::user::RepoError;
use kokkak_domain::{
    CategoryJobMainAutocompleteInput, CategoryJobMainAutocompleteRow, CategoryJobMainCreateInput,
    CategoryJobMainCreateResult, CategoryJobMainDeleteResult, CategoryJobMainDetailRow,
    CategoryJobMainListInput, CategoryJobMainPage, CategoryJobMainRepository,
    CategoryJobMainUpdateInput, CategoryJobMainUpdateResult,
};

pub struct CategoryJobMainService {
    repo: Arc<dyn CategoryJobMainRepository>,
}

impl CategoryJobMainService {
    pub fn new(repo: Arc<dyn CategoryJobMainRepository>) -> Self {
        Self { repo }
    }

    pub fn disabled() -> Self {
        struct DisabledRepo;
        #[async_trait::async_trait]
        impl CategoryJobMainRepository for DisabledRepo {
            async fn list(
                &self,
                _input: &CategoryJobMainListInput,
            ) -> Result<CategoryJobMainPage, RepoError> {
                Err(RepoError::Backend(
                    "CategoryJobMainService::disabled — repository not wired (set KOKKAK_DATABASE__SQLSERVER_URL)"
                        .into(),
                ))
            }
            async fn create(
                &self,
                _input: &CategoryJobMainCreateInput,
            ) -> Result<CategoryJobMainCreateResult, RepoError> {
                Err(RepoError::Backend(
                    "CategoryJobMainService::disabled — repository not wired".into(),
                ))
            }
            async fn update(
                &self,
                _input: &CategoryJobMainUpdateInput,
            ) -> Result<CategoryJobMainUpdateResult, RepoError> {
                Err(RepoError::Backend(
                    "CategoryJobMainService::disabled — repository not wired".into(),
                ))
            }
            async fn delete(
                &self,
                _category_guid: &str,
                _actor_user_guid: &str,
            ) -> Result<CategoryJobMainDeleteResult, RepoError> {
                Err(RepoError::Backend(
                    "CategoryJobMainService::disabled — repository not wired".into(),
                ))
            }
            async fn autocomplete(
                &self,
                _input: &CategoryJobMainAutocompleteInput,
            ) -> Result<Vec<CategoryJobMainAutocompleteRow>, RepoError> {
                Err(RepoError::Backend(
                    "CategoryJobMainService::disabled — repository not wired".into(),
                ))
            }
            async fn detail(
                &self,
                _category_guid: &str,
            ) -> Result<Option<CategoryJobMainDetailRow>, RepoError> {
                Err(RepoError::Backend(
                    "CategoryJobMainService::disabled — repository not wired".into(),
                ))
            }
        }
        let repo: Arc<dyn CategoryJobMainRepository> = Arc::new(DisabledRepo);
        Self { repo }
    }

    pub fn repo(&self) -> Arc<dyn CategoryJobMainRepository> {
        Arc::clone(&self.repo)
    }

    pub async fn list(
        &self,
        input: CategoryJobMainListInput,
    ) -> Result<CategoryJobMainPage, RepoError> {
        self.repo.list(&input).await
    }

    pub async fn create(
        &self,
        input: CategoryJobMainCreateInput,
    ) -> Result<CategoryJobMainCreateResult, RepoError> {
        self.repo.create(&input).await
    }

    pub async fn update(
        &self,
        input: CategoryJobMainUpdateInput,
    ) -> Result<CategoryJobMainUpdateResult, RepoError> {
        self.repo.update(&input).await
    }

    pub async fn delete(
        &self,
        category_guid: &str,
        actor_user_guid: &str,
    ) -> Result<CategoryJobMainDeleteResult, RepoError> {
        self.repo.delete(category_guid, actor_user_guid).await
    }

    pub async fn autocomplete(
        &self,
        input: CategoryJobMainAutocompleteInput,
    ) -> Result<Vec<CategoryJobMainAutocompleteRow>, RepoError> {
        self.repo.autocomplete(&input).await
    }

    pub async fn detail(
        &self,
        category_guid: &str,
    ) -> Result<Option<CategoryJobMainDetailRow>, RepoError> {
        self.repo.detail(category_guid).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use kokkak_domain::CategoryJobMainRow;
    use std::sync::Mutex;

    #[derive(Default)]
    struct MockRepo {
        rows: Mutex<Vec<CategoryJobMainRow>>,
        last_delete: Mutex<Option<(String, String)>>,
        autocomplete_rows: Mutex<Vec<CategoryJobMainAutocompleteRow>>,
        last_autocomplete: Mutex<Option<CategoryJobMainAutocompleteInput>>,
    }

    #[async_trait::async_trait]
    impl CategoryJobMainRepository for MockRepo {
        async fn list(
            &self,
            _input: &CategoryJobMainListInput,
        ) -> Result<CategoryJobMainPage, RepoError> {
            let items = self.rows.lock().unwrap().clone();
            let total = items.len() as i64;
            Ok(CategoryJobMainPage {
                items,
                total_count: total,
                page: 1,
                page_size: total as u32,
                total_page: 1,
                active: 0,
                close: 0,
            })
        }
        async fn create(
            &self,
            _input: &CategoryJobMainCreateInput,
        ) -> Result<CategoryJobMainCreateResult, RepoError> {
            Ok(CategoryJobMainCreateResult {
                success: true,
                code: "SUCCESS".into(),
                message: "ok".into(),
                category_job_main_guid: Some(uuid::Uuid::new_v4().to_string()),
            })
        }
        async fn update(
            &self,
            input: &CategoryJobMainUpdateInput,
        ) -> Result<CategoryJobMainUpdateResult, RepoError> {
            Ok(CategoryJobMainUpdateResult {
                success: true,
                code: "SUCCESS".into(),
                message: "ok".into(),
                category_job_main_guid: Some(input.category_job_main_guid.clone()),
            })
        }
        async fn delete(
            &self,
            category_guid: &str,
            actor_user_guid: &str,
        ) -> Result<CategoryJobMainDeleteResult, RepoError> {
            *self.last_delete.lock().unwrap() =
                Some((category_guid.to_string(), actor_user_guid.to_string()));
            Ok(CategoryJobMainDeleteResult {
                success: true,
                code: "SUCCESS".into(),
                message: "ok".into(),
                category_job_main_guid: category_guid.to_string(),
            })
        }
        async fn autocomplete(
            &self,
            input: &CategoryJobMainAutocompleteInput,
        ) -> Result<Vec<CategoryJobMainAutocompleteRow>, RepoError> {
            *self.last_autocomplete.lock().unwrap() = Some(input.clone());
            Ok(self.autocomplete_rows.lock().unwrap().clone())
        }
        async fn detail(
            &self,
            _category_guid: &str,
        ) -> Result<Option<CategoryJobMainDetailRow>, RepoError> {
            Ok(None)
        }
    }

    fn make_row(guid: &str, name: &str, status: i32) -> CategoryJobMainRow {
        CategoryJobMainRow {
            category_job_main_guid: guid.into(),
            category_job_main_name: name.into(),
            category_job_main_locale: "th".into(),
            category_job_main_icon_style: "solid".into(),
            category_job_main_icon_line: "wrench".into(),
            category_job_main_img_path: format!("category-job-mains/{guid}/icon/x.webp"),
            category_job_main_img_url: None,
            category_job_main_status: status,
            category_job_main_priority: 0,
            has_sub_service: false,
            category_job_main_create_at: Some(Utc::now()),
            category_job_main_create_by: "actor".into(),
            category_job_main_update_at: Some(Utc::now()),
            category_job_main_update_by: "actor".into(),
        }
    }

    #[tokio::test]
    async fn list_forwards_filters_and_returns_repo_rows() {
        let repo = MockRepo {
            rows: Mutex::new(vec![make_row("g1", "Home Repair", 1)]),
            ..Default::default()
        };
        let repo: Arc<dyn CategoryJobMainRepository> = Arc::new(repo);
        let svc = CategoryJobMainService::new(repo);

        let page = svc
            .list(CategoryJobMainListInput {
                keyword: Some("home".into()),
                status: None,
                locale: Some("th".into()),
                page: 1,
                page_size: 20,
            })
            .await
            .unwrap();
        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0].category_job_main_name, "Home Repair");
        assert_eq!(page.total_count, 1);
    }

    #[tokio::test]
    async fn create_update_delete_roundtrip() {
        let last_delete = Arc::new(Mutex::new(Option::<(String, String)>::None));
        let rows = Arc::new(Mutex::new(Vec::<CategoryJobMainRow>::new()));

        let last_delete_inner = last_delete.clone();
        let rows_inner = rows.clone();
        let repo = MockRepoHarness {
            last_delete: last_delete_inner,
            rows: rows_inner,
        };
        let repo_arc: Arc<dyn CategoryJobMainRepository> = Arc::new(repo);
        let svc = CategoryJobMainService::new(repo_arc);

        let create_res = svc
            .create(CategoryJobMainCreateInput {
                category_job_main_name_la: Some("Plumbing".into()),
                category_job_main_name_en: Some("Plumbing".into()),
                category_job_main_name_th: None,
                category_job_main_name_zh: None,
                category_job_main_icon_style: Some("solid".into()),
                category_job_main_icon_line: Some("pipe".into()),
                category_job_main_img_path: Some("category-job-mains/x/icon/y.webp".into()),
                category_job_main_priority: Some(10),
                create_by: "admin-1".into(),
            })
            .await
            .unwrap();
        assert!(create_res.success);
        assert!(create_res.category_job_main_guid.is_some());

        let update_res = svc
            .update(CategoryJobMainUpdateInput {
                category_job_main_guid: create_res.category_job_main_guid.clone().unwrap(),
                category_job_main_name_la: Some("Plumbing v2".into()),
                category_job_main_name_en: Some("Plumbing v2 EN".into()),
                category_job_main_name_th: None,
                category_job_main_name_zh: None,
                category_job_main_icon_style: Some("solid".into()),
                category_job_main_icon_line: Some("pipe".into()),
                category_job_main_img_path: None,
                category_job_main_status: 1,
                category_job_main_priority: 20,
                update_by: "admin-1".into(),
            })
            .await
            .unwrap();
        assert!(update_res.success);

        let del_res = svc
            .delete(
                &create_res.category_job_main_guid.clone().unwrap(),
                "admin-1",
            )
            .await
            .unwrap();
        assert!(del_res.success);
        let recorded = last_delete.lock().unwrap().clone();
        assert_eq!(recorded.unwrap().1, "admin-1");

        drop(rows.lock().unwrap());
    }

    struct MockRepoHarness {
        rows: Arc<Mutex<Vec<CategoryJobMainRow>>>,
        last_delete: Arc<Mutex<Option<(String, String)>>>,
    }

    #[async_trait::async_trait]
    impl CategoryJobMainRepository for MockRepoHarness {
        async fn list(
            &self,
            _input: &CategoryJobMainListInput,
        ) -> Result<CategoryJobMainPage, RepoError> {
            let items = self.rows.lock().unwrap().clone();
            let total = items.len() as i64;
            Ok(CategoryJobMainPage {
                items,
                total_count: total,
                page: 1,
                page_size: total as u32,
                total_page: 1,
                active: 0,
                close: 0,
            })
        }
        async fn create(
            &self,
            _input: &CategoryJobMainCreateInput,
        ) -> Result<CategoryJobMainCreateResult, RepoError> {
            Ok(CategoryJobMainCreateResult {
                success: true,
                code: "SUCCESS".into(),
                message: "ok".into(),
                category_job_main_guid: Some(uuid::Uuid::new_v4().to_string()),
            })
        }
        async fn update(
            &self,
            input: &CategoryJobMainUpdateInput,
        ) -> Result<CategoryJobMainUpdateResult, RepoError> {
            Ok(CategoryJobMainUpdateResult {
                success: true,
                code: "SUCCESS".into(),
                message: "ok".into(),
                category_job_main_guid: Some(input.category_job_main_guid.clone()),
            })
        }
        async fn delete(
            &self,
            category_guid: &str,
            actor_user_guid: &str,
        ) -> Result<CategoryJobMainDeleteResult, RepoError> {
            *self.last_delete.lock().unwrap() =
                Some((category_guid.to_string(), actor_user_guid.to_string()));
            Ok(CategoryJobMainDeleteResult {
                success: true,
                code: "SUCCESS".into(),
                message: "ok".into(),
                category_job_main_guid: category_guid.to_string(),
            })
        }
        async fn autocomplete(
            &self,
            _input: &CategoryJobMainAutocompleteInput,
        ) -> Result<Vec<CategoryJobMainAutocompleteRow>, RepoError> {
            Ok(Vec::new())
        }
        async fn detail(
            &self,
            _category_guid: &str,
        ) -> Result<Option<CategoryJobMainDetailRow>, RepoError> {
            Ok(None)
        }
    }

    #[tokio::test]
    async fn autocomplete_forwards_filters_and_returns_repo_rows() {
        let repo = MockRepo {
            autocomplete_rows: Mutex::new(vec![
                CategoryJobMainAutocompleteRow {
                    category_job_main_guid: "11111111-1111-1111-1111-111111111111".into(),
                    category_job_main_name: "Plumbing".into(),
                },
                CategoryJobMainAutocompleteRow {
                    category_job_main_guid: "22222222-2222-2222-2222-222222222222".into(),
                    category_job_main_name: "Electrical".into(),
                },
            ]),
            ..Default::default()
        };
        let repo: Arc<dyn CategoryJobMainRepository> = Arc::new(repo);
        let svc = CategoryJobMainService::new(repo);

        let rows = svc
            .autocomplete(CategoryJobMainAutocompleteInput {
                keyword: Some("plumb".into()),
                status: Some(1),
                locale: Some("la".into()),
                take: Some(5),
            })
            .await
            .unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].category_job_main_name, "Plumbing");
        assert_eq!(
            rows[1].category_job_main_guid,
            "22222222-2222-2222-2222-222222222222"
        );
    }

    #[tokio::test]
    async fn autocomplete_passes_through_all_none_inputs() {
        let repo = MockRepo::default();
        let repo: Arc<dyn CategoryJobMainRepository> = Arc::new(repo);
        let svc = CategoryJobMainService::new(repo);

        let rows = svc
            .autocomplete(CategoryJobMainAutocompleteInput {
                keyword: None,
                status: None,
                locale: None,
                take: None,
            })
            .await
            .unwrap();
        assert!(rows.is_empty());
    }
}
