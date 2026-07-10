use async_trait::async_trait;
use chrono::{DateTime, Utc};
use tiberius::ToSql;

use kokkak_domain::traits::category_job_service_sub_warranty::CategoryJobServiceSubWarrantyRepository;
use kokkak_domain::traits::user::RepoError;
use kokkak_domain::{
    CategoryJobServiceSubWarrantyAutocompleteInput, CategoryJobServiceSubWarrantyAutocompleteRow,
    CategoryJobServiceSubWarrantyCreateInput, CategoryJobServiceSubWarrantyCreateResult,
    CategoryJobServiceSubWarrantyDeleteInput, CategoryJobServiceSubWarrantyDeleteResult,
    CategoryJobServiceSubWarrantyDetailRow, CategoryJobServiceSubWarrantyFullDetailRow,
    CategoryJobServiceSubWarrantyListInput, CategoryJobServiceSubWarrantyPage,
    CategoryJobServiceSubWarrantyUpdateInput, CategoryJobServiceSubWarrantyUpdateResult,
};

use crate::db::mssql::{exec_sp, read_i32, read_str, MssqlPool};

#[derive(Clone)]
pub struct MssqlCategoryJobServiceSubWarrantyRepository {
    pool: MssqlPool,
}

impl MssqlCategoryJobServiceSubWarrantyRepository {
    pub fn new(pool: MssqlPool) -> Self {
        Self { pool }
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

fn row_to_warranty_autocomplete_row(
    row: &tiberius::Row,
) -> CategoryJobServiceSubWarrantyAutocompleteRow {
    CategoryJobServiceSubWarrantyAutocompleteRow {
        category_job_service_sub_warranty_guid: read_str(
            row,
            "category_job_service_sub_warranty_guid",
        )
        .unwrap_or("")
        .to_string(),
        category_job_service_sub_warranty_name: read_str(
            row,
            "category_job_service_sub_warranty_name",
        )
        .unwrap_or("")
        .to_string(),
    }
}

fn row_to_warranty_row(row: &tiberius::Row) -> CategoryJobServiceSubWarrantyDetailRow {
    CategoryJobServiceSubWarrantyDetailRow {
        category_job_service_sub_warranty_guid: read_str(
            row,
            "category_job_service_sub_warranty_guid",
        )
        .unwrap_or("")
        .to_string(),
        category_job_service_sub_warranty_description: read_str(
            row,
            "category_job_service_sub_warranty_description",
        )
        .unwrap_or("")
        .to_string(),
        category_job_service_sub_warranty_warranty_amount_day: read_i32(
            row,
            "category_job_service_sub_warranty_warranty_amount_day",
        )
        .unwrap_or(0),
        category_job_service_sub_warranty_status: read_i32(
            row,
            "category_job_service_sub_warranty_status",
        )
        .unwrap_or(0),
        category_job_service_sub_warranty_icon: read_str(
            row,
            "category_job_service_sub_warranty_icon",
        )
        .unwrap_or("")
        .to_string(),
        category_job_service_sub_warranty_create_at: {
            row.get::<DateTime<Utc>, _>("category_job_service_sub_warranty_create_at")
        },
        category_job_service_sub_warranty_create_by: read_str(
            row,
            "category_job_service_sub_warranty_create_by",
        )
        .unwrap_or("")
        .to_string(),
        category_job_service_sub_warranty_update_at: {
            row.get::<DateTime<Utc>, _>("category_job_service_sub_warranty_update_at")
        },
        category_job_service_sub_warranty_update_by: read_str(
            row,
            "category_job_service_sub_warranty_update_by",
        )
        .unwrap_or("")
        .to_string(),
    }
}

fn row_to_warranty_detail_row(row: &tiberius::Row) -> CategoryJobServiceSubWarrantyFullDetailRow {
    CategoryJobServiceSubWarrantyFullDetailRow {
        category_job_service_sub_warranty_guid: read_str(
            row,
            "category_job_service_sub_warranty_guid",
        )
        .unwrap_or("")
        .to_string(),
        category_job_service_sub_warranty_description_la: read_str(
            row,
            "category_job_service_sub_warranty_description_la",
        )
        .unwrap_or("")
        .to_string(),
        category_job_service_sub_warranty_description_en: read_str(
            row,
            "category_job_service_sub_warranty_description_en",
        )
        .unwrap_or("")
        .to_string(),
        category_job_service_sub_warranty_description_th: read_str(
            row,
            "category_job_service_sub_warranty_description_th",
        )
        .unwrap_or("")
        .to_string(),
        category_job_service_sub_warranty_description_zh: read_str(
            row,
            "category_job_service_sub_warranty_description_zh",
        )
        .unwrap_or("")
        .to_string(),
        category_job_service_sub_warranty_warranty_amount_day: read_i32(
            row,
            "category_job_service_sub_warranty_warranty_amount_day",
        )
        .unwrap_or(0),
        category_job_service_sub_warranty_status: read_i32(
            row,
            "category_job_service_sub_warranty_status",
        )
        .unwrap_or(0),
        category_job_service_sub_warranty_icon: read_str(
            row,
            "category_job_service_sub_warranty_icon",
        )
        .unwrap_or("")
        .to_string(),
        category_job_service_sub_warranty_create_at: {
            row.get::<DateTime<Utc>, _>("category_job_service_sub_warranty_create_at")
        },
        category_job_service_sub_warranty_create_by: read_str(
            row,
            "category_job_service_sub_warranty_create_by",
        )
        .unwrap_or("")
        .to_string(),
        category_job_service_sub_warranty_update_at: {
            row.get::<DateTime<Utc>, _>("category_job_service_sub_warranty_update_at")
        },
        category_job_service_sub_warranty_update_by: read_str(
            row,
            "category_job_service_sub_warranty_update_by",
        )
        .unwrap_or("")
        .to_string(),
    }
}

#[async_trait]
impl CategoryJobServiceSubWarrantyRepository for MssqlCategoryJobServiceSubWarrantyRepository {
    async fn list(
        &self,
        input: &CategoryJobServiceSubWarrantyListInput,
    ) -> Result<CategoryJobServiceSubWarrantyPage, RepoError> {
        let guid = input
            .category_job_service_sub_warranty_guid
            .as_deref()
            .unwrap_or("");
        let keyword: Option<&str> = input.keyword.as_deref();
        let status = input.status;
        let locale: String = normalize_locale(input.locale.as_deref());
        let page: i32 = input.page.unwrap_or(1).max(1) as i32;
        let page_size: i32 = input.page_size.unwrap_or(20).clamp(1, 100) as i32;

        let params: &[&dyn ToSql] = &[
            &guid as &dyn ToSql,
            &keyword as &dyn ToSql,
            &status as &dyn ToSql,
            &locale as &dyn ToSql,
            &page as &dyn ToSql,
            &page_size as &dyn ToSql,
        ];

        let rows = exec_sp(
            &self.pool,
            "EXEC dbo.SP_CATEGORY_JOB_SERVICE_SUB_WARRANTY_GET \
                @p_category_job_service_sub_warranty_guid = @P1, \
                @p_keyword = @P2, \
                @p_status = @P3, \
                @p_locale = @P4, \
                @p_page = @P5, \
                @p_page_size = @P6",
            params,
        )
        .await?;

        if rows.is_empty() {
            return Ok(CategoryJobServiceSubWarrantyPage {
                items: Vec::new(),
                total_count: 0,
                page: input.page.unwrap_or(1),
                page_size: input.page_size.unwrap_or(20),
                total_page: 0,
            });
        }

        let first = &rows[0];
        let total_count: i64 = first.get::<i32, _>("total_count").unwrap_or(0) as i64;
        let page: u32 = first
            .get::<i32, _>("page")
            .map(|v| v.max(0) as u32)
            .unwrap_or(input.page.unwrap_or(1));
        let page_size: u32 = first
            .get::<i32, _>("page_size")
            .map(|v| v.max(0) as u32)
            .unwrap_or(input.page_size.unwrap_or(20));
        let total_page: u32 = if page_size == 0 {
            0
        } else {
            (total_count as u64).div_ceil(page_size as u64) as u32
        };

        let items: Vec<CategoryJobServiceSubWarrantyDetailRow> =
            rows.iter().map(row_to_warranty_row).collect();

        Ok(CategoryJobServiceSubWarrantyPage {
            items,
            total_count,
            page,
            page_size,
            total_page,
        })
    }

    async fn create(
        &self,
        input: &CategoryJobServiceSubWarrantyCreateInput,
    ) -> Result<CategoryJobServiceSubWarrantyCreateResult, RepoError> {
        let guid = input
            .category_job_service_sub_warranty_guid
            .as_deref()
            .unwrap_or("");
        let desc_la = input
            .category_job_service_sub_warranty_description_la
            .as_deref();
        let desc_en = input
            .category_job_service_sub_warranty_description_en
            .as_deref();
        let desc_th = input
            .category_job_service_sub_warranty_description_th
            .as_deref();
        let desc_zh = input
            .category_job_service_sub_warranty_description_zh
            .as_deref();
        let amount_day: i32 = input.category_job_service_sub_warranty_warranty_amount_day;
        let status: i32 = input.category_job_service_sub_warranty_status;
        let icon = input.category_job_service_sub_warranty_icon.as_deref();
        let create_by: &str = input.create_by.as_str();

        let params: &[&dyn ToSql] = &[
            &guid as &dyn ToSql,
            &desc_la as &dyn ToSql,
            &desc_en as &dyn ToSql,
            &desc_th as &dyn ToSql,
            &desc_zh as &dyn ToSql,
            &amount_day as &dyn ToSql,
            &status as &dyn ToSql,
            &icon as &dyn ToSql,
            &create_by as &dyn ToSql,
        ];

        let rows = exec_sp(
            &self.pool,
            "EXEC dbo.SP_CATEGORY_JOB_SERVICE_SUB_WARRANTY_INSERT \
                @p_category_job_service_sub_warranty_guid = @P1, \
                @p_category_job_service_sub_warranty_description_la = @P2, \
                @p_category_job_service_sub_warranty_description_en = @P3, \
                @p_category_job_service_sub_warranty_description_th = @P4, \
                @p_category_job_service_sub_warranty_description_zh = @P5, \
                @p_category_job_service_sub_warranty_warranty_amount_day = @P6, \
                @p_category_job_service_sub_warranty_status = @P7, \
                @p_category_job_service_sub_warranty_icon = @P8, \
                @p_create_by = @P9",
            params,
        )
        .await?;

        let row = rows.first().ok_or_else(|| {
            RepoError::Backend(
                "SP_CATEGORY_JOB_SERVICE_SUB_WARRANTY_INSERT returned no result row".into(),
            )
        })?;

        let success: bool = row.get::<bool, _>("success").unwrap_or(false);
        let code = read_str(row, "code").unwrap_or("").to_string();
        let message = read_str(row, "message").unwrap_or("").to_string();
        let out_guid = read_str(row, "category_job_service_sub_warranty_guid")
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());

        Ok(CategoryJobServiceSubWarrantyCreateResult {
            success,
            code,
            message,
            category_job_service_sub_warranty_guid: out_guid,
        })
    }

    async fn update(
        &self,
        input: &CategoryJobServiceSubWarrantyUpdateInput,
    ) -> Result<CategoryJobServiceSubWarrantyUpdateResult, RepoError> {
        let guid = input.category_job_service_sub_warranty_guid.as_str();
        let desc_la = input
            .category_job_service_sub_warranty_description_la
            .as_deref();
        let desc_en = input
            .category_job_service_sub_warranty_description_en
            .as_deref();
        let desc_th = input
            .category_job_service_sub_warranty_description_th
            .as_deref();
        let desc_zh = input
            .category_job_service_sub_warranty_description_zh
            .as_deref();
        let amount_day: Option<i32> = input.category_job_service_sub_warranty_warranty_amount_day;
        let status: Option<i32> = input.category_job_service_sub_warranty_status;
        let icon = input.category_job_service_sub_warranty_icon.as_deref();
        let update_by: &str = input.update_by.as_str();

        let params: &[&dyn ToSql] = &[
            &guid as &dyn ToSql,
            &desc_la as &dyn ToSql,
            &desc_en as &dyn ToSql,
            &desc_th as &dyn ToSql,
            &desc_zh as &dyn ToSql,
            &amount_day as &dyn ToSql,
            &status as &dyn ToSql,
            &icon as &dyn ToSql,
            &update_by as &dyn ToSql,
        ];

        let rows = exec_sp(
            &self.pool,
            "EXEC dbo.SP_CATEGORY_JOB_SERVICE_SUB_WARRANTY_UPDATE \
                @p_category_job_service_sub_warranty_guid = @P1, \
                @p_category_job_service_sub_warranty_description_la = @P2, \
                @p_category_job_service_sub_warranty_description_en = @P3, \
                @p_category_job_service_sub_warranty_description_th = @P4, \
                @p_category_job_service_sub_warranty_description_zh = @P5, \
                @p_category_job_service_sub_warranty_warranty_amount_day = @P6, \
                @p_category_job_service_sub_warranty_status = @P7, \
                @p_category_job_service_sub_warranty_icon = @P8, \
                @p_update_by = @P9",
            params,
        )
        .await?;

        let row = rows.first().ok_or_else(|| {
            RepoError::Backend(
                "SP_CATEGORY_JOB_SERVICE_SUB_WARRANTY_UPDATE returned no result row".into(),
            )
        })?;

        let success: bool = row.get::<bool, _>("success").unwrap_or(false);
        let code = read_str(row, "code").unwrap_or("").to_string();
        let message = read_str(row, "message").unwrap_or("").to_string();
        let out_guid = read_str(row, "category_job_service_sub_warranty_guid")
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());

        Ok(CategoryJobServiceSubWarrantyUpdateResult {
            success,
            code,
            message,
            category_job_service_sub_warranty_guid: out_guid,
        })
    }

    async fn delete(
        &self,
        input: &CategoryJobServiceSubWarrantyDeleteInput,
    ) -> Result<CategoryJobServiceSubWarrantyDeleteResult, RepoError> {
        let guid = input.category_job_service_sub_warranty_guid.as_str();
        let update_by: &str = input.update_by.as_str();

        let params: &[&dyn ToSql] = &[&guid as &dyn ToSql, &update_by as &dyn ToSql];

        let rows = exec_sp(
            &self.pool,
            "EXEC dbo.SP_CATEGORY_JOB_SERVICE_SUB_WARRANTY_DELETE \
                @p_category_job_service_sub_warranty_guid = @P1, \
                @p_update_by = @P2",
            params,
        )
        .await?;

        let row = rows.first().ok_or_else(|| {
            RepoError::Backend(
                "SP_CATEGORY_JOB_SERVICE_SUB_WARRANTY_DELETE returned no result row".into(),
            )
        })?;

        let success: bool = row.get::<bool, _>("success").unwrap_or(false);
        let code = read_str(row, "code").unwrap_or("").to_string();
        let message = read_str(row, "message").unwrap_or("").to_string();
        let out_guid = read_str(row, "category_job_service_sub_warranty_guid")
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());

        Ok(CategoryJobServiceSubWarrantyDeleteResult {
            success,
            code,
            message,
            category_job_service_sub_warranty_guid: out_guid,
        })
    }

    async fn autocomplete(
        &self,
        input: &CategoryJobServiceSubWarrantyAutocompleteInput,
    ) -> Result<Vec<CategoryJobServiceSubWarrantyAutocompleteRow>, RepoError> {
        let guid = input
            .category_job_service_sub_warranty_guid
            .as_deref()
            .unwrap_or("");
        let keyword: Option<&str> = input.keyword.as_deref();
        let status: Option<i32> = input.status;
        let locale_raw: Option<&str> = input.locale.as_deref();
        let locale: String = normalize_locale(locale_raw);
        let limit: Option<i32> = Some(input.limit.unwrap_or(20).clamp(1, 100));

        let rows = exec_sp(
            &self.pool,
            "EXEC dbo.SP_CATEGORY_JOB_SERVICE_SUB_WARRANTY_AUTOCOMPLETE \
                @p_category_job_service_sub_warranty_guid = @P1, \
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

        Ok(rows.iter().map(row_to_warranty_autocomplete_row).collect())
    }

    async fn detail(
        &self,
        category_job_service_sub_warranty_guid: &str,
    ) -> Result<Option<CategoryJobServiceSubWarrantyFullDetailRow>, RepoError> {
        let guid = category_job_service_sub_warranty_guid.trim();
        if guid.is_empty() {
            return Ok(None);
        }

        let rows = exec_sp(
            &self.pool,
            "EXEC dbo.SP_CATEGORY_JOB_SERVICE_SUB_WARRANTY_DETAIL_GET \
                @p_category_job_service_sub_warranty_guid = @P1",
            &[&guid as &dyn ToSql],
        )
        .await?;

        Ok(rows.first().map(row_to_warranty_detail_row))
    }
}
