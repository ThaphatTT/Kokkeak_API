use std::sync::Arc;

use kokkak_domain::traits::category_job_service_sub::{
    CategoryJobServiceSubRepository, SubImageForCreate, SubImageForUpdate,
};
use kokkak_domain::traits::user::RepoError;
use kokkak_domain::{
    CategoryJobServiceSubCreateInput, CategoryJobServiceSubCreateResult,
    CategoryJobServiceSubCreateSpInput, CategoryJobServiceSubCreateSpResult,
    CategoryJobServiceSubDeleteResult, CategoryJobServiceSubDetailBundle,
    CategoryJobServiceSubImageCreateInput, CategoryJobServiceSubImageCreateResult,
    CategoryJobServiceSubImageDeleteInput, CategoryJobServiceSubImageDeleteResult,
    CategoryJobServiceSubImageRow, CategoryJobServiceSubRow, CategoryJobServiceSubUpdateInput,
    CategoryJobServiceSubUpdateResult,
};

pub struct CategoryJobServiceSubService {
    repo: Arc<dyn CategoryJobServiceSubRepository>,
}

impl CategoryJobServiceSubService {
    pub fn new(repo: Arc<dyn CategoryJobServiceSubRepository>) -> Self {
        Self { repo }
    }

    pub fn disabled() -> Self {
        struct DisabledRepo;

        #[async_trait::async_trait]
        impl CategoryJobServiceSubRepository for DisabledRepo {
            async fn list(
                &self,
                _category_job_service_guid: &str,
                _keyword: Option<&str>,
                _status: Option<i32>,
                _locale: &str,
                _include_deleted: bool,
            ) -> Result<Vec<CategoryJobServiceSubRow>, RepoError> {
                Err(RepoError::Backend(
                    "CategoryJobServiceSubService::disabled — repository not wired (set KOKKAK_DATABASE__SQLSERVER_URL)"
                        .into(),
                ))
            }

            async fn detail(
                &self,
                _category_job_service_sub_guid: &str,
            ) -> Result<CategoryJobServiceSubDetailBundle, RepoError> {
                Err(RepoError::Backend(
                    "CategoryJobServiceSubService::disabled — repository not wired".into(),
                ))
            }

            async fn list_images(
                &self,
                _category_job_service_sub_guid: &str,
            ) -> Result<Vec<CategoryJobServiceSubImageRow>, RepoError> {
                Err(RepoError::Backend(
                    "CategoryJobServiceSubService::disabled — repository not wired".into(),
                ))
            }

            async fn create(
                &self,
                _input: &CategoryJobServiceSubCreateInput,
            ) -> Result<CategoryJobServiceSubCreateResult, RepoError> {
                Err(RepoError::Backend(
                    "CategoryJobServiceSubService::disabled — repository not wired".into(),
                ))
            }

            async fn update(
                &self,
                _input: &CategoryJobServiceSubUpdateInput,
            ) -> Result<CategoryJobServiceSubUpdateResult, RepoError> {
                Err(RepoError::Backend(
                    "CategoryJobServiceSubService::disabled — repository not wired".into(),
                ))
            }

            async fn delete(
                &self,
                _category_job_service_sub_guid: &str,
                _actor_user_guid: &str,
            ) -> Result<CategoryJobServiceSubDeleteResult, RepoError> {
                Err(RepoError::Backend(
                    "CategoryJobServiceSubService::disabled — repository not wired".into(),
                ))
            }

            async fn create_image(
                &self,
                _input: &CategoryJobServiceSubImageCreateInput,
            ) -> Result<CategoryJobServiceSubImageCreateResult, RepoError> {
                Err(RepoError::Backend(
                    "CategoryJobServiceSubService::disabled — repository not wired".into(),
                ))
            }

            async fn delete_image(
                &self,
                _input: &CategoryJobServiceSubImageDeleteInput,
            ) -> Result<CategoryJobServiceSubImageDeleteResult, RepoError> {
                Err(RepoError::Backend(
                    "CategoryJobServiceSubService::disabled — repository not wired".into(),
                ))
            }

            async fn create_with_images(
                &self,
                _input: &CategoryJobServiceSubCreateInput,
                _image_paths: &[SubImageForCreate],
            ) -> Result<CategoryJobServiceSubCreateResult, RepoError> {
                Err(RepoError::Backend(
                    "CategoryJobServiceSubService::disabled — repository not wired".into(),
                ))
            }

            async fn update_with_images(
                &self,
                _input: &CategoryJobServiceSubUpdateInput,
                _image_paths: &[SubImageForUpdate],
            ) -> Result<CategoryJobServiceSubUpdateResult, RepoError> {
                Err(RepoError::Backend(
                    "CategoryJobServiceSubService::disabled — repository not wired".into(),
                ))
            }

            async fn create_via_sp(
                &self,
                _input: &CategoryJobServiceSubCreateSpInput,
            ) -> Result<CategoryJobServiceSubCreateSpResult, RepoError> {
                Err(RepoError::Backend(
                    "CategoryJobServiceSubService::disabled — repository not wired".into(),
                ))
            }
        }

        let repo: Arc<dyn CategoryJobServiceSubRepository> = Arc::new(DisabledRepo);
        Self { repo }
    }

    pub fn repo(&self) -> Arc<dyn CategoryJobServiceSubRepository> {
        Arc::clone(&self.repo)
    }

    pub async fn list(
        &self,
        category_job_service_guid: &str,
        keyword: Option<&str>,
        status: Option<i32>,
        locale: &str,
        include_deleted: bool,
    ) -> Result<Vec<CategoryJobServiceSubRow>, RepoError> {
        self.repo
            .list(
                category_job_service_guid,
                keyword,
                status,
                locale,
                include_deleted,
            )
            .await
    }

    pub async fn detail(
        &self,
        category_job_service_sub_guid: &str,
    ) -> Result<CategoryJobServiceSubDetailBundle, RepoError> {
        self.repo.detail(category_job_service_sub_guid).await
    }

    pub async fn list_images(
        &self,
        category_job_service_sub_guid: &str,
    ) -> Result<Vec<CategoryJobServiceSubImageRow>, RepoError> {
        self.repo.list_images(category_job_service_sub_guid).await
    }

    pub async fn create(
        &self,
        input: CategoryJobServiceSubCreateInput,
    ) -> Result<CategoryJobServiceSubCreateResult, RepoError> {
        self.repo.create(&input).await
    }

    pub async fn update(
        &self,
        input: CategoryJobServiceSubUpdateInput,
    ) -> Result<CategoryJobServiceSubUpdateResult, RepoError> {
        self.repo.update(&input).await
    }

    pub async fn delete(
        &self,
        category_job_service_sub_guid: &str,
        actor_user_guid: &str,
    ) -> Result<CategoryJobServiceSubDeleteResult, RepoError> {
        self.repo
            .delete(category_job_service_sub_guid, actor_user_guid)
            .await
    }

    pub async fn create_image(
        &self,
        input: CategoryJobServiceSubImageCreateInput,
    ) -> Result<CategoryJobServiceSubImageCreateResult, RepoError> {
        self.repo.create_image(&input).await
    }

    pub async fn delete_image(
        &self,
        input: CategoryJobServiceSubImageDeleteInput,
    ) -> Result<CategoryJobServiceSubImageDeleteResult, RepoError> {
        self.repo.delete_image(&input).await
    }

    pub async fn create_with_images(
        &self,
        input: CategoryJobServiceSubCreateInput,
        image_paths: Vec<SubImageForCreate>,
    ) -> Result<CategoryJobServiceSubCreateResult, RepoError> {
        self.repo.create_with_images(&input, &image_paths).await
    }

    pub async fn update_with_images(
        &self,
        input: CategoryJobServiceSubUpdateInput,
        image_paths: Vec<SubImageForUpdate>,
    ) -> Result<CategoryJobServiceSubUpdateResult, RepoError> {
        self.repo.update_with_images(&input, &image_paths).await
    }

    pub async fn create_via_sp(
        &self,
        input: CategoryJobServiceSubCreateSpInput,
    ) -> Result<CategoryJobServiceSubCreateSpResult, RepoError> {
        self.repo.create_via_sp(&input).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use rust_decimal::Decimal;
    use std::sync::Mutex;

    #[derive(Default)]
    struct MockRepo {
        rows: Mutex<Vec<CategoryJobServiceSubRow>>,
        images: Mutex<Vec<CategoryJobServiceSubImageRow>>,
        last_delete: Mutex<Option<(String, String)>>,
    }

    #[async_trait::async_trait]
    impl CategoryJobServiceSubRepository for MockRepo {
        async fn list(
            &self,
            _category_job_service_guid: &str,
            _keyword: Option<&str>,
            _status: Option<i32>,
            _locale: &str,
            _include_deleted: bool,
        ) -> Result<Vec<CategoryJobServiceSubRow>, RepoError> {
            Ok(self.rows.lock().unwrap().clone())
        }

        async fn detail(
            &self,
            category_job_service_sub_guid: &str,
        ) -> Result<CategoryJobServiceSubDetailBundle, RepoError> {
            let sub = self
                .rows
                .lock()
                .unwrap()
                .iter()
                .find(|r| r.category_job_service_sub_guid == category_job_service_sub_guid)
                .cloned()
                .ok_or_else(|| {
                    RepoError::Backend(format!("SUB_NOT_FOUND: {category_job_service_sub_guid}"))
                })?;
            let images = self
                .images
                .lock()
                .unwrap()
                .iter()
                .filter(|i| {
                    i.category_job_service_sub_img_category_job_service_sub_guid
                        == category_job_service_sub_guid
                })
                .cloned()
                .collect();
            Ok(CategoryJobServiceSubDetailBundle {
                sub,
                images,
                fees: vec![],
                warranties: vec![],
            })
        }

        async fn list_images(
            &self,
            category_job_service_sub_guid: &str,
        ) -> Result<Vec<CategoryJobServiceSubImageRow>, RepoError> {
            Ok(self
                .images
                .lock()
                .unwrap()
                .iter()
                .filter(|i| {
                    i.category_job_service_sub_img_category_job_service_sub_guid
                        == category_job_service_sub_guid
                })
                .cloned()
                .collect())
        }

        async fn create(
            &self,
            input: &CategoryJobServiceSubCreateInput,
        ) -> Result<CategoryJobServiceSubCreateResult, RepoError> {
            let guid = uuid::Uuid::new_v4().to_string();
            self.rows.lock().unwrap().push(CategoryJobServiceSubRow {
                category_job_service_sub_guid: guid.clone(),
                category_job_service_sub_category_job_service_main_guid: input
                    .category_job_service_guid
                    .clone(),
                category_job_service_sub_category_job_service_sub_fee_guid: String::new(),
                category_job_service_sub_category_job_service_sub_warranty_guid: String::new(),
                category_job_service_name: "Air Con".into(),
                category_job_service_sub_name: input.category_job_service_sub_name.clone(),
                category_job_service_sub_locale: "la".into(),
                category_job_service_sub_start_price: input.category_job_service_sub_start_price,
                category_job_service_sub_description: input
                    .category_job_service_sub_description
                    .clone(),
                category_job_service_sub_status: 1,
                main_img_path: String::new(),
                main_img_url: None,
                category_job_service_sub_create_at: Some(Utc::now()),
                category_job_service_sub_create_by: input.create_by.clone(),
                category_job_service_sub_update_at: None,
                category_job_service_sub_update_by: String::new(),
            });
            Ok(CategoryJobServiceSubCreateResult {
                success: true,
                code: "SUCCESS".into(),
                message: "ok".into(),
                category_job_service_sub_guid: Some(guid),
            })
        }

        async fn update(
            &self,
            input: &CategoryJobServiceSubUpdateInput,
        ) -> Result<CategoryJobServiceSubUpdateResult, RepoError> {
            let mut rows = self.rows.lock().unwrap();
            if let Some(row) = rows
                .iter_mut()
                .find(|r| r.category_job_service_sub_guid == input.category_job_service_sub_guid)
            {
                row.category_job_service_sub_name = input.category_job_service_sub_name.clone();
                row.category_job_service_sub_start_price =
                    input.category_job_service_sub_start_price;
                row.category_job_service_sub_description =
                    input.category_job_service_sub_description.clone();
                row.category_job_service_sub_status = input.category_job_service_sub_status;
                row.category_job_service_sub_update_at = Some(Utc::now());
                row.category_job_service_sub_update_by = input.update_by.clone();
            } else {
                return Err(RepoError::Backend(format!(
                    "SUB_NOT_FOUND: {}",
                    input.category_job_service_sub_guid
                )));
            }
            Ok(CategoryJobServiceSubUpdateResult {
                success: true,
                code: "SUCCESS".into(),
                message: "ok".into(),
                category_job_service_sub_guid: input.category_job_service_sub_guid.clone(),
            })
        }

        async fn delete(
            &self,
            category_job_service_sub_guid: &str,
            actor_user_guid: &str,
        ) -> Result<CategoryJobServiceSubDeleteResult, RepoError> {
            *self.last_delete.lock().unwrap() = Some((
                category_job_service_sub_guid.to_string(),
                actor_user_guid.to_string(),
            ));
            self.rows
                .lock()
                .unwrap()
                .retain(|r| r.category_job_service_sub_guid != category_job_service_sub_guid);
            Ok(CategoryJobServiceSubDeleteResult {
                success: true,
                code: "SUCCESS".into(),
                message: "ok".into(),
                category_job_service_sub_guid: category_job_service_sub_guid.to_string(),
            })
        }

        async fn create_image(
            &self,
            input: &CategoryJobServiceSubImageCreateInput,
        ) -> Result<CategoryJobServiceSubImageCreateResult, RepoError> {
            let guid = uuid::Uuid::new_v4().to_string();
            self.images
                .lock()
                .unwrap()
                .push(CategoryJobServiceSubImageRow {
                    category_job_service_sub_img_guid: guid.clone(),
                    category_job_service_sub_img_category_job_service_sub_guid: input
                        .category_job_service_sub_guid
                        .clone(),
                    category_job_service_sub_img_type: input.img_type,
                    category_job_service_sub_img_type_language: 0,
                    category_job_service_sub_img_priority: input.img_priority,
                    category_job_service_sub_img_path: input.img_path.clone(),
                    category_job_service_sub_img_url: None,
                    category_job_service_sub_img_status: 1,
                    category_job_service_sub_img_create_at: Some(Utc::now()),
                    category_job_service_sub_img_create_by: input.create_by.clone(),
                });
            Ok(CategoryJobServiceSubImageCreateResult {
                success: true,
                code: "SUCCESS".into(),
                message: "ok".into(),
                category_job_service_sub_img_guid: Some(guid),
            })
        }

        async fn delete_image(
            &self,
            input: &CategoryJobServiceSubImageDeleteInput,
        ) -> Result<CategoryJobServiceSubImageDeleteResult, RepoError> {
            self.images.lock().unwrap().retain(|i| {
                i.category_job_service_sub_img_guid != input.category_job_service_sub_img_guid
            });
            Ok(CategoryJobServiceSubImageDeleteResult {
                success: true,
                code: "SUCCESS".into(),
                message: "ok".into(),
                category_job_service_sub_img_guid: input.category_job_service_sub_img_guid.clone(),
            })
        }

        async fn create_with_images(
            &self,
            input: &CategoryJobServiceSubCreateInput,
            image_paths: &[SubImageForCreate],
        ) -> Result<CategoryJobServiceSubCreateResult, RepoError> {
            let create_res = self.create(input).await?;
            let sub_guid = create_res
                .category_job_service_sub_guid
                .clone()
                .ok_or_else(|| RepoError::Backend("no sub_guid from create".into()))?;
            for img in image_paths {
                let img_input = CategoryJobServiceSubImageCreateInput {
                    category_job_service_sub_guid: sub_guid.clone(),
                    img_type: img.img_type,
                    img_priority: img.img_priority,
                    img_path: img.img_path.clone(),
                    create_by: input.create_by.clone(),
                };
                self.create_image(&img_input).await?;
            }
            Ok(create_res)
        }

        async fn update_with_images(
            &self,
            input: &CategoryJobServiceSubUpdateInput,
            image_paths: &[SubImageForUpdate],
        ) -> Result<CategoryJobServiceSubUpdateResult, RepoError> {
            let update_res = self.update(input).await?;
            for img in image_paths {
                let img_input = CategoryJobServiceSubImageCreateInput {
                    category_job_service_sub_guid: input.category_job_service_sub_guid.clone(),
                    img_type: img.img_type,
                    img_priority: img.img_priority,
                    img_path: img.img_path.clone(),
                    create_by: input.update_by.clone(),
                };
                self.create_image(&img_input).await?;
            }
            Ok(update_res)
        }

        async fn create_via_sp(
            &self,
            input: &CategoryJobServiceSubCreateSpInput,
        ) -> Result<CategoryJobServiceSubCreateSpResult, RepoError> {
            let guid = input
                .category_job_service_sub_guid
                .clone()
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
            self.rows.lock().unwrap().push(CategoryJobServiceSubRow {
                category_job_service_sub_guid: guid.clone(),
                category_job_service_sub_category_job_service_main_guid: input
                    .category_job_service_main_guid
                    .clone(),
                category_job_service_sub_category_job_service_sub_fee_guid: String::new(),
                category_job_service_sub_category_job_service_sub_warranty_guid: String::new(),
                category_job_service_name: String::new(),
                category_job_service_sub_name: input
                    .category_job_service_sub_name_la
                    .clone()
                    .or_else(|| input.category_job_service_sub_name_en.clone())
                    .unwrap_or_default(),
                category_job_service_sub_locale: "la".into(),
                category_job_service_sub_start_price: input.category_job_service_sub_start_price,
                category_job_service_sub_description: input
                    .category_job_service_sub_description_la
                    .clone()
                    .unwrap_or_default(),
                category_job_service_sub_status: input.category_job_service_sub_status,
                main_img_path: String::new(),
                main_img_url: None,
                category_job_service_sub_create_at: Some(Utc::now()),
                category_job_service_sub_create_by: input.create_by.clone(),
                category_job_service_sub_update_at: None,
                category_job_service_sub_update_by: String::new(),
            });
            Ok(CategoryJobServiceSubCreateSpResult {
                success: true,
                code: "INSERT_SUCCESS".into(),
                message: "ok".into(),
                category_job_service_sub_guid: Some(guid),
                warranty_count: input.warranties.len() as i32,
                fee_count: input.fees.len() as i32,
                image_count: input.images.len() as i32,
            })
        }
    }

    fn make_sub_row(guid: &str, main_guid: &str, name: &str) -> CategoryJobServiceSubRow {
        CategoryJobServiceSubRow {
            category_job_service_sub_guid: guid.into(),
            category_job_service_sub_category_job_service_main_guid: main_guid.into(),
            category_job_service_sub_category_job_service_sub_fee_guid: String::new(),
            category_job_service_sub_category_job_service_sub_warranty_guid: String::new(),
            category_job_service_name: "Air Con".into(),
            category_job_service_sub_name: name.into(),
            category_job_service_sub_locale: "la".into(),
            category_job_service_sub_start_price: Decimal::new(900000, 2),
            category_job_service_sub_description: "desc".into(),
            category_job_service_sub_status: 1,
            main_img_path: String::new(),
            main_img_url: None,
            category_job_service_sub_create_at: Some(Utc::now()),
            category_job_service_sub_create_by: "admin".into(),
            category_job_service_sub_update_at: None,
            category_job_service_sub_update_by: String::new(),
        }
    }

    #[tokio::test]
    async fn list_forwards_filters() {
        let repo = MockRepo {
            rows: Mutex::new(vec![make_sub_row("s1", "m1", "ล้างแอร์")]),
            ..Default::default()
        };
        let repo: Arc<dyn CategoryJobServiceSubRepository> = Arc::new(repo);
        let svc = CategoryJobServiceSubService::new(repo);

        let rows = svc
            .list("m1", Some("ล้าง"), Some(1), "la", false)
            .await
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].category_job_service_sub_name, "ล้างแอร์");
    }

    #[tokio::test]
    async fn create_detail_update_delete_roundtrip() {
        let repo = MockRepo::default();
        let repo_arc: Arc<dyn CategoryJobServiceSubRepository> = Arc::new(repo);
        let svc = CategoryJobServiceSubService::new(repo_arc);

        let create_res = svc
            .create(CategoryJobServiceSubCreateInput {
                category_job_service_guid: "m-1".into(),
                category_job_service_sub_name: "ล้างแอร์ 9000-12000 BTU".into(),
                category_job_service_sub_start_price: Decimal::new(900000, 2),
                category_job_service_sub_description: "desc".into(),
                create_by: "admin-1".into(),
                images: vec![],
            })
            .await
            .unwrap();
        assert!(create_res.success);
        let guid = create_res.category_job_service_sub_guid.clone().unwrap();

        let detail = svc.detail(&guid).await.unwrap();
        assert_eq!(detail.sub.category_job_service_sub_guid, guid);

        let update_res = svc
            .update(CategoryJobServiceSubUpdateInput {
                category_job_service_sub_guid: guid.clone(),
                category_job_service_guid: "m-1".into(),
                category_job_service_sub_name: "ล้างแอร์ 9000-18000 BTU".into(),
                category_job_service_sub_start_price: Decimal::new(1200000, 2),
                category_job_service_sub_description: "desc v2".into(),
                category_job_service_sub_status: 1,
                update_by: "admin-1".into(),
                images: vec![],
            })
            .await
            .unwrap();
        assert!(update_res.success);

        let del_res = svc.delete(&guid, "admin-1").await.unwrap();
        assert!(del_res.success);
        assert_eq!(del_res.category_job_service_sub_guid, guid);
    }
}
