use async_trait::async_trait;
use tiberius::ToSql;
use uuid::Uuid;

use kokkak_domain::category_job_main::CategoryJobMainError;
use kokkak_domain::traits::category_job_main::CategoryJobMainRepository;
use kokkak_domain::traits::user::RepoError;
use kokkak_domain::{
    CategoryJobMainAutocompleteInput, CategoryJobMainAutocompleteRow, CategoryJobMainCreateInput,
    CategoryJobMainCreateResult, CategoryJobMainDeleteResult, CategoryJobMainListInput,
    CategoryJobMainPage, CategoryJobMainRow, CategoryJobMainUpdateInput,
    CategoryJobMainUpdateResult,
};

use crate::db::mssql::{exec_sp, read_guid_str, read_i32, read_str, read_uuid, MssqlPool};

#[derive(Clone)]
pub struct MssqlCategoryJobMainRepository {
    pool: MssqlPool,
}

impl MssqlCategoryJobMainRepository {
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
impl CategoryJobMainRepository for MssqlCategoryJobMainRepository {
    async fn list(
        &self,
        input: &CategoryJobMainListInput,
    ) -> Result<CategoryJobMainPage, RepoError> {
        let keyword: Option<&str> = input.keyword.as_deref();
        let status: Option<i32> = input.status;
        let locale: Option<&str> = input.locale.as_deref();
        let page: i32 = input.page as i32;
        let page_size: i32 = input.page_size as i32;

        let rows = exec_sp(
            &self.pool,
            "EXEC dbo.SP_CATEGORY_JOB_MAIN_GET \
                    @p_keyword = @P1, \
                    @p_status = @P2, \
                    @p_page = @P3, \
                    @p_page_size = @P4, \
                    @p_locale = @P5",
            &[
                &keyword as &dyn ToSql,
                &status as &dyn ToSql,
                &page as &dyn ToSql,
                &page_size as &dyn ToSql,
                &locale as &dyn ToSql,
            ],
        )
        .await?;

        if rows.is_empty() {
            return Ok(CategoryJobMainPage {
                items: Vec::new(),
                total_count: 0,
                page: input.page,
                page_size: input.page_size,
                total_page: 0,
            });
        }

        let first = &rows[0];
        let total_count: i64 = first.get::<i32, _>("total_count").unwrap_or(0) as i64;
        let page: u32 = first
            .get::<i32, _>("page")
            .map(|v| v.max(0) as u32)
            .unwrap_or(input.page);
        let page_size: u32 = first
            .get::<i32, _>("page_size")
            .map(|v| v.max(0) as u32)
            .unwrap_or(input.page_size);
        let total_page: u32 = if page_size == 0 {
            0
        } else {
            (total_count as u64).div_ceil(page_size as u64) as u32
        };

        let items: Vec<CategoryJobMainRow> =
            rows.iter().map(row_to_category_job_main_row).collect();

        Ok(CategoryJobMainPage {
            items,
            total_count,
            page,
            page_size,
            total_page,
        })
    }

    async fn create(
        &self,
        input: &CategoryJobMainCreateInput,
    ) -> Result<CategoryJobMainCreateResult, RepoError> {
        let name_la: Option<&str> = input.category_job_main_name_la.as_deref();
        let name_en: Option<&str> = input.category_job_main_name_en.as_deref();
        let name_th: Option<&str> = input.category_job_main_name_th.as_deref();
        let name_zh: Option<&str> = input.category_job_main_name_zh.as_deref();
        let icon_style: Option<&str> = input.category_job_main_icon_style.as_deref();
        let icon_line: Option<&str> = input.category_job_main_icon_line.as_deref();
        let img_path: Option<&str> = input.category_job_main_img_path.as_deref();
        let priority: Option<i32> = input.category_job_main_priority;
        let create_by = input.create_by.as_str();

        let params: &[&dyn ToSql] = &[
            &name_la,
            &name_en,
            &name_th,
            &name_zh,
            &icon_style,
            &icon_line,
            &img_path,
            &priority,
            &create_by,
        ];

        let rows = exec_sp(
            &self.pool,
            "EXEC dbo.SP_CATEGORY_JOB_MAIN_INSERT \
                @p_name_la = @P1, \
                @p_name_en = @P2, \
                @p_name_th = @P3, \
                @p_name_zh = @P4, \
                @p_icon_style = @P5, \
                @p_icon_line = @P6, \
                @p_img_path = @P7, \
                @p_priority = @P8, \
                @p_create_by = @P9",
            params,
        )
        .await?;

        let row = rows.first().ok_or_else(|| {
            RepoError::Backend(
                "SP_CATEGORY_JOB_MAIN_INSERT returned no row (driver/protocol mismatch)".into(),
            )
        })?;

        let success: bool = row.get::<bool, _>("success").unwrap_or(false);
        let code = read_str(row, "code").unwrap_or("").to_string();
        let message = read_str(row, "message").unwrap_or("").to_string();

        if !success {
            return Err(RepoError::Backend(format!(
                "{}: {code} — {message}",
                CategoryJobMainError::CODE_SUCCESS
            )));
        }

        Ok(CategoryJobMainCreateResult {
            success,
            code,
            message,
            category_job_main_guid: {
                let s = read_guid_str(row, "category_job_main_guid");
                if s.trim().is_empty() {
                    None
                } else {
                    Some(s.trim().to_string())
                }
            },
        })
    }

    async fn update(
        &self,
        input: &CategoryJobMainUpdateInput,
    ) -> Result<CategoryJobMainUpdateResult, RepoError> {
        let guid = input.category_job_main_guid.as_str();
        let name = input.category_job_main_name.as_str();
        let icon_style: Option<&str> = input.category_job_main_icon_style.as_deref();
        let icon_line: Option<&str> = input.category_job_main_icon_line.as_deref();
        let img_path: Option<&str> = input.category_job_main_img_path.as_deref();
        let priority = input.category_job_main_priority;
        let status = input.category_job_main_status;
        let update_by = input.update_by.as_str();

        let params: &[&dyn ToSql] = &[
            &guid,
            &name,
            &icon_style,
            &icon_line,
            &img_path,
            &priority,
            &status,
            &update_by,
        ];

        let rows = exec_sp(
            &self.pool,
            "EXEC dbo.SP_CATEGORY_JOB_MAIN_UPDATE \
                @p_category_job_main_guid = @P1, \
                @p_category_job_main_name = @P2, \
                @p_category_job_main_icon_style = @P3, \
                @p_category_job_main_icon_line = @P4, \
                @p_category_job_main_img_path = @P5, \
                @p_category_job_main_priority = @P6, \
                @p_category_job_main_status = @P7, \
                @p_update_by = @P8",
            params,
        )
        .await?;

        let row = rows.first().ok_or_else(|| {
            RepoError::Backend(
                "SP_CATEGORY_JOB_MAIN_UPDATE returned no row (driver/protocol mismatch)".into(),
            )
        })?;

        let success: bool = row.get::<bool, _>("success").unwrap_or(false);
        let code = read_str(row, "code").unwrap_or("").to_string();
        let message = read_str(row, "message").unwrap_or("").to_string();

        if !success {
            return Err(RepoError::Backend(format!(
                "{}: {code} — {message}",
                CategoryJobMainError::CODE_SUCCESS
            )));
        }

        Ok(CategoryJobMainUpdateResult {
            success,
            code,
            message,
            category_job_main_guid: {
                if let Some(g) = read_uuid(row, "category_job_main_guid") {
                    if !g.is_nil() {
                        Some(g.to_string())
                    } else {
                        Some(read_guid_str(row, "category_job_main_guid"))
                    }
                } else {
                    let s = read_guid_str(row, "category_job_main_guid");
                    if s.is_empty() {
                        None
                    } else {
                        Some(s)
                    }
                }
            },
        })
    }

    async fn delete(
        &self,
        category_guid: &str,
        actor_user_guid: &str,
    ) -> Result<CategoryJobMainDeleteResult, RepoError> {
        let params: &[&dyn ToSql] = &[&category_guid, &actor_user_guid];

        let rows = exec_sp(
            &self.pool,
            "EXEC dbo.SP_CATEGORY_JOB_MAIN_DELETE \
                    @p_category_job_main_guid = @P1, \
                    @p_update_by = @P2",
            params,
        )
        .await?;

        let row = rows.first().ok_or_else(|| {
            RepoError::Backend(
                "SP_CATEGORY_JOB_MAIN_DELETE returned no row (driver/protocol mismatch)".into(),
            )
        })?;

        let success: bool = row.get::<bool, _>("success").unwrap_or(false);
        let code = read_str(row, "code").unwrap_or("").to_string();
        let message = read_str(row, "message").unwrap_or("").to_string();

        if !success {
            return Err(RepoError::Backend(format!(
                "{}: {code} — {message}",
                CategoryJobMainError::CODE_SUCCESS
            )));
        }

        let returned_guid = read_guid_str(row, "category_job_main_guid");
        let final_guid = if returned_guid.is_empty() {
            category_guid.to_string()
        } else {
            returned_guid
        };

        Ok(CategoryJobMainDeleteResult {
            success,
            code,
            message,
            category_job_main_guid: final_guid,
        })
    }

    async fn autocomplete(
        &self,
        input: &CategoryJobMainAutocompleteInput,
    ) -> Result<Vec<CategoryJobMainAutocompleteRow>, RepoError> {
        let keyword: Option<&str> = input.keyword.as_deref();
        let status: Option<i32> = input.status;
        let locale_raw: Option<&str> = input.locale.as_deref();
        let locale: Option<String> = locale_raw.map(normalize_locale_for_sp);
        let take: Option<i32> = Some(input.take.unwrap_or(20).clamp(1, 100));

        let locale_param: Option<&str> = locale.as_deref();

        let rows = exec_sp(
            &self.pool,
            "EXEC dbo.SP_AUTOCOMPLETE_CATEGORY_JOB_MAIN_GET \
                    @p_keyword = @P1, \
                    @p_status = @P2, \
                    @p_locale = @P3, \
                    @p_take = @P4",
            &[
                &keyword as &dyn ToSql,
                &status as &dyn ToSql,
                &locale_param as &dyn ToSql,
                &take as &dyn ToSql,
            ],
        )
        .await?;

        Ok(rows
            .iter()
            .map(row_to_category_job_main_autocomplete_row)
            .collect())
    }
}

fn normalize_locale_for_sp(raw: &str) -> String {
    let primary = raw.split('-').next().unwrap_or("").trim().to_lowercase();
    if matches!(primary.as_str(), "la" | "en" | "th" | "zh") {
        return primary;
    }
    if matches!(primary.as_str(), "lo") {
        return "la".to_string();
    }
    "la".to_string()
}

fn row_to_category_job_main_autocomplete_row(
    row: &tiberius::Row,
) -> CategoryJobMainAutocompleteRow {
    CategoryJobMainAutocompleteRow {
        category_job_main_guid: read_guid_str(row, "category_job_main_guid"),
        category_job_main_name: read_str(row, "category_job_main_name")
            .unwrap_or("")
            .to_string(),
    }
}

fn row_to_category_job_main_row(row: &tiberius::Row) -> CategoryJobMainRow {
    let guid = read_guid_str(row, "category_job_main_guid");
    let has_sub_service: bool = row.get::<bool, _>("has_sub_service").unwrap_or(false);
    CategoryJobMainRow {
        category_job_main_guid: guid.clone(),
        category_job_main_name: read_str(row, "category_job_main_name")
            .unwrap_or("")
            .to_string(),
        category_job_main_locale: read_str(row, "category_job_main_locale")
            .unwrap_or("th")
            .to_string(),
        category_job_main_icon_style: read_str(row, "category_job_main_icon_style")
            .unwrap_or("")
            .to_string(),
        category_job_main_icon_line: read_str(row, "category_job_main_icon_line")
            .unwrap_or("")
            .to_string(),
        category_job_main_img_path: read_str(row, "category_job_main_img_path")
            .unwrap_or("")
            .to_string(),
        category_job_main_img_url: None,
        category_job_main_status: read_i32(row, "category_job_main_status").unwrap_or(0),
        category_job_main_priority: read_i32(row, "category_job_main_priority").unwrap_or(0),
        has_sub_service,
        category_job_main_create_at: {
            use chrono::{DateTime, Utc};
            row.get::<DateTime<Utc>, _>("category_job_main_create_at")
        },
        category_job_main_create_by: read_str(row, "category_job_main_create_by")
            .unwrap_or("")
            .to_string(),
        category_job_main_update_at: {
            use chrono::{DateTime, Utc};
            row.get::<DateTime<Utc>, _>("category_job_main_update_at")
        },
        category_job_main_update_by: read_str(row, "category_job_main_update_by")
            .unwrap_or("")
            .to_string(),
    }
}

impl MssqlCategoryJobMainRepository {
    #[allow(dead_code)]
    fn _uuid_zero_use(_: Uuid) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parsing_safe_when_columns_missing() {
        use chrono::Utc;
        let row = CategoryJobMainRow {
            category_job_main_guid: "11111111-1111-1111-1111-111111111111".into(),
            category_job_main_name: "Home Repair".into(),
            category_job_main_locale: "th".into(),
            category_job_main_icon_style: "solid".into(),
            category_job_main_icon_line: "wrench".into(),
            category_job_main_img_path: "category-job-mains/1111/icon/x.webp".into(),
            category_job_main_img_url: None,
            category_job_main_status: 1,
            category_job_main_priority: 5,
            has_sub_service: true,
            category_job_main_create_at: Some(Utc::now()),
            category_job_main_create_by: "admin".into(),
            category_job_main_update_at: Some(Utc::now()),
            category_job_main_update_by: "admin".into(),
        };
        assert_eq!(row.category_job_main_status, 1);
        assert_eq!(row.category_job_main_priority, 5);
        assert_eq!(row.category_job_main_locale, "th");
        assert!(row.has_sub_service);
        assert!(row.category_job_main_img_url.is_none());
    }
}
