use std::sync::Arc;

use kokkak_domain::traits::user::RepoError;
use kokkak_domain::{
    CategoryJobServiceMainCreateInput, CategoryJobServiceMainCreateResult,
    CategoryJobServiceMainDeleteResult, CategoryJobServiceMainRepository,
    CategoryJobServiceMainRow, CategoryJobServiceMainUpdateInput,
    CategoryJobServiceMainUpdateResult,
};

pub struct CategoryJobServiceMainService {
    repo: Arc<dyn CategoryJobServiceMainRepository>,
}

impl CategoryJobServiceMainService {
    pub fn new(repo: Arc<dyn CategoryJobServiceMainRepository>) -> Self {
        Self { repo }
    }

    pub fn disabled() -> Self {
        struct DisabledRepo;
        #[async_trait::async_trait]
        impl CategoryJobServiceMainRepository for DisabledRepo {
            async fn list(
                &self,
                _category_job_main_guid: &str,
                _keyword: Option<&str>,
                _include_inactive: bool,
            ) -> Result<Vec<CategoryJobServiceMainRow>, RepoError> {
                Err(RepoError::Backend(
                    "CategoryJobServiceMainService::disabled — repository not wired (set KOKKAK_DATABASE__SQLSERVER_URL)"
                        .into(),
                ))
            }
            async fn create(
                &self,
                _input: &CategoryJobServiceMainCreateInput,
            ) -> Result<CategoryJobServiceMainCreateResult, RepoError> {
                Err(RepoError::Backend(
                    "CategoryJobServiceMainService::disabled — repository not wired".into(),
                ))
            }
            async fn update(
                &self,
                _input: &CategoryJobServiceMainUpdateInput,
            ) -> Result<CategoryJobServiceMainUpdateResult, RepoError> {
                Err(RepoError::Backend(
                    "CategoryJobServiceMainService::disabled — repository not wired".into(),
                ))
            }
            async fn delete(
                &self,
                _service_guid: &str,
                _actor_user_guid: &str,
            ) -> Result<CategoryJobServiceMainDeleteResult, RepoError> {
                Err(RepoError::Backend(
                    "CategoryJobServiceMainService::disabled — repository not wired".into(),
                ))
            }
        }
        let repo: Arc<dyn CategoryJobServiceMainRepository> = Arc::new(DisabledRepo);
        Self { repo }
    }

    pub fn repo(&self) -> Arc<dyn CategoryJobServiceMainRepository> {
        Arc::clone(&self.repo)
    }

    pub async fn list(
        &self,
        category_job_main_guid: &str,
        keyword: Option<&str>,
        include_inactive: bool,
    ) -> Result<Vec<CategoryJobServiceMainRow>, RepoError> {
        self.repo
            .list(category_job_main_guid, keyword, include_inactive)
            .await
    }

    pub async fn create(
        &self,
        input: CategoryJobServiceMainCreateInput,
    ) -> Result<CategoryJobServiceMainCreateResult, RepoError> {
        self.repo.create(&input).await
    }

    pub async fn update(
        &self,
        input: CategoryJobServiceMainUpdateInput,
    ) -> Result<CategoryJobServiceMainUpdateResult, RepoError> {
        self.repo.update(&input).await
    }

    pub async fn delete(
        &self,
        service_guid: &str,
        actor_user_guid: &str,
    ) -> Result<CategoryJobServiceMainDeleteResult, RepoError> {
        self.repo.delete(service_guid, actor_user_guid).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use std::sync::Mutex;

    #[derive(Default)]
    struct MockRepo {
        rows: Mutex<Vec<CategoryJobServiceMainRow>>,
        last_delete: Mutex<Option<(String, String)>>,
    }

    #[async_trait::async_trait]
    impl CategoryJobServiceMainRepository for MockRepo {
        async fn list(
            &self,
            _category_job_main_guid: &str,
            _keyword: Option<&str>,
            _include_inactive: bool,
        ) -> Result<Vec<CategoryJobServiceMainRow>, RepoError> {
            Ok(self.rows.lock().unwrap().clone())
        }
        async fn create(
            &self,
            _input: &CategoryJobServiceMainCreateInput,
        ) -> Result<CategoryJobServiceMainCreateResult, RepoError> {
            Ok(CategoryJobServiceMainCreateResult {
                success: true,
                code: "SUCCESS".into(),
                message: "ok".into(),
                category_job_service_guid: Some(uuid::Uuid::new_v4().to_string()),
            })
        }
        async fn update(
            &self,
            input: &CategoryJobServiceMainUpdateInput,
        ) -> Result<CategoryJobServiceMainUpdateResult, RepoError> {
            Ok(CategoryJobServiceMainUpdateResult {
                success: true,
                code: "SUCCESS".into(),
                message: "ok".into(),
                category_job_service_guid: Some(input.category_job_service_guid.clone()),
            })
        }
        async fn delete(
            &self,
            service_guid: &str,
            actor_user_guid: &str,
        ) -> Result<CategoryJobServiceMainDeleteResult, RepoError> {
            *self.last_delete.lock().unwrap() =
                Some((service_guid.to_string(), actor_user_guid.to_string()));
            Ok(CategoryJobServiceMainDeleteResult {
                success: true,
                code: "SUCCESS".into(),
                message: "ok".into(),
                category_job_service_guid: service_guid.to_string(),
            })
        }
    }

    fn make_row(service_guid: &str, main_guid: &str, name: &str) -> CategoryJobServiceMainRow {
        CategoryJobServiceMainRow {
            category_job_service_guid: service_guid.into(),
            category_job_service_category_main_guid: main_guid.into(),
            category_job_main_name: "Home Repair".into(),
            category_job_service_name: name.into(),
            category_job_service_icon_style: "solid".into(),
            category_job_service_icon_line: "snowflake".into(),
            category_job_service_img_path: format!(
                "category-job-services/{service_guid}/icon/x.webp"
            ),
            category_job_service_img_url: None,
            category_job_service_status: 1,
            has_sub_service: false,
            category_job_service_create_at: Some(Utc::now()),
            category_job_service_create_by: "admin".into(),
            category_job_service_update_at: Some(Utc::now()),
            category_job_service_update_by: "admin".into(),
        }
    }

    #[tokio::test]
    async fn list_forwards_filters() {
        let repo = MockRepo {
            rows: Mutex::new(vec![make_row("s1", "m1", "Air Con")]),
            ..Default::default()
        };
        let repo: Arc<dyn CategoryJobServiceMainRepository> = Arc::new(repo);
        let svc = CategoryJobServiceMainService::new(repo);

        let rows = svc.list("m1", Some("air"), false).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].category_job_service_name, "Air Con");
    }

    #[tokio::test]
    async fn create_update_delete_roundtrip() {
        let repo = MockRepo::default();
        let repo_arc: Arc<dyn CategoryJobServiceMainRepository> = Arc::new(repo);
        let svc = CategoryJobServiceMainService::new(repo_arc);

        let create_res = svc
            .create(CategoryJobServiceMainCreateInput {
                category_job_main_guid: "m-1".into(),
                category_job_service_name: "Plumbing".into(),
                category_job_service_icon_style: Some("solid".into()),
                category_job_service_icon_line: Some("pipe".into()),
                category_job_service_img_path: Some("category-job-services/x/icon/y.webp".into()),
                create_by: "admin-1".into(),
            })
            .await
            .unwrap();
        assert!(create_res.success);
        assert!(create_res.category_job_service_guid.is_some());

        let created_guid = create_res.category_job_service_guid.clone().unwrap();

        let update_res = svc
            .update(CategoryJobServiceMainUpdateInput {
                category_job_service_guid: created_guid.clone(),
                category_job_main_guid: "m-1".into(),
                category_job_service_name: "Plumbing v2".into(),
                category_job_service_icon_style: Some("solid".into()),
                category_job_service_icon_line: Some("pipe".into()),
                category_job_service_img_path: None,
                category_job_service_status: 1,
                update_by: "admin-1".into(),
            })
            .await
            .unwrap();
        assert!(update_res.success);

        let del_res = svc.delete(&created_guid, "admin-1").await.unwrap();
        assert!(del_res.success);
        assert_eq!(del_res.category_job_service_guid, created_guid);
    }
}
