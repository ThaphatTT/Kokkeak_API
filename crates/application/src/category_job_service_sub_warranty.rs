use std::sync::Arc;

use kokkak_domain::traits::user::RepoError;
use kokkak_domain::{
    CategoryJobServiceSubWarrantyCreateInput, CategoryJobServiceSubWarrantyCreateResult,
    CategoryJobServiceSubWarrantyDeleteInput, CategoryJobServiceSubWarrantyDeleteResult,
    CategoryJobServiceSubWarrantyListInput, CategoryJobServiceSubWarrantyPage,
    CategoryJobServiceSubWarrantyRepository, CategoryJobServiceSubWarrantyUpdateInput,
    CategoryJobServiceSubWarrantyUpdateResult,
};

pub struct CategoryJobServiceSubWarrantyService {
    repo: Arc<dyn CategoryJobServiceSubWarrantyRepository>,
}

impl CategoryJobServiceSubWarrantyService {
    pub fn new(repo: Arc<dyn CategoryJobServiceSubWarrantyRepository>) -> Self {
        Self { repo }
    }

    pub fn disabled() -> Self {
        struct DisabledRepo;
        #[async_trait::async_trait]
        impl CategoryJobServiceSubWarrantyRepository for DisabledRepo {
            async fn list(
                &self,
                _input: &CategoryJobServiceSubWarrantyListInput,
            ) -> Result<CategoryJobServiceSubWarrantyPage, RepoError> {
                Err(RepoError::Backend(
                    "CategoryJobServiceSubWarrantyService::disabled — repository not wired (set KOKKAK_DATABASE__SQLSERVER_URL)"
                        .into(),
                ))
            }

            async fn create(
                &self,
                _input: &CategoryJobServiceSubWarrantyCreateInput,
            ) -> Result<CategoryJobServiceSubWarrantyCreateResult, RepoError> {
                Err(RepoError::Backend(
                    "CategoryJobServiceSubWarrantyService::disabled — repository not wired (set KOKKAK_DATABASE__SQLSERVER_URL)"
                        .into(),
                ))
            }

            async fn update(
                &self,
                _input: &CategoryJobServiceSubWarrantyUpdateInput,
            ) -> Result<CategoryJobServiceSubWarrantyUpdateResult, RepoError> {
                Err(RepoError::Backend(
                    "CategoryJobServiceSubWarrantyService::disabled — repository not wired (set KOKKAK_DATABASE__SQLSERVER_URL)"
                        .into(),
                ))
            }

            async fn delete(
                &self,
                _input: &CategoryJobServiceSubWarrantyDeleteInput,
            ) -> Result<CategoryJobServiceSubWarrantyDeleteResult, RepoError> {
                Err(RepoError::Backend(
                    "CategoryJobServiceSubWarrantyService::disabled — repository not wired (set KOKKAK_DATABASE__SQLSERVER_URL)"
                        .into(),
                ))
            }
        }
        let repo: Arc<dyn CategoryJobServiceSubWarrantyRepository> = Arc::new(DisabledRepo);
        Self { repo }
    }

    pub async fn list(
        &self,
        input: CategoryJobServiceSubWarrantyListInput,
    ) -> Result<CategoryJobServiceSubWarrantyPage, RepoError> {
        self.repo.list(&input).await
    }

    pub async fn create(
        &self,
        input: CategoryJobServiceSubWarrantyCreateInput,
    ) -> Result<CategoryJobServiceSubWarrantyCreateResult, RepoError> {
        self.repo.create(&input).await
    }

    pub async fn update(
        &self,
        input: CategoryJobServiceSubWarrantyUpdateInput,
    ) -> Result<CategoryJobServiceSubWarrantyUpdateResult, RepoError> {
        self.repo.update(&input).await
    }

    pub async fn delete(
        &self,
        input: CategoryJobServiceSubWarrantyDeleteInput,
    ) -> Result<CategoryJobServiceSubWarrantyDeleteResult, RepoError> {
        self.repo.delete(&input).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use kokkak_domain::CategoryJobServiceSubWarrantyDetailRow;
    use std::sync::Mutex;

    #[derive(Default)]
    struct MockRepo {
        items: Mutex<Vec<CategoryJobServiceSubWarrantyDetailRow>>,
        last_input: Mutex<Option<CategoryJobServiceSubWarrantyListInput>>,
    }

    #[async_trait::async_trait]
    #[async_trait]
    impl CategoryJobServiceSubWarrantyRepository for MockRepo {
        async fn list(
            &self,
            input: &CategoryJobServiceSubWarrantyListInput,
        ) -> Result<CategoryJobServiceSubWarrantyPage, RepoError> {
            *self.last_input.lock().unwrap() = Some(input.clone());
            let items = self.items.lock().unwrap().clone();
            let count = items.len() as i64;
            Ok(CategoryJobServiceSubWarrantyPage {
                items,
                total_count: count,
                page: 1,
                page_size: 20,
                total_page: 1,
            })
        }

        async fn create(
            &self,
            _input: &CategoryJobServiceSubWarrantyCreateInput,
        ) -> Result<CategoryJobServiceSubWarrantyCreateResult, RepoError> {
            Ok(CategoryJobServiceSubWarrantyCreateResult {
                success: true,
                code: "INSERT_SUCCESS".into(),
                message: "ok".into(),
                category_job_service_sub_warranty_guid: Some("mock-guid".into()),
            })
        }
    }

    fn make_warranty(guid: &str, status: i32) -> CategoryJobServiceSubWarrantyDetailRow {
        CategoryJobServiceSubWarrantyDetailRow {
            category_job_service_sub_warranty_guid: guid.into(),
            category_job_service_sub_warranty_description: "desc".into(),
            category_job_service_sub_warranty_warranty_amount_day: 30,
            category_job_service_sub_warranty_status: status,
            category_job_service_sub_warranty_icon: "icon.webp".into(),
            category_job_service_sub_warranty_create_at: Some(Utc::now()),
            category_job_service_sub_warranty_create_by: "admin".into(),
            category_job_service_sub_warranty_update_at: Some(Utc::now()),
            category_job_service_sub_warranty_update_by: "admin".into(),
        }
    }

    #[tokio::test]
    async fn list_forwards_input_and_returns_repo_rows() {
        let repo = MockRepo {
            items: Mutex::new(vec![make_warranty("g1", 1)]),
            ..Default::default()
        };
        let repo: Arc<dyn CategoryJobServiceSubWarrantyRepository> = Arc::new(repo);
        let svc = CategoryJobServiceSubWarrantyService::new(repo);

        let page = svc
            .list(CategoryJobServiceSubWarrantyListInput {
                category_job_service_sub_warranty_guid: Some("g1".into()),
                keyword: None,
                status: Some(1),
                locale: Some("la".into()),
                page: Some(1),
                page_size: Some(20),
            })
            .await
            .unwrap();

        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0].category_job_service_sub_warranty_guid, "g1");
        assert_eq!(page.total_count, 1);
    }

    #[tokio::test]
    async fn list_with_empty_input_yields_empty_page() {
        let repo = MockRepo::default();
        let repo: Arc<dyn CategoryJobServiceSubWarrantyRepository> = Arc::new(repo);
        let svc = CategoryJobServiceSubWarrantyService::new(repo);

        let page = svc
            .list(CategoryJobServiceSubWarrantyListInput::default())
            .await
            .unwrap();

        assert!(page.items.is_empty());
        assert_eq!(page.total_count, 0);
    }
}
