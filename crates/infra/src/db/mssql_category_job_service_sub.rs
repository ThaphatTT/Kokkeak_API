use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use tiberius::ToSql;

use kokkak_domain::category_job_service_sub::CategoryJobServiceSubError;
use kokkak_domain::traits::category_job_service_sub::{
    CategoryJobServiceSubRepository, SubImageForCreate, SubImageForUpdate,
};
use kokkak_domain::traits::user::RepoError;
use kokkak_domain::{
    CategoryJobServiceSubCreateInput, CategoryJobServiceSubCreateResult,
    CategoryJobServiceSubCreateSpInput, CategoryJobServiceSubCreateSpResult,
    CategoryJobServiceSubDeleteResult, CategoryJobServiceSubDetailBundle,
    CategoryJobServiceSubDetailFeeRow, CategoryJobServiceSubDetailImageRow,
    CategoryJobServiceSubDetailRow, CategoryJobServiceSubDetailWarrantyRow,
    CategoryJobServiceSubFeeRow, CategoryJobServiceSubImageCreateInput,
    CategoryJobServiceSubImageCreateResult, CategoryJobServiceSubImageDeleteInput,
    CategoryJobServiceSubImageDeleteResult, CategoryJobServiceSubImageRow,
    CategoryJobServiceSubRow, CategoryJobServiceSubUpdateInput, CategoryJobServiceSubUpdateResult,
    CategoryJobServiceSubUpdateSpInput, CategoryJobServiceSubUpdateSpResult,
    CategoryJobServiceSubWarrantyRow,
};

use crate::db::mssql::{
    begin_tx, commit_tx, exec_sp, exec_sp_multi, exec_sp_on, read_guid_str, read_i32, read_str,
    rollback_tx, MssqlPool,
};

#[derive(Clone)]
pub struct MssqlCategoryJobServiceSubRepository {
    pool: MssqlPool,
}

impl MssqlCategoryJobServiceSubRepository {
    pub fn new(pool: MssqlPool) -> Self {
        Self { pool }
    }

    pub fn disabled() -> Self {
        Self {
            pool: crate::db::mssql::build_disabled_pool(),
        }
    }
}

#[async_trait]
impl CategoryJobServiceSubRepository for MssqlCategoryJobServiceSubRepository {
    async fn list(
        &self,
        category_job_service_guid: &str,
        keyword: Option<&str>,
        status: Option<i32>,
        locale: &str,
        include_deleted: bool,
    ) -> Result<Vec<CategoryJobServiceSubRow>, RepoError> {
        let main_guid = category_job_service_guid;
        let kw = keyword;
        let status_param: Option<i32> = status;
        let loc = locale;
        let inc_deleted = include_deleted;

        let params: &[&dyn ToSql] = &[&main_guid, &kw, &status_param, &loc, &inc_deleted];

        let rows = exec_sp(
            &self.pool,
            "EXEC dbo.SP_CATEGORY_JOB_SERVICE_SUB_GET \
                @p_category_job_service_guid = @P1, \
                @p_keyword = @P2, \
                @p_status = @P3, \
                @p_locale = @P4, \
                @p_include_deleted = @P5",
            params,
        )
        .await?;
        Ok(rows.iter().map(row_to_sub_row).collect())
    }

    async fn detail(
        &self,
        category_job_service_sub_guid: &str,
        locale: &str,
    ) -> Result<CategoryJobServiceSubDetailBundle, RepoError> {
        let sub_guid = category_job_service_sub_guid;
        let loc: &str = locale;
        let params: &[&dyn ToSql] = &[&sub_guid, &loc];

        let sets = exec_sp_multi(
            &self.pool,
            "EXEC dbo.SP_CATEGORY_JOB_SERVICE_SUB_DETAIL_GET \
                @p_category_job_service_sub_guid = @P1, \
                @p_locale = @P2",
            params,
        )
        .await?;

        let mut sets = sets;
        let detail_set = if !sets.is_empty() {
            sets.remove(0)
        } else {
            Vec::new()
        };
        let sub_row = detail_set.first().ok_or_else(|| {
            RepoError::Backend(
                "SP_CATEGORY_JOB_SERVICE_SUB_DETAIL_GET returned no detail row (SUB_NOT_FOUND)"
                    .into(),
            )
        })?;
        let sub = row_to_sub_detail_row(sub_row);

        let warranties_set = if !sets.is_empty() {
            sets.remove(0)
        } else {
            Vec::new()
        };
        let fees_set = if !sets.is_empty() {
            sets.remove(0)
        } else {
            Vec::new()
        };
        let images_set = if !sets.is_empty() {
            sets.remove(0)
        } else {
            Vec::new()
        };

        Ok(CategoryJobServiceSubDetailBundle {
            sub,
            warranties: warranties_set
                .iter()
                .map(row_to_sub_detail_warranty_row)
                .collect(),
            fees: fees_set.iter().map(row_to_sub_detail_fee_row).collect(),
            images: images_set.iter().map(row_to_sub_detail_image_row).collect(),
        })
    }

    async fn list_images(
        &self,
        category_job_service_sub_guid: &str,
    ) -> Result<Vec<CategoryJobServiceSubImageRow>, RepoError> {
        let sub_guid = category_job_service_sub_guid;
        let params: &[&dyn ToSql] = &[&sub_guid];

        let rows = exec_sp(
            &self.pool,
            "EXEC dbo.SP_CATEGORY_JOB_SERVICE_SUB_IMG_GET \
                @p_category_job_service_sub_guid = @P1",
            params,
        )
        .await?;
        Ok(rows.iter().map(row_to_sub_image_row).collect())
    }

    async fn create(
        &self,
        input: &CategoryJobServiceSubCreateInput,
    ) -> Result<CategoryJobServiceSubCreateResult, RepoError> {
        let main_guid = input.category_job_service_guid.as_str();
        let name = input.category_job_service_sub_name.as_str();
        let start_price = input.category_job_service_sub_start_price;
        let description = input.category_job_service_sub_description.as_str();
        let create_by = input.create_by.as_str();

        let params: &[&dyn ToSql] = &[&main_guid, &name, &start_price, &description, &create_by];

        let rows = exec_sp(
            &self.pool,
            "EXEC dbo.SP_CATEGORY_JOB_SERVICE_SUB_CREATE \
                @p_category_job_service_guid = @P1, \
                @p_category_job_service_sub_name = @P2, \
                @p_category_job_service_sub_start_price = @P3, \
                @p_category_job_service_sub_description = @P4, \
                @p_create_by = @P5",
            params,
        )
        .await?;

        let row = rows.first().ok_or_else(|| {
            RepoError::Backend(
                "SP_CATEGORY_JOB_SERVICE_SUB_CREATE returned no row (driver/protocol mismatch)"
                    .into(),
            )
        })?;

        let success: bool = row.get::<bool, _>("success").unwrap_or(false);
        let code = read_str(row, "code").unwrap_or("").to_string();
        let message = read_str(row, "message").unwrap_or("").to_string();

        if !success {
            return Err(RepoError::Backend(format!(
                "{}: {code} — {message}",
                CategoryJobServiceSubError::CODE_SUCCESS
            )));
        }

        Ok(CategoryJobServiceSubCreateResult {
            success,
            code,
            message,
            category_job_service_sub_guid: {
                let s = read_guid_str(row, "category_job_service_sub_guid");
                if s.is_empty() {
                    None
                } else {
                    Some(s)
                }
            },
        })
    }

    async fn update(
        &self,
        input: &CategoryJobServiceSubUpdateInput,
    ) -> Result<CategoryJobServiceSubUpdateResult, RepoError> {
        let sub_guid = input.category_job_service_sub_guid.as_str();
        let main_guid = input.category_job_service_guid.as_str();
        let name = input.category_job_service_sub_name.as_str();
        let start_price = input.category_job_service_sub_start_price;
        let description = input.category_job_service_sub_description.as_str();
        let status = input.category_job_service_sub_status;
        let update_by = input.update_by.as_str();

        let params: &[&dyn ToSql] = &[
            &sub_guid,
            &main_guid,
            &name,
            &start_price,
            &description,
            &status,
            &update_by,
        ];

        let rows = exec_sp(
            &self.pool,
            "EXEC dbo.SP_CATEGORY_JOB_SERVICE_SUB_UPDATE \
                @p_category_job_service_sub_guid = @P1, \
                @p_category_job_service_guid = @P2, \
                @p_category_job_service_sub_name = @P3, \
                @p_category_job_service_sub_start_price = @P4, \
                @p_category_job_service_sub_description = @P5, \
                @p_category_job_service_sub_status = @P6, \
                @p_update_by = @P7",
            params,
        )
        .await?;

        let row = rows.first().ok_or_else(|| {
            RepoError::Backend(
                "SP_CATEGORY_JOB_SERVICE_SUB_UPDATE returned no row (driver/protocol mismatch)"
                    .into(),
            )
        })?;

        let success: bool = row.get::<bool, _>("success").unwrap_or(false);
        let code = read_str(row, "code").unwrap_or("").to_string();
        let message = read_str(row, "message").unwrap_or("").to_string();

        if !success {
            return Err(RepoError::Backend(format!(
                "{}: {code} — {message}",
                CategoryJobServiceSubError::CODE_SUCCESS
            )));
        }

        let returned_guid = read_guid_str(row, "category_job_service_sub_guid");
        let final_guid = if returned_guid.is_empty() {
            input.category_job_service_sub_guid.clone()
        } else {
            returned_guid
        };

        Ok(CategoryJobServiceSubUpdateResult {
            success,
            code,
            message,
            category_job_service_sub_guid: final_guid,
        })
    }

    async fn delete(
        &self,
        category_job_service_sub_guid: &str,
        actor_user_guid: &str,
    ) -> Result<CategoryJobServiceSubDeleteResult, RepoError> {
        let sub_guid = category_job_service_sub_guid;
        let actor = actor_user_guid;

        let params: &[&dyn ToSql] = &[&sub_guid, &actor];

        let rows = exec_sp(
            &self.pool,
            "EXEC dbo.SP_CATEGORY_JOB_SERVICE_SUB_DELETE \
                @p_category_job_service_sub_guid = @P1, \
                @p_update_by = @P2",
            params,
        )
        .await?;

        let row = rows.first().ok_or_else(|| {
            RepoError::Backend(
                "SP_CATEGORY_JOB_SERVICE_SUB_DELETE returned no row (driver/protocol mismatch)"
                    .into(),
            )
        })?;

        let success: bool = row.get::<bool, _>("success").unwrap_or(false);
        let code = read_str(row, "code").unwrap_or("").to_string();
        let message = read_str(row, "message").unwrap_or("").to_string();

        if !success {
            return Err(RepoError::Backend(format!(
                "{}: {code} — {message}",
                CategoryJobServiceSubError::CODE_SUCCESS
            )));
        }

        let returned_guid = read_guid_str(row, "category_job_service_sub_guid");
        let final_guid = if returned_guid.is_empty() {
            category_job_service_sub_guid.to_string()
        } else {
            returned_guid
        };

        Ok(CategoryJobServiceSubDeleteResult {
            success,
            code,
            message,
            category_job_service_sub_guid: final_guid,
        })
    }

    async fn create_image(
        &self,
        input: &CategoryJobServiceSubImageCreateInput,
    ) -> Result<CategoryJobServiceSubImageCreateResult, RepoError> {
        let sub_guid = input.category_job_service_sub_guid.as_str();
        let img_type = input.img_type;
        let img_priority = input.img_priority;
        let img_path = input.img_path.as_str();
        let create_by = input.create_by.as_str();

        let params: &[&dyn ToSql] = &[&sub_guid, &img_type, &img_priority, &img_path, &create_by];

        let rows = exec_sp(
            &self.pool,
            "EXEC dbo.SP_CATEGORY_JOB_SERVICE_SUB_IMG_CREATE \
                @p_category_job_service_sub_guid = @P1, \
                @p_img_type = @P2, \
                @p_img_priority = @P3, \
                @p_img_path = @P4, \
                @p_create_by = @P5",
            params,
        )
        .await?;

        let row = rows.first().ok_or_else(|| {
            RepoError::Backend(
                "SP_CATEGORY_JOB_SERVICE_SUB_IMG_CREATE returned no row (driver/protocol mismatch)"
                    .into(),
            )
        })?;

        let success: bool = row.get::<bool, _>("success").unwrap_or(false);
        let code = read_str(row, "code").unwrap_or("").to_string();
        let message = read_str(row, "message").unwrap_or("").to_string();

        if !success {
            return Err(RepoError::Backend(format!(
                "{}: {code} — {message}",
                CategoryJobServiceSubError::CODE_SUCCESS
            )));
        }

        Ok(CategoryJobServiceSubImageCreateResult {
            success,
            code,
            message,
            category_job_service_sub_img_guid: {
                let s = read_guid_str(row, "category_job_service_sub_img_guid");
                if s.is_empty() {
                    None
                } else {
                    Some(s)
                }
            },
        })
    }

    async fn delete_image(
        &self,
        input: &CategoryJobServiceSubImageDeleteInput,
    ) -> Result<CategoryJobServiceSubImageDeleteResult, RepoError> {
        let img_guid = input.category_job_service_sub_img_guid.as_str();
        let update_by = input.update_by.as_str();

        let params: &[&dyn ToSql] = &[&img_guid, &update_by];

        let rows = exec_sp(
            &self.pool,
            "EXEC dbo.SP_CATEGORY_JOB_SERVICE_SUB_IMG_DELETE \
                @p_category_job_service_sub_img_guid = @P1, \
                @p_update_by = @P2",
            params,
        )
        .await?;

        let row = rows.first().ok_or_else(|| {
            RepoError::Backend(
                "SP_CATEGORY_JOB_SERVICE_SUB_IMG_DELETE returned no row (driver/protocol mismatch)"
                    .into(),
            )
        })?;

        let success: bool = row.get::<bool, _>("success").unwrap_or(false);
        let code = read_str(row, "code").unwrap_or("").to_string();
        let message = read_str(row, "message").unwrap_or("").to_string();

        if !success {
            return Err(RepoError::Backend(format!(
                "{}: {code} — {message}",
                CategoryJobServiceSubError::CODE_SUCCESS
            )));
        }

        let returned_guid = read_guid_str(row, "category_job_service_sub_img_guid");
        let final_guid = if returned_guid.is_empty() {
            input.category_job_service_sub_img_guid.clone()
        } else {
            returned_guid
        };

        Ok(CategoryJobServiceSubImageDeleteResult {
            success,
            code,
            message,
            category_job_service_sub_img_guid: final_guid,
        })
    }

    async fn create_with_images(
        &self,
        input: &CategoryJobServiceSubCreateInput,
        image_paths: &[SubImageForCreate],
    ) -> Result<CategoryJobServiceSubCreateResult, RepoError> {
        let mut pooled = self
            .pool
            .get()
            .await
            .map_err(|e| RepoError::Backend(format!("acquire: {e}")))?;
        let conn: &mut crate::db::mssql::MssqlClient = &mut *pooled;

        begin_tx(conn).await?;

        let create_result = match self.create_in_tx(conn, input).await {
            Ok(r) => r,
            Err(e) => {
                rollback_tx(conn).await;
                return Err(e);
            }
        };

        let sub_guid = match create_result.category_job_service_sub_guid.clone() {
            Some(g) if !g.is_empty() => g,
            _ => {
                rollback_tx(conn).await;
                return Err(RepoError::Backend(
                    "SP_CATEGORY_JOB_SERVICE_SUB_CREATE returned no sub_guid".into(),
                ));
            }
        };

        for img in image_paths {
            let img_input = CategoryJobServiceSubImageCreateInput {
                category_job_service_sub_guid: sub_guid.clone(),
                img_type: img.img_type,
                img_priority: img.img_priority,
                img_path: img.img_path.clone(),
                create_by: input.create_by.clone(),
            };
            if let Err(e) = self.create_image_in_tx(conn, &img_input).await {
                tracing::warn!(error = %e, "create_image_in_tx failed — rolling back");
                rollback_tx(conn).await;
                return Err(e);
            }
        }

        commit_tx(conn).await?;
        Ok(create_result)
    }

    async fn update_with_images(
        &self,
        input: &CategoryJobServiceSubUpdateInput,
        image_paths: &[SubImageForUpdate],
    ) -> Result<CategoryJobServiceSubUpdateResult, RepoError> {
        let mut pooled = self
            .pool
            .get()
            .await
            .map_err(|e| RepoError::Backend(format!("acquire: {e}")))?;
        let conn: &mut crate::db::mssql::MssqlClient = &mut *pooled;

        begin_tx(conn).await?;

        let update_result = match self.update_in_tx(conn, input).await {
            Ok(r) => r,
            Err(e) => {
                rollback_tx(conn).await;
                return Err(e);
            }
        };

        for img in image_paths {
            let img_input = CategoryJobServiceSubImageCreateInput {
                category_job_service_sub_guid: input.category_job_service_sub_guid.clone(),
                img_type: img.img_type,
                img_priority: img.img_priority,
                img_path: img.img_path.clone(),
                create_by: input.update_by.clone(),
            };
            if let Err(e) = self.create_image_in_tx(conn, &img_input).await {
                tracing::warn!(error = %e, "create_image_in_tx (update) failed — rolling back");
                rollback_tx(conn).await;
                return Err(e);
            }
        }

        commit_tx(conn).await?;
        Ok(update_result)
    }

    async fn create_via_sp(
        &self,
        input: &CategoryJobServiceSubCreateSpInput,
    ) -> Result<CategoryJobServiceSubCreateSpResult, RepoError> {
        let warranties_json = serde_json::to_string(&input.warranties)
            .map_err(|e| RepoError::Backend(format!("warranties_json serialize: {e}")))?;
        let fees_json = serde_json::to_string(&input.fees)
            .map_err(|e| RepoError::Backend(format!("fees_json serialize: {e}")))?;
        let images_json = serde_json::to_string(&input.images)
            .map_err(|e| RepoError::Backend(format!("images_json serialize: {e}")))?;

        let sub_guid = input.category_job_service_sub_guid.as_deref();
        let main_guid = input.category_job_service_main_guid.as_str();
        let name_la = input.category_job_service_sub_name_la.as_deref();
        let name_en = input.category_job_service_sub_name_en.as_deref();
        let name_th = input.category_job_service_sub_name_th.as_deref();
        let name_zh = input.category_job_service_sub_name_zh.as_deref();
        let start_price = input.category_job_service_sub_start_price;
        let desc_la = input.category_job_service_sub_description_la.as_deref();
        let desc_en = input.category_job_service_sub_description_en.as_deref();
        let desc_th = input.category_job_service_sub_description_th.as_deref();
        let desc_zh = input.category_job_service_sub_description_zh.as_deref();
        let status = input.category_job_service_sub_status;
        let warranties_json_str = warranties_json.as_str();
        let fees_json_str = fees_json.as_str();
        let images_json_str = images_json.as_str();
        let create_by = input.create_by.as_str();

        let params: &[&dyn ToSql] = &[
            &sub_guid,
            &main_guid,
            &name_la,
            &name_en,
            &name_th,
            &name_zh,
            &start_price,
            &desc_la,
            &desc_en,
            &desc_th,
            &desc_zh,
            &status,
            &warranties_json_str,
            &fees_json_str,
            &images_json_str,
            &create_by,
        ];

        let rows = exec_sp(
            &self.pool,
            "EXEC dbo.SP_CATEGORY_JOB_SERVICE_SUB_INSERT \
                @p_category_job_service_sub_guid = @P1, \
                @p_category_job_service_main_guid = @P2, \
                @p_category_job_service_sub_name_la = @P3, \
                @p_category_job_service_sub_name_en = @P4, \
                @p_category_job_service_sub_name_th = @P5, \
                @p_category_job_service_sub_name_zh = @P6, \
                @p_category_job_service_sub_start_price = @P7, \
                @p_category_job_service_sub_description_la = @P8, \
                @p_category_job_service_sub_description_en = @P9, \
                @p_category_job_service_sub_description_th = @P10, \
                @p_category_job_service_sub_description_zh = @P11, \
                @p_category_job_service_sub_status = @P12, \
                @p_warranties_json = @P13, \
                @p_fees_json = @P14, \
                @p_images_json = @P15, \
                @p_create_by = @P16",
            params,
        )
        .await?;

        let row = rows.first().ok_or_else(|| {
            RepoError::Backend("SP_CATEGORY_JOB_SERVICE_SUB_INSERT returned no row".into())
        })?;

        let success: bool = row.get::<bool, _>("success").unwrap_or(false);
        let code = read_str(row, "code").unwrap_or("").to_string();
        let message = read_str(row, "message").unwrap_or("").to_string();

        if !success {
            return Err(RepoError::Backend(format!(
                "SP_CATEGORY_JOB_SERVICE_SUB_INSERT: {code} — {message}"
            )));
        }

        Ok(CategoryJobServiceSubCreateSpResult {
            success,
            code,
            message,
            category_job_service_sub_guid: {
                let s = read_guid_str(row, "category_job_service_sub_guid");
                if s.is_empty() {
                    None
                } else {
                    Some(s)
                }
            },
            warranty_count: read_i32(row, "warranty_count").unwrap_or(0),
            fee_count: read_i32(row, "fee_count").unwrap_or(0),
            image_count: read_i32(row, "image_count").unwrap_or(0),
        })
    }

    async fn update_via_sp(
        &self,
        input: &CategoryJobServiceSubUpdateSpInput,
    ) -> Result<CategoryJobServiceSubUpdateSpResult, RepoError> {
        let warranties_json = serde_json::to_string(&input.warranties)
            .map_err(|e| RepoError::Backend(format!("warranties_json serialize: {e}")))?;
        let fees_json = serde_json::to_string(&input.fees)
            .map_err(|e| RepoError::Backend(format!("fees_json serialize: {e}")))?;
        let images_json = serde_json::to_string(&input.images)
            .map_err(|e| RepoError::Backend(format!("images_json serialize: {e}")))?;

        let sub_guid = input.category_job_service_sub_guid.as_str();
        let main_guid = input.category_job_service_main_guid.as_deref();
        let name_la = input.category_job_service_sub_name_la.as_deref();
        let name_en = input.category_job_service_sub_name_en.as_deref();
        let name_th = input.category_job_service_sub_name_th.as_deref();
        let name_zh = input.category_job_service_sub_name_zh.as_deref();
        let start_price = input.category_job_service_sub_start_price;
        let desc_la = input.category_job_service_sub_description_la.as_deref();
        let desc_en = input.category_job_service_sub_description_en.as_deref();
        let desc_th = input.category_job_service_sub_description_th.as_deref();
        let desc_zh = input.category_job_service_sub_description_zh.as_deref();
        let status = input.category_job_service_sub_status;
        let warranties_json_str = warranties_json.as_str();
        let fees_json_str = fees_json.as_str();
        let images_json_str = images_json.as_str();
        let replace_images = input.replace_images;
        let update_by = input.update_by.as_str();

        let params: &[&dyn ToSql] = &[
            &sub_guid,
            &main_guid,
            &name_la,
            &name_en,
            &name_th,
            &name_zh,
            &start_price,
            &desc_la,
            &desc_en,
            &desc_th,
            &desc_zh,
            &status,
            &warranties_json_str,
            &fees_json_str,
            &images_json_str,
            &replace_images,
            &update_by,
        ];

        let rows = exec_sp(
            &self.pool,
            "EXEC dbo.SP_CATEGORY_JOB_SERVICE_SUB_UPDATE \
                @p_category_job_service_sub_guid = @P1, \
                @p_category_job_service_main_guid = @P2, \
                @p_category_job_service_sub_name_la = @P3, \
                @p_category_job_service_sub_name_en = @P4, \
                @p_category_job_service_sub_name_th = @P5, \
                @p_category_job_service_sub_name_zh = @P6, \
                @p_category_job_service_sub_start_price = @P7, \
                @p_category_job_service_sub_description_la = @P8, \
                @p_category_job_service_sub_description_en = @P9, \
                @p_category_job_service_sub_description_th = @P10, \
                @p_category_job_service_sub_description_zh = @P11, \
                @p_category_job_service_sub_status = @P12, \
                @p_warranties_json = @P13, \
                @p_fees_json = @P14, \
                @p_images_json = @P15, \
                @p_replace_images = @P16, \
                @p_update_by = @P17",
            params,
        )
        .await?;

        let row = rows.first().ok_or_else(|| {
            RepoError::Backend("SP_CATEGORY_JOB_SERVICE_SUB_UPDATE returned no row".into())
        })?;

        let success: bool = row.get::<bool, _>("success").unwrap_or(false);
        let code = read_str(row, "code").unwrap_or("").to_string();
        let message = read_str(row, "message").unwrap_or("").to_string();

        if !success {
            return Err(RepoError::Backend(format!(
                "SP_CATEGORY_JOB_SERVICE_SUB_UPDATE: {code} — {message}"
            )));
        }

        Ok(CategoryJobServiceSubUpdateSpResult {
            success,
            code,
            message,
            category_job_service_sub_guid: {
                let s = read_guid_str(row, "category_job_service_sub_guid");
                if s.is_empty() {
                    input.category_job_service_sub_guid.clone()
                } else {
                    s
                }
            },
            warranty_count: read_i32(row, "warranty_count").unwrap_or(0),
            fee_count: read_i32(row, "fee_count").unwrap_or(0),
            image_count: read_i32(row, "image_count").unwrap_or(0),
        })
    }
}

impl MssqlCategoryJobServiceSubRepository {
    async fn create_in_tx(
        &self,
        conn: &mut crate::db::mssql::MssqlClient,
        input: &CategoryJobServiceSubCreateInput,
    ) -> Result<CategoryJobServiceSubCreateResult, RepoError> {
        let main_guid = input.category_job_service_guid.as_str();
        let name = input.category_job_service_sub_name.as_str();
        let start_price = input.category_job_service_sub_start_price;
        let description = input.category_job_service_sub_description.as_str();
        let create_by = input.create_by.as_str();

        let params: &[&dyn ToSql] = &[&main_guid, &name, &start_price, &description, &create_by];

        let rows = exec_sp_on(
            conn,
            "EXEC dbo.SP_CATEGORY_JOB_SERVICE_SUB_CREATE \
                @p_category_job_service_guid = @P1, \
                @p_category_job_service_sub_name = @P2, \
                @p_category_job_service_sub_start_price = @P3, \
                @p_category_job_service_sub_description = @P4, \
                @p_create_by = @P5",
            params,
        )
        .await?;

        let row = rows.first().ok_or_else(|| {
            RepoError::Backend(
                "SP_CATEGORY_JOB_SERVICE_SUB_CREATE returned no row (driver/protocol mismatch)"
                    .into(),
            )
        })?;

        let success: bool = row.get::<bool, _>("success").unwrap_or(false);
        let code = read_str(row, "code").unwrap_or("").to_string();
        let message = read_str(row, "message").unwrap_or("").to_string();

        if !success {
            return Err(RepoError::Backend(format!(
                "{}: {code} — {message}",
                CategoryJobServiceSubError::CODE_SUCCESS
            )));
        }

        Ok(CategoryJobServiceSubCreateResult {
            success,
            code,
            message,
            category_job_service_sub_guid: {
                let s = read_guid_str(row, "category_job_service_sub_guid");
                if s.is_empty() {
                    None
                } else {
                    Some(s)
                }
            },
        })
    }

    async fn update_in_tx(
        &self,
        conn: &mut crate::db::mssql::MssqlClient,
        input: &CategoryJobServiceSubUpdateInput,
    ) -> Result<CategoryJobServiceSubUpdateResult, RepoError> {
        let sub_guid = input.category_job_service_sub_guid.as_str();
        let main_guid = input.category_job_service_guid.as_str();
        let name = input.category_job_service_sub_name.as_str();
        let start_price = input.category_job_service_sub_start_price;
        let description = input.category_job_service_sub_description.as_str();
        let status = input.category_job_service_sub_status;
        let update_by = input.update_by.as_str();

        let params: &[&dyn ToSql] = &[
            &sub_guid,
            &main_guid,
            &name,
            &start_price,
            &description,
            &status,
            &update_by,
        ];

        let rows = exec_sp_on(
            conn,
            "EXEC dbo.SP_CATEGORY_JOB_SERVICE_SUB_UPDATE \
                @p_category_job_service_sub_guid = @P1, \
                @p_category_job_service_guid = @P2, \
                @p_category_job_service_sub_name = @P3, \
                @p_category_job_service_sub_start_price = @P4, \
                @p_category_job_service_sub_description = @P5, \
                @p_category_job_service_sub_status = @P6, \
                @p_update_by = @P7",
            params,
        )
        .await?;

        let row = rows.first().ok_or_else(|| {
            RepoError::Backend(
                "SP_CATEGORY_JOB_SERVICE_SUB_UPDATE returned no row (driver/protocol mismatch)"
                    .into(),
            )
        })?;

        let success: bool = row.get::<bool, _>("success").unwrap_or(false);
        let code = read_str(row, "code").unwrap_or("").to_string();
        let message = read_str(row, "message").unwrap_or("").to_string();

        if !success {
            return Err(RepoError::Backend(format!(
                "{}: {code} — {message}",
                CategoryJobServiceSubError::CODE_SUCCESS
            )));
        }

        let returned_guid = read_guid_str(row, "category_job_service_sub_guid");
        let final_guid = if returned_guid.is_empty() {
            input.category_job_service_sub_guid.clone()
        } else {
            returned_guid
        };

        Ok(CategoryJobServiceSubUpdateResult {
            success,
            code,
            message,
            category_job_service_sub_guid: final_guid,
        })
    }

    async fn create_image_in_tx(
        &self,
        conn: &mut crate::db::mssql::MssqlClient,
        input: &CategoryJobServiceSubImageCreateInput,
    ) -> Result<CategoryJobServiceSubImageCreateResult, RepoError> {
        let sub_guid = input.category_job_service_sub_guid.as_str();
        let img_type = input.img_type;
        let img_priority = input.img_priority;
        let img_path = input.img_path.as_str();
        let create_by = input.create_by.as_str();

        let params: &[&dyn ToSql] = &[&sub_guid, &img_type, &img_priority, &img_path, &create_by];

        let rows = exec_sp_on(
            conn,
            "EXEC dbo.SP_CATEGORY_JOB_SERVICE_SUB_IMG_CREATE \
                @p_category_job_service_sub_guid = @P1, \
                @p_img_type = @P2, \
                @p_img_priority = @P3, \
                @p_img_path = @P4, \
                @p_create_by = @P5",
            params,
        )
        .await?;

        let row = rows.first().ok_or_else(|| {
            RepoError::Backend(
                "SP_CATEGORY_JOB_SERVICE_SUB_IMG_CREATE returned no row (driver/protocol mismatch)"
                    .into(),
            )
        })?;

        let success: bool = row.get::<bool, _>("success").unwrap_or(false);
        let code = read_str(row, "code").unwrap_or("").to_string();
        let message = read_str(row, "message").unwrap_or("").to_string();

        if !success {
            return Err(RepoError::Backend(format!(
                "{}: {code} — {message}",
                CategoryJobServiceSubError::CODE_SUCCESS
            )));
        }

        Ok(CategoryJobServiceSubImageCreateResult {
            success,
            code,
            message,
            category_job_service_sub_img_guid: {
                let s = read_guid_str(row, "category_job_service_sub_img_guid");
                if s.is_empty() {
                    None
                } else {
                    Some(s)
                }
            },
        })
    }
}

fn read_decimal(row: &tiberius::Row, col: &str) -> Decimal {
    row.get::<Decimal, _>(col).unwrap_or(Decimal::ZERO)
}

fn read_datetime_utc(row: &tiberius::Row, col: &str) -> Option<DateTime<Utc>> {
    row.get::<chrono::NaiveDateTime, _>(col)
        .map(|ndt| ndt.and_utc())
}

fn row_to_sub_row(row: &tiberius::Row) -> CategoryJobServiceSubRow {
    CategoryJobServiceSubRow {
        category_job_service_sub_guid: read_guid_str(row, "category_job_service_sub_guid"),
        category_job_service_sub_category_job_service_main_guid: read_guid_str(
            row,
            "category_job_service_sub_category_job_service_main_guid",
        ),
        category_job_service_sub_category_job_service_sub_fee_guid: read_guid_str(
            row,
            "category_job_service_sub_category_job_service_sub_fee_guid",
        ),
        category_job_service_sub_category_job_service_sub_warranty_guid: read_guid_str(
            row,
            "category_job_service_sub_category_job_service_sub_warranty_guid",
        ),
        category_job_service_name: read_str(row, "category_job_service_name")
            .unwrap_or("")
            .to_string(),
        category_job_service_sub_name: read_str(row, "category_job_service_sub_name")
            .unwrap_or("")
            .to_string(),
        category_job_service_sub_locale: read_str(row, "category_job_service_sub_locale")
            .unwrap_or("")
            .to_string(),
        category_job_service_sub_start_price: read_decimal(
            row,
            "category_job_service_sub_start_price",
        ),
        category_job_service_sub_description: read_str(row, "category_job_service_sub_description")
            .unwrap_or("")
            .to_string(),
        category_job_service_sub_status: read_i32(row, "category_job_service_sub_status")
            .unwrap_or(0),
        main_img_path: read_str(row, "main_img_path").unwrap_or("").to_string(),
        main_img_url: None,
        category_job_service_sub_create_at: read_datetime_utc(
            row,
            "category_job_service_sub_create_at",
        ),
        category_job_service_sub_create_by: read_str(row, "category_job_service_sub_create_by")
            .unwrap_or("")
            .to_string(),
        category_job_service_sub_update_at: read_datetime_utc(
            row,
            "category_job_service_sub_update_at",
        ),
        category_job_service_sub_update_by: read_str(row, "category_job_service_sub_update_by")
            .unwrap_or("")
            .to_string(),
    }
}

fn row_to_sub_image_row(row: &tiberius::Row) -> CategoryJobServiceSubImageRow {
    CategoryJobServiceSubImageRow {
        category_job_service_sub_img_guid: read_guid_str(row, "category_job_service_sub_img_guid"),
        category_job_service_sub_img_category_job_service_sub_guid: read_guid_str(
            row,
            "category_job_service_sub_img_category_job_service_sub_guid",
        ),
        category_job_service_sub_img_type: read_i32(row, "category_job_service_sub_img_type")
            .unwrap_or(0),
        category_job_service_sub_img_type_language: read_i32(
            row,
            "category_job_service_sub_img_type_language",
        )
        .unwrap_or(0),
        category_job_service_sub_img_priority: read_i32(
            row,
            "category_job_service_sub_img_priority",
        )
        .unwrap_or(0),
        category_job_service_sub_img_path: read_str(row, "category_job_service_sub_img_path")
            .unwrap_or("")
            .to_string(),
        category_job_service_sub_img_url: None,
        category_job_service_sub_img_status: read_i32(row, "category_job_service_sub_img_status")
            .unwrap_or(0),
        category_job_service_sub_img_create_at: read_datetime_utc(
            row,
            "category_job_service_sub_img_create_at",
        ),
        category_job_service_sub_img_create_by: read_str(
            row,
            "category_job_service_sub_img_create_by",
        )
        .unwrap_or("")
        .to_string(),
    }
}

#[allow(dead_code)]
fn row_to_sub_fee_row(row: &tiberius::Row) -> CategoryJobServiceSubFeeRow {
    CategoryJobServiceSubFeeRow {
        category_job_service_sub_fee_guid: read_guid_str(row, "category_job_service_sub_fee_guid"),
        category_job_service_sub_fee_category_job_service_sub_guid: read_guid_str(
            row,
            "category_job_service_sub_fee_category_job_service_sub_guid",
        ),
        category_job_service_sub_fee_name: read_str(row, "category_job_service_sub_fee_name")
            .unwrap_or("")
            .to_string(),
        category_job_service_sub_fee_price: read_decimal(row, "category_job_service_sub_fee_price"),
        category_job_service_sub_fee_status: read_i32(row, "category_job_service_sub_fee_status")
            .unwrap_or(0),
    }
}

#[allow(dead_code)]
fn row_to_sub_warranty_row(row: &tiberius::Row) -> CategoryJobServiceSubWarrantyRow {
    CategoryJobServiceSubWarrantyRow {
        category_job_service_sub_warranty_guid: read_guid_str(
            row,
            "category_job_service_sub_warranty_guid",
        ),
        category_job_service_sub_warranty_category_job_service_sub_guid: read_guid_str(
            row,
            "category_job_service_sub_warranty_category_job_service_sub_guid",
        ),
        category_job_service_sub_warranty_name: read_str(
            row,
            "category_job_service_sub_warranty_name",
        )
        .unwrap_or("")
        .to_string(),
        category_job_service_sub_warranty_day: read_i32(
            row,
            "category_job_service_sub_warranty_day",
        )
        .unwrap_or(0),
        category_job_service_sub_warranty_status: read_i32(
            row,
            "category_job_service_sub_warranty_status",
        )
        .unwrap_or(0),
    }
}

fn row_to_sub_detail_row(row: &tiberius::Row) -> CategoryJobServiceSubDetailRow {
    CategoryJobServiceSubDetailRow {
        category_job_service_guid: read_guid_str(row, "category_job_service_guid"),
        category_job_service_sub_guid: read_guid_str(row, "category_job_service_sub_guid"),
        category_job_service_sub_category_job_service_main_guid: read_guid_str(
            row,
            "category_job_service_sub_category_job_service_main_guid",
        ),
        category_job_service_sub_name_la: read_str(row, "category_job_service_sub_name_la")
            .unwrap_or("")
            .to_string(),
        category_job_service_sub_name_en: read_str(row, "category_job_service_sub_name_en")
            .unwrap_or("")
            .to_string(),
        category_job_service_sub_name_th: read_str(row, "category_job_service_sub_name_th")
            .unwrap_or("")
            .to_string(),
        category_job_service_sub_name_zh: read_str(row, "category_job_service_sub_name_zh")
            .unwrap_or("")
            .to_string(),
        category_job_service_sub_description_la: read_str(
            row,
            "category_job_service_sub_description_la",
        )
        .unwrap_or("")
        .to_string(),
        category_job_service_sub_description_en: read_str(
            row,
            "category_job_service_sub_description_en",
        )
        .unwrap_or("")
        .to_string(),
        category_job_service_sub_description_th: read_str(
            row,
            "category_job_service_sub_description_th",
        )
        .unwrap_or("")
        .to_string(),
        category_job_service_sub_description_zh: read_str(
            row,
            "category_job_service_sub_description_zh",
        )
        .unwrap_or("")
        .to_string(),
        category_job_service_sub_start_price: read_decimal(
            row,
            "category_job_service_sub_start_price",
        ),
        category_job_service_sub_status: read_i32(row, "category_job_service_sub_status")
            .unwrap_or(0),
        category_job_service_sub_create_at: read_datetime_utc(
            row,
            "category_job_service_sub_create_at",
        ),
        category_job_service_sub_create_by: read_str(row, "category_job_service_sub_create_by")
            .unwrap_or("")
            .to_string(),
        category_job_service_sub_update_at: read_datetime_utc(
            row,
            "category_job_service_sub_update_at",
        ),
        category_job_service_sub_update_by: read_str(row, "category_job_service_sub_update_by")
            .unwrap_or("")
            .to_string(),
    }
}

fn row_to_sub_detail_warranty_row(row: &tiberius::Row) -> CategoryJobServiceSubDetailWarrantyRow {
    CategoryJobServiceSubDetailWarrantyRow {
        category_job_service_sub_warranty_guid: read_guid_str(
            row,
            "category_job_service_sub_warranty_guid",
        ),
        category_job_service_sub_warranty_map_sort_order: read_i32(
            row,
            "category_job_service_sub_warranty_map_sort_order",
        )
        .unwrap_or(0),
        category_job_service_sub_warranty_description: read_str(
            row,
            "category_job_service_sub_warranty_description",
        )
        .unwrap_or("")
        .to_string(),
        category_job_service_sub_warranty_locale: read_str(
            row,
            "category_job_service_sub_warranty_locale",
        )
        .unwrap_or("la")
        .to_string(),
        category_job_service_sub_warranty_warranty_amount_day: read_i32(
            row,
            "category_job_service_sub_warranty_warranty_amount_day",
        )
        .unwrap_or(0),
        category_job_service_sub_warranty_icon: read_str(
            row,
            "category_job_service_sub_warranty_icon",
        )
        .unwrap_or("")
        .to_string(),
        category_job_service_sub_warranty_status: read_i32(
            row,
            "category_job_service_sub_warranty_status",
        )
        .unwrap_or(0),
        category_job_service_sub_warranty_map_create_at: read_datetime_utc(
            row,
            "category_job_service_sub_warranty_map_create_at",
        ),
        category_job_service_sub_warranty_map_create_by: read_str(
            row,
            "category_job_service_sub_warranty_map_create_by",
        )
        .unwrap_or("")
        .to_string(),
        category_job_service_sub_warranty_map_update_at: read_datetime_utc(
            row,
            "category_job_service_sub_warranty_map_update_at",
        ),
        category_job_service_sub_warranty_map_update_by: read_str(
            row,
            "category_job_service_sub_warranty_map_update_by",
        )
        .unwrap_or("")
        .to_string(),
    }
}

fn row_to_sub_detail_fee_row(row: &tiberius::Row) -> CategoryJobServiceSubDetailFeeRow {
    CategoryJobServiceSubDetailFeeRow {
        category_job_service_sub_fee_guid: read_guid_str(row, "category_job_service_sub_fee_guid"),
        category_job_service_sub_fee_map_sort_order: read_i32(
            row,
            "category_job_service_sub_fee_map_sort_order",
        )
        .unwrap_or(0),
        category_job_service_sub_fee_header: read_str(row, "category_job_service_sub_fee_header")
            .unwrap_or("")
            .to_string(),
        category_job_service_sub_fee_description: read_str(
            row,
            "category_job_service_sub_fee_description",
        )
        .unwrap_or("")
        .to_string(),
        category_job_service_sub_fee_locale: read_str(row, "category_job_service_sub_fee_locale")
            .unwrap_or("la")
            .to_string(),
        category_job_service_sub_fee_icon: read_str(row, "category_job_service_sub_fee_icon")
            .unwrap_or("")
            .to_string(),
        category_job_service_sub_fee_price: read_decimal(row, "category_job_service_sub_fee_price"),
        category_job_service_sub_fee_status: read_i32(row, "category_job_service_sub_fee_status")
            .unwrap_or(0),
        category_job_service_sub_fee_map_create_at: read_datetime_utc(
            row,
            "category_job_service_sub_fee_map_create_at",
        ),
        category_job_service_sub_fee_map_create_by: read_str(
            row,
            "category_job_service_sub_fee_map_create_by",
        )
        .unwrap_or("")
        .to_string(),
        category_job_service_sub_fee_map_update_at: read_datetime_utc(
            row,
            "category_job_service_sub_fee_map_update_at",
        ),
        category_job_service_sub_fee_map_update_by: read_str(
            row,
            "category_job_service_sub_fee_map_update_by",
        )
        .unwrap_or("")
        .to_string(),
    }
}

fn row_to_sub_detail_image_row(row: &tiberius::Row) -> CategoryJobServiceSubDetailImageRow {
    CategoryJobServiceSubDetailImageRow {
        category_job_service_sub_img_guid: read_guid_str(row, "category_job_service_sub_img_guid"),
        category_job_service_sub_img_category_job_service_sub_guid: read_guid_str(
            row,
            "category_job_service_sub_img_category_job_service_sub_guid",
        ),
        category_job_service_sub_img_type: read_i32(row, "category_job_service_sub_img_type")
            .unwrap_or(0),
        category_job_service_sub_img_type_language: read_i32(
            row,
            "category_job_service_sub_img_type_language",
        )
        .unwrap_or(0),
        category_job_service_sub_img_priority: read_i32(
            row,
            "category_job_service_sub_img_priority",
        )
        .unwrap_or(0),
        category_job_service_sub_img_img_path: read_str(
            row,
            "category_job_service_sub_img_img_path",
        )
        .unwrap_or("")
        .to_string(),
        category_job_service_sub_img_url: None,
        category_job_service_sub_img_status: read_i32(row, "category_job_service_sub_img_status")
            .unwrap_or(0),
        category_job_service_sub_img_create_at: read_datetime_utc(
            row,
            "category_job_service_sub_img_create_at",
        ),
        category_job_service_sub_img_create_by: read_str(
            row,
            "category_job_service_sub_img_create_by",
        )
        .unwrap_or("")
        .to_string(),
        category_job_service_sub_img_update_at: read_datetime_utc(
            row,
            "category_job_service_sub_img_update_at",
        ),
        category_job_service_sub_img_update_by: read_str(
            row,
            "category_job_service_sub_img_update_by",
        )
        .unwrap_or("")
        .to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn row_parsing_safe_when_columns_missing() {
        let row = CategoryJobServiceSubRow {
            category_job_service_sub_guid: "s-1".into(),
            category_job_service_sub_category_job_service_main_guid: "m-1".into(),
            category_job_service_sub_category_job_service_sub_fee_guid: "fee-1".into(),
            category_job_service_sub_category_job_service_sub_warranty_guid: "w-1".into(),
            category_job_service_name: "Air Con".into(),
            category_job_service_sub_name: "ล้างแอร์ 9000-12000 BTU".into(),
            category_job_service_sub_locale: "la".into(),
            category_job_service_sub_start_price: Decimal::new(900000, 2),
            category_job_service_sub_description: "ล้างแอร์ตามมาตรฐาน".into(),
            category_job_service_sub_status: 1,
            main_img_path: "category-job-service-subs/s-1/image/x.webp".into(),
            main_img_url: None,
            category_job_service_sub_create_at: Some(Utc::now()),
            category_job_service_sub_create_by: "admin".into(),
            category_job_service_sub_update_at: Some(Utc::now()),
            category_job_service_sub_update_by: "admin".into(),
        };
        assert_eq!(row.category_job_service_sub_status, 1);
        assert_eq!(
            row.category_job_service_sub_start_price,
            Decimal::new(900000, 2)
        );
        assert_eq!(row.category_job_service_sub_locale, "la");
        assert_eq!(
            row.category_job_service_sub_category_job_service_sub_fee_guid,
            "fee-1"
        );
        assert_eq!(
            row.category_job_service_sub_category_job_service_sub_warranty_guid,
            "w-1"
        );
    }
}
