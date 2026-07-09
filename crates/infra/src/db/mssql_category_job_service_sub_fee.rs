use async_trait::async_trait;
use chrono::{DateTime, Utc};
use tiberius::ToSql;

use kokkak_domain::traits::category_job_service_sub_fee::CategoryJobServiceSubFeeRepository;
use kokkak_domain::traits::user::RepoError;
use kokkak_domain::{
    CategoryJobServiceSubFeeAdminRow, CategoryJobServiceSubFeeAutocompleteInput,
    CategoryJobServiceSubFeeAutocompleteRow, CategoryJobServiceSubFeeCreateInput,
    CategoryJobServiceSubFeeCreateResult, CategoryJobServiceSubFeeDeleteInput,
    CategoryJobServiceSubFeeDeleteResult, CategoryJobServiceSubFeeListInput,
    CategoryJobServiceSubFeePage, CategoryJobServiceSubFeeUpdateInput,
    CategoryJobServiceSubFeeUpdateResult,
};

use crate::db::mssql::{exec_sp, read_i32, read_str, MssqlPool};

#[derive(Clone)]
pub struct MssqlCategoryJobServiceSubFeeRepository {
    pool: MssqlPool,
}

impl MssqlCategoryJobServiceSubFeeRepository {
    pub fn new(pool: MssqlPool) -> Self {
        Self { pool }
    }

    pub fn disabled() -> Self {
        Self {
            pool: crate::db::mssql::build_disabled_pool(),
        }
    }
}

fn normalize_locale(raw: Option<&str>) -> String {
    let s = raw.unwrap_or("").trim().to_ascii_lowercase();
    let primary = s.split('-').next().unwrap_or("");
    if matches!(primary, "la" | "lo" | "en" | "th" | "zh") {
        return if primary == "lo" {
            "la".to_string()
        } else {
            primary.to_string()
        };
    }
    "la".to_string()
}

fn row_to_fee_autocomplete_row(row: &tiberius::Row) -> CategoryJobServiceSubFeeAutocompleteRow {
    let guid = read_str(row, "category_job_service_sub_fee_guid")
        .unwrap_or("")
        .to_string();
    let header = read_str(row, "category_job_service_sub_fee_header")
        .unwrap_or("")
        .to_string();
    let value = read_str(row, "value").unwrap_or("").to_string();
    let label = read_str(row, "label").unwrap_or("").to_string();
    CategoryJobServiceSubFeeAutocompleteRow {
        category_job_service_sub_fee_guid: guid,
        category_job_service_sub_fee_header: header.clone(),
        category_job_service_sub_fee_description: read_str(
            row,
            "category_job_service_sub_fee_description",
        )
        .unwrap_or("")
        .to_string(),
        category_job_service_sub_fee_price: row
            .get::<rust_decimal::Decimal, _>("category_job_service_sub_fee_price")
            .unwrap_or(rust_decimal::Decimal::ZERO),
        category_job_service_sub_fee_status: read_i32(row, "category_job_service_sub_fee_status")
            .unwrap_or(0),
        category_job_service_sub_fee_icon: read_str(row, "category_job_service_sub_fee_icon")
            .unwrap_or("")
            .to_string(),
        value: if value.is_empty() {
            read_str(row, "category_job_service_sub_fee_guid")
                .unwrap_or("")
                .to_string()
        } else {
            value
        },
        label: if label.is_empty() { header } else { label },
    }
}

fn row_to_fee_row(row: &tiberius::Row) -> CategoryJobServiceSubFeeAdminRow {
    CategoryJobServiceSubFeeAdminRow {
        category_job_service_sub_fee_guid: read_str(row, "category_job_service_sub_fee_guid")
            .unwrap_or("")
            .to_string(),
        category_job_service_sub_fee_header: read_str(row, "category_job_service_sub_fee_header")
            .unwrap_or("")
            .to_string(),
        category_job_service_sub_fee_description: read_str(
            row,
            "category_job_service_sub_fee_description",
        )
        .unwrap_or("")
        .to_string(),
        category_job_service_sub_fee_price: row
            .get::<rust_decimal::Decimal, _>("category_job_service_sub_fee_price")
            .unwrap_or(rust_decimal::Decimal::ZERO),
        category_job_service_sub_fee_status: read_i32(row, "category_job_service_sub_fee_status")
            .unwrap_or(0),
        category_job_service_sub_fee_icon: read_str(row, "category_job_service_sub_fee_icon")
            .unwrap_or("")
            .to_string(),
        category_job_service_sub_fee_create_at: {
            row.get::<DateTime<Utc>, _>("category_job_service_sub_fee_create_at")
        },
        category_job_service_sub_fee_create_by: read_str(
            row,
            "category_job_service_sub_fee_create_by",
        )
        .unwrap_or("")
        .to_string(),
        category_job_service_sub_fee_update_at: {
            row.get::<DateTime<Utc>, _>("category_job_service_sub_fee_update_at")
        },
        category_job_service_sub_fee_update_by: read_str(
            row,
            "category_job_service_sub_fee_update_by",
        )
        .unwrap_or("")
        .to_string(),
    }
}

#[async_trait]
impl CategoryJobServiceSubFeeRepository for MssqlCategoryJobServiceSubFeeRepository {
    async fn list(
        &self,
        input: &CategoryJobServiceSubFeeListInput,
    ) -> Result<CategoryJobServiceSubFeePage, RepoError> {
        let guid = input
            .category_job_service_sub_fee_guid
            .as_deref()
            .unwrap_or("");
        let keyword = input.keyword.clone();
        let kw_ref: Option<&str> = keyword.as_deref();
        let status = input.status;
        let locale_param: String = normalize_locale(input.locale.as_deref());
        let include_deleted = input.include_deleted.unwrap_or(false);
        let page_in: u32 = input.page.unwrap_or(1).max(1);
        let page_size_in: u32 = input.page_size.unwrap_or(20).clamp(1, 100);
        let page: i32 = page_in as i32;
        let page_size: i32 = page_size_in as i32;

        let params: &[&dyn ToSql] = &[
            &guid as &dyn ToSql,
            &kw_ref as &dyn ToSql,
            &status as &dyn ToSql,
            &locale_param as &dyn ToSql,
            &include_deleted as &dyn ToSql,
            &page as &dyn ToSql,
            &page_size as &dyn ToSql,
        ];

        let rows = exec_sp(
            &self.pool,
            "EXEC dbo.SP_CATEGORY_JOB_SERVICE_SUB_FEE_GET \
                @p_category_job_service_sub_fee_guid = @P1, \
                @p_keyword = @P2, \
                @p_status = @P3, \
                @p_locale = @P4, \
                @p_include_deleted = @P5, \
                @p_page = @P6, \
                @p_page_size = @P7",
            params,
        )
        .await?;

        if rows.is_empty() {
            return Ok(CategoryJobServiceSubFeePage {
                items: Vec::new(),
                total_count: 0,
                page: page_in,
                page_size: page_size_in,
                total_page: 0,
            });
        }

        let first = &rows[0];
        let total_count: i64 = read_i32(first, "total_count")
            .map(|v| v as i64)
            .unwrap_or(0);
        let out_page: u32 = read_i32(first, "page")
            .map(|v| v.max(0) as u32)
            .unwrap_or(page_in);
        let out_page_size: u32 = read_i32(first, "page_size")
            .map(|v| v.max(0) as u32)
            .unwrap_or(page_size_in);
        let total_page: u32 = if out_page_size == 0 {
            0
        } else {
            (total_count as u64).div_ceil(out_page_size as u64) as u32
        };

        let items: Vec<CategoryJobServiceSubFeeAdminRow> =
            rows.iter().map(row_to_fee_row).collect();

        Ok(CategoryJobServiceSubFeePage {
            items,
            total_count,
            page: out_page,
            page_size: out_page_size,
            total_page,
        })
    }

    async fn create(
        &self,
        input: &CategoryJobServiceSubFeeCreateInput,
    ) -> Result<CategoryJobServiceSubFeeCreateResult, RepoError> {
        let guid = input
            .category_job_service_sub_fee_guid
            .as_deref()
            .unwrap_or("");
        let header_la = input.category_job_service_sub_fee_header_la.as_deref();
        let desc_la = input.category_job_service_sub_fee_description_la.as_deref();
        let header_en = input.category_job_service_sub_fee_header_en.as_deref();
        let desc_en = input.category_job_service_sub_fee_description_en.as_deref();
        let header_th = input.category_job_service_sub_fee_header_th.as_deref();
        let desc_th = input.category_job_service_sub_fee_description_th.as_deref();
        let header_zh = input.category_job_service_sub_fee_header_zh.as_deref();
        let desc_zh = input.category_job_service_sub_fee_description_zh.as_deref();
        let price: rust_decimal::Decimal = input.category_job_service_sub_fee_price;
        let status: i32 = input.category_job_service_sub_fee_status;
        let icon = input.category_job_service_sub_fee_icon.as_deref();
        let create_by: &str = input.create_by.as_str();

        let params: &[&dyn ToSql] = &[
            &guid as &dyn ToSql,
            &header_la as &dyn ToSql,
            &desc_la as &dyn ToSql,
            &header_en as &dyn ToSql,
            &desc_en as &dyn ToSql,
            &header_th as &dyn ToSql,
            &desc_th as &dyn ToSql,
            &header_zh as &dyn ToSql,
            &desc_zh as &dyn ToSql,
            &price as &dyn ToSql,
            &status as &dyn ToSql,
            &icon as &dyn ToSql,
            &create_by as &dyn ToSql,
        ];

        let rows = exec_sp(
            &self.pool,
            "EXEC dbo.SP_CATEGORY_JOB_SERVICE_SUB_FEE_INSERT \
                @p_category_job_service_sub_fee_guid = @P1, \
                @p_category_job_service_sub_fee_header_la = @P2, \
                @p_category_job_service_sub_fee_description_la = @P3, \
                @p_category_job_service_sub_fee_header_en = @P4, \
                @p_category_job_service_sub_fee_description_en = @P5, \
                @p_category_job_service_sub_fee_header_th = @P6, \
                @p_category_job_service_sub_fee_description_th = @P7, \
                @p_category_job_service_sub_fee_header_zh = @P8, \
                @p_category_job_service_sub_fee_description_zh = @P9, \
                @p_category_job_service_sub_fee_price = @P10, \
                @p_category_job_service_sub_fee_status = @P11, \
                @p_category_job_service_sub_fee_icon = @P12, \
                @p_create_by = @P13",
            params,
        )
        .await?;

        let row = rows.first().ok_or_else(|| {
            RepoError::Backend(
                "SP_CATEGORY_JOB_SERVICE_SUB_FEE_INSERT returned no result row".into(),
            )
        })?;

        let success: bool = row.get::<bool, _>("success").unwrap_or(false);
        let code = read_str(row, "code").unwrap_or("").to_string();
        let message = read_str(row, "message").unwrap_or("").to_string();
        let out_guid = read_str(row, "category_job_service_sub_fee_guid")
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());

        Ok(CategoryJobServiceSubFeeCreateResult {
            success,
            code,
            message,
            category_job_service_sub_fee_guid: out_guid,
        })
    }

    async fn update(
        &self,
        input: &CategoryJobServiceSubFeeUpdateInput,
    ) -> Result<CategoryJobServiceSubFeeUpdateResult, RepoError> {
        let guid = input.category_job_service_sub_fee_guid.as_str();
        let header_la = input.category_job_service_sub_fee_header_la.as_deref();
        let desc_la = input.category_job_service_sub_fee_description_la.as_deref();
        let header_en = input.category_job_service_sub_fee_header_en.as_deref();
        let desc_en = input.category_job_service_sub_fee_description_en.as_deref();
        let header_th = input.category_job_service_sub_fee_header_th.as_deref();
        let desc_th = input.category_job_service_sub_fee_description_th.as_deref();
        let header_zh = input.category_job_service_sub_fee_header_zh.as_deref();
        let desc_zh = input.category_job_service_sub_fee_description_zh.as_deref();
        let price: Option<rust_decimal::Decimal> = input.category_job_service_sub_fee_price;
        let status: Option<i32> = input.category_job_service_sub_fee_status;
        let icon = input.category_job_service_sub_fee_icon.as_deref();
        let update_by: &str = input.update_by.as_str();

        let params: &[&dyn ToSql] = &[
            &guid as &dyn ToSql,
            &header_la as &dyn ToSql,
            &desc_la as &dyn ToSql,
            &header_en as &dyn ToSql,
            &desc_en as &dyn ToSql,
            &header_th as &dyn ToSql,
            &desc_th as &dyn ToSql,
            &header_zh as &dyn ToSql,
            &desc_zh as &dyn ToSql,
            &price as &dyn ToSql,
            &status as &dyn ToSql,
            &icon as &dyn ToSql,
            &update_by as &dyn ToSql,
        ];

        let rows = exec_sp(
            &self.pool,
            "EXEC dbo.SP_CATEGORY_JOB_SERVICE_SUB_FEE_UPDATE \
                        @p_category_job_service_sub_fee_guid = @P1, \
                        @p_category_job_service_sub_fee_header_la = @P2, \
                        @p_category_job_service_sub_fee_description_la = @P3, \
                        @p_category_job_service_sub_fee_header_en = @P4, \
                        @p_category_job_service_sub_fee_description_en = @P5, \
                        @p_category_job_service_sub_fee_header_th = @P6, \
                        @p_category_job_service_sub_fee_description_th = @P7, \
                        @p_category_job_service_sub_fee_header_zh = @P8, \
                        @p_category_job_service_sub_fee_description_zh = @P9, \
                        @p_category_job_service_sub_fee_price = @P10, \
                        @p_category_job_service_sub_fee_status = @P11, \
                        @p_category_job_service_sub_fee_icon = @P12, \
                        @p_update_by = @P13",
            params,
        )
        .await?;

        let row = rows.first().ok_or_else(|| {
            RepoError::Backend(
                "SP_CATEGORY_JOB_SERVICE_SUB_FEE_UPDATE returned no result row".into(),
            )
        })?;

        let success: bool = row.get::<bool, _>("success").unwrap_or(false);
        let code = read_str(row, "code").unwrap_or("").to_string();
        let message = read_str(row, "message").unwrap_or("").to_string();
        let out_guid = read_str(row, "category_job_service_sub_fee_guid")
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());

        Ok(CategoryJobServiceSubFeeUpdateResult {
            success,
            code,
            message,
            category_job_service_sub_fee_guid: out_guid,
        })
    }

    async fn delete(
        &self,
        input: &CategoryJobServiceSubFeeDeleteInput,
    ) -> Result<CategoryJobServiceSubFeeDeleteResult, RepoError> {
        let guid = input.category_job_service_sub_fee_guid.as_str();
        let update_by: &str = input.update_by.as_str();

        let params: &[&dyn ToSql] = &[&guid as &dyn ToSql, &update_by as &dyn ToSql];

        let rows = exec_sp(
            &self.pool,
            "EXEC dbo.SP_CATEGORY_JOB_SERVICE_SUB_FEE_DELETE \
                @p_category_job_service_sub_fee_guid = @P1, \
                @p_update_by = @P2",
            params,
        )
        .await?;

        let row = rows.first().ok_or_else(|| {
            RepoError::Backend(
                "SP_CATEGORY_JOB_SERVICE_SUB_FEE_DELETE returned no result row".into(),
            )
        })?;

        let success: bool = row.get::<bool, _>("success").unwrap_or(false);
        let code = read_str(row, "code").unwrap_or("").to_string();
        let message = read_str(row, "message").unwrap_or("").to_string();
        let out_guid = read_str(row, "category_job_service_sub_fee_guid")
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());

        Ok(CategoryJobServiceSubFeeDeleteResult {
            success,
            code,
            message,
            category_job_service_sub_fee_guid: out_guid,
        })
    }

    async fn autocomplete(
        &self,
        input: &CategoryJobServiceSubFeeAutocompleteInput,
    ) -> Result<Vec<CategoryJobServiceSubFeeAutocompleteRow>, RepoError> {
        let guid = input
            .category_job_service_sub_fee_guid
            .as_deref()
            .unwrap_or("");
        let keyword: Option<&str> = input.keyword.as_deref();
        let status: Option<i32> = input.status;
        let locale_raw: Option<&str> = input.locale.as_deref();
        let locale: String = normalize_locale(locale_raw);
        let limit: Option<i32> = Some(input.limit.unwrap_or(20).clamp(1, 100));

        let rows = exec_sp(
            &self.pool,
            "EXEC dbo.SP_CATEGORY_JOB_SERVICE_SUB_FEE_AUTOCOMPLETE \
                @p_category_job_service_sub_fee_guid = @P1, \
                @p_keyword = @P2, \
                @p_locale = @P3, \
                @p_status = @P4, \
                @p_limit = @P5",
            &[
                &guid as &dyn ToSql,
                &keyword as &dyn ToSql,
                &locale as &dyn ToSql,
                &status as &dyn ToSql,
                &limit as &dyn ToSql,
            ],
        )
        .await?;

        Ok(rows.iter().map(row_to_fee_autocomplete_row).collect())
    }
}
