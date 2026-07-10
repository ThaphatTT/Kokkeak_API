use std::sync::Arc;

use kokkak_domain::traits::user::RepoError;
use kokkak_domain::{
    CategoryJobServiceSubFeeAutocompleteInput, CategoryJobServiceSubFeeAutocompleteRow,
    CategoryJobServiceSubFeeCreateInput, CategoryJobServiceSubFeeCreateResult,
    CategoryJobServiceSubFeeDeleteInput, CategoryJobServiceSubFeeDeleteResult,
    CategoryJobServiceSubFeeDetailRow, CategoryJobServiceSubFeeListInput,
    CategoryJobServiceSubFeePage, CategoryJobServiceSubFeeRepository,
    CategoryJobServiceSubFeeUpdateInput, CategoryJobServiceSubFeeUpdateResult,
};

pub struct CategoryJobServiceSubFeeService {
    repo: Arc<dyn CategoryJobServiceSubFeeRepository>,
}

impl CategoryJobServiceSubFeeService {
    pub fn new(repo: Arc<dyn CategoryJobServiceSubFeeRepository>) -> Self {
        Self { repo }
    }

    pub fn disabled() -> Self {
        struct DisabledRepo;
        #[async_trait::async_trait]
        impl CategoryJobServiceSubFeeRepository for DisabledRepo {
            async fn list(
                &self,
                _input: &CategoryJobServiceSubFeeListInput,
            ) -> Result<CategoryJobServiceSubFeePage, RepoError> {
                Err(RepoError::Backend(
                    "CategoryJobServiceSubFeeService::disabled — repository not wired (set KOKKAK_DATABASE__SQLSERVER_URL)"
                        .into(),
                ))
            }
            async fn create(
                &self,
                _input: &CategoryJobServiceSubFeeCreateInput,
            ) -> Result<CategoryJobServiceSubFeeCreateResult, RepoError> {
                Err(RepoError::Backend(
                    "CategoryJobServiceSubFeeService::disabled — repository not wired (set KOKKAK_DATABASE__SQLSERVER_URL)"
                        .into(),
                ))
            }
            async fn update(
                &self,
                _input: &CategoryJobServiceSubFeeUpdateInput,
            ) -> Result<CategoryJobServiceSubFeeUpdateResult, RepoError> {
                Err(RepoError::Backend(
                    "CategoryJobServiceSubFeeService::disabled — repository not wired (set KOKKAK_DATABASE__SQLSERVER_URL)"
                        .into(),
                ))
            }
            async fn delete(
                &self,
                _input: &CategoryJobServiceSubFeeDeleteInput,
            ) -> Result<CategoryJobServiceSubFeeDeleteResult, RepoError> {
                Err(RepoError::Backend(
                    "CategoryJobServiceSubFeeService::disabled — repository not wired (set KOKKAK_DATABASE__SQLSERVER_URL)"
                        .into(),
                ))
            }
            async fn autocomplete(
                &self,
                _input: &CategoryJobServiceSubFeeAutocompleteInput,
            ) -> Result<Vec<CategoryJobServiceSubFeeAutocompleteRow>, RepoError> {
                Err(RepoError::Backend(
                    "CategoryJobServiceSubFeeService::disabled — repository not wired (set KOKKAK_DATABASE__SQLSERVER_URL)"
                        .into(),
                ))
            }
            async fn detail(
                &self,
                _category_job_service_sub_fee_guid: &str,
            ) -> Result<Option<CategoryJobServiceSubFeeDetailRow>, RepoError> {
                Err(RepoError::Backend(
                    "CategoryJobServiceSubFeeService::disabled — repository not wired (set KOKKAK_DATABASE__SQLSERVER_URL)"
                        .into(),
                ))
            }
        }
        let repo: Arc<dyn CategoryJobServiceSubFeeRepository> = Arc::new(DisabledRepo);
        Self { repo }
    }

    pub fn repo(&self) -> Arc<dyn CategoryJobServiceSubFeeRepository> {
        Arc::clone(&self.repo)
    }

    pub async fn list(
        &self,
        input: CategoryJobServiceSubFeeListInput,
    ) -> Result<CategoryJobServiceSubFeePage, RepoError> {
        self.repo.list(&input).await
    }

    pub async fn create(
        &self,
        input: CategoryJobServiceSubFeeCreateInput,
    ) -> Result<CategoryJobServiceSubFeeCreateResult, RepoError> {
        self.repo.create(&input).await
    }

    pub async fn update(
        &self,
        input: CategoryJobServiceSubFeeUpdateInput,
    ) -> Result<CategoryJobServiceSubFeeUpdateResult, RepoError> {
        self.repo.update(&input).await
    }

    pub async fn delete(
        &self,
        input: CategoryJobServiceSubFeeDeleteInput,
    ) -> Result<CategoryJobServiceSubFeeDeleteResult, RepoError> {
        self.repo.delete(&input).await
    }

    pub async fn autocomplete(
        &self,
        input: CategoryJobServiceSubFeeAutocompleteInput,
    ) -> Result<Vec<CategoryJobServiceSubFeeAutocompleteRow>, RepoError> {
        self.repo.autocomplete(&input).await
    }

    pub async fn detail(
        &self,
        category_job_service_sub_fee_guid: &str,
    ) -> Result<Option<CategoryJobServiceSubFeeDetailRow>, RepoError> {
        self.repo.detail(category_job_service_sub_fee_guid).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use kokkak_domain::CategoryJobServiceSubFeeAdminRow;
    use std::sync::Mutex;

    #[derive(Default)]
    struct MockRepo {
        items: Mutex<Vec<CategoryJobServiceSubFeeAdminRow>>,
        last_input: Mutex<Option<CategoryJobServiceSubFeeListInput>>,
        last_create_input: Mutex<Option<CategoryJobServiceSubFeeCreateInput>>,
        last_update_input: Mutex<Option<CategoryJobServiceSubFeeUpdateInput>>,
        last_delete_input: Mutex<Option<CategoryJobServiceSubFeeDeleteInput>>,
        create_result: Mutex<Option<Result<CategoryJobServiceSubFeeCreateResult, RepoError>>>,
        update_result: Mutex<Option<Result<CategoryJobServiceSubFeeUpdateResult, RepoError>>>,
        delete_result: Mutex<Option<Result<CategoryJobServiceSubFeeDeleteResult, RepoError>>>,
    }

    #[async_trait::async_trait]
    impl CategoryJobServiceSubFeeRepository for MockRepo {
        async fn list(
            &self,
            input: &CategoryJobServiceSubFeeListInput,
        ) -> Result<CategoryJobServiceSubFeePage, RepoError> {
            *self.last_input.lock().unwrap() = Some(input.clone());
            let items = self.items.lock().unwrap().clone();
            let total = items.len() as i64;
            let page_size = input.page_size.unwrap_or(20);
            Ok(CategoryJobServiceSubFeePage {
                items,
                total_count: total,
                page: input.page.unwrap_or(1),
                page_size,
                total_page: if total == 0 {
                    0
                } else {
                    ((total + page_size as i64 - 1) / page_size as i64) as u32
                },
            })
        }
        async fn create(
            &self,
            input: &CategoryJobServiceSubFeeCreateInput,
        ) -> Result<CategoryJobServiceSubFeeCreateResult, RepoError> {
            *self.last_create_input.lock().unwrap() = Some(input.clone());
            let result = self.create_result.lock().unwrap().clone();
            match result {
                Some(r) => r,
                None => Ok(CategoryJobServiceSubFeeCreateResult {
                    success: true,
                    code: "INSERT_SUCCESS".into(),
                    message: "ok".into(),
                    category_job_service_sub_fee_guid: Some(
                        input
                            .category_job_service_sub_fee_guid
                            .clone()
                            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
                    ),
                }),
            }
        }
        async fn update(
            &self,
            input: &CategoryJobServiceSubFeeUpdateInput,
        ) -> Result<CategoryJobServiceSubFeeUpdateResult, RepoError> {
            *self.last_update_input.lock().unwrap() = Some(input.clone());
            let result = self.update_result.lock().unwrap().clone();
            match result {
                Some(r) => r,
                None => Ok(CategoryJobServiceSubFeeUpdateResult {
                    success: true,
                    code: "UPDATE_SUCCESS".into(),
                    message: "ok".into(),
                    category_job_service_sub_fee_guid: Some(
                        input.category_job_service_sub_fee_guid.clone(),
                    ),
                }),
            }
        }
        async fn delete(
            &self,
            input: &CategoryJobServiceSubFeeDeleteInput,
        ) -> Result<CategoryJobServiceSubFeeDeleteResult, RepoError> {
            *self.last_delete_input.lock().unwrap() = Some(input.clone());
            let result = self.delete_result.lock().unwrap().clone();
            match result {
                Some(r) => r,
                None => Ok(CategoryJobServiceSubFeeDeleteResult {
                    success: true,
                    code: "DELETE_SUCCESS".into(),
                    message: "ok".into(),
                    category_job_service_sub_fee_guid: Some(
                        input.category_job_service_sub_fee_guid.clone(),
                    ),
                }),
            }
        }
    }

    fn make_fee(guid: &str, header: &str, status: i32) -> CategoryJobServiceSubFeeAdminRow {
        CategoryJobServiceSubFeeAdminRow {
            category_job_service_sub_fee_guid: guid.into(),
            category_job_service_sub_fee_header: header.into(),
            category_job_service_sub_fee_description: "desc".into(),
            category_job_service_sub_fee_price: rust_decimal::Decimal::new(1500, 0),
            category_job_service_sub_fee_status: status,
            category_job_service_sub_fee_icon: "icon.webp".into(),
            category_job_service_sub_fee_create_at: Some(Utc::now()),
            category_job_service_sub_fee_create_by: "admin".into(),
            category_job_service_sub_fee_update_at: Some(Utc::now()),
            category_job_service_sub_fee_update_by: "admin".into(),
        }
    }

    #[tokio::test]
    async fn list_forwards_input_and_returns_repo_rows() {
        let repo = MockRepo {
            items: Mutex::new(vec![make_fee("g1", "Service Fee A", 1)]),
            ..Default::default()
        };
        let repo: Arc<dyn CategoryJobServiceSubFeeRepository> = Arc::new(repo);
        let svc = CategoryJobServiceSubFeeService::new(repo);

        let page = svc
            .list(CategoryJobServiceSubFeeListInput {
                category_job_service_sub_fee_guid: Some("g1".into()),
                keyword: Some("Fee".into()),
                status: Some(1),
                locale: Some("la".into()),
                include_deleted: Some(false),
                page: Some(1),
                page_size: Some(20),
            })
            .await
            .unwrap();

        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0].category_job_service_sub_fee_guid, "g1");
        assert_eq!(page.total_count, 1);
        assert_eq!(page.page, 1);
        assert_eq!(page.page_size, 20);
    }

    #[tokio::test]
    async fn list_with_empty_input_yields_empty_page() {
        let repo = MockRepo::default();
        let repo: Arc<dyn CategoryJobServiceSubFeeRepository> = Arc::new(repo);
        let svc = CategoryJobServiceSubFeeService::new(repo);

        let page = svc
            .list(CategoryJobServiceSubFeeListInput::default())
            .await
            .unwrap();

        assert!(page.items.is_empty());
        assert_eq!(page.total_count, 0);
        assert_eq!(page.total_page, 0);
    }

    #[tokio::test]
    async fn create_forwards_input_and_returns_success_result() {
        let repo = MockRepo::default();
        let repo: Arc<dyn CategoryJobServiceSubFeeRepository> = Arc::new(repo);
        let svc = CategoryJobServiceSubFeeService::new(repo);

        let result = svc
            .create(CategoryJobServiceSubFeeCreateInput {
                category_job_service_sub_fee_guid: Some("f1".into()),
                category_job_service_sub_fee_header_la: Some("ຄ່າຂົນສົ່ງ".into()),
                category_job_service_sub_fee_description_la: Some("desc la".into()),
                category_job_service_sub_fee_header_en: Some("Delivery".into()),
                category_job_service_sub_fee_description_en: Some("desc en".into()),
                category_job_service_sub_fee_header_th: Some("ค่าขนส่ง".into()),
                category_job_service_sub_fee_description_th: Some("desc th".into()),
                category_job_service_sub_fee_header_zh: Some("运费".into()),
                category_job_service_sub_fee_description_zh: Some("desc zh".into()),
                category_job_service_sub_fee_price: rust_decimal::Decimal::new(5000, 2),
                category_job_service_sub_fee_status: 1,
                category_job_service_sub_fee_icon: Some("fee/x.webp".into()),
                create_by: "admin-1".into(),
            })
            .await
            .unwrap();

        assert!(result.success);
        assert_eq!(result.code, "INSERT_SUCCESS");
        assert_eq!(result.category_job_service_sub_fee_guid, Some("f1".into()));
    }

    #[tokio::test]
    async fn create_surfaces_duplicate_from_repo() {
        let dup = Ok(CategoryJobServiceSubFeeCreateResult {
            success: false,
            code: "DUPLICATE_GUID".into(),
            message: "duplicate".into(),
            category_job_service_sub_fee_guid: Some("dup".into()),
        });
        let repo = MockRepo {
            create_result: Mutex::new(Some(dup)),
            ..Default::default()
        };
        let repo: Arc<dyn CategoryJobServiceSubFeeRepository> = Arc::new(repo);
        let svc = CategoryJobServiceSubFeeService::new(repo);

        let result = svc
            .create(CategoryJobServiceSubFeeCreateInput {
                category_job_service_sub_fee_guid: Some("dup".into()),
                category_job_service_sub_fee_header_la: None,
                category_job_service_sub_fee_description_la: None,
                category_job_service_sub_fee_header_en: None,
                category_job_service_sub_fee_description_en: None,
                category_job_service_sub_fee_header_th: None,
                category_job_service_sub_fee_description_th: None,
                category_job_service_sub_fee_header_zh: None,
                category_job_service_sub_fee_description_zh: None,
                category_job_service_sub_fee_price: rust_decimal::Decimal::ZERO,
                category_job_service_sub_fee_status: 1,
                category_job_service_sub_fee_icon: None,
                create_by: "admin-1".into(),
            })
            .await
            .unwrap();

        assert!(!result.success);
        assert_eq!(result.code, "DUPLICATE_GUID");
    }
}
