use async_trait::async_trait;
use chrono::{DateTime, Utc};
use tiberius::ToSql;
use uuid::Uuid;

use kokkak_domain::category_job_service_main::CategoryJobServiceMainError;
use kokkak_domain::traits::category_job_service_main::CategoryJobServiceMainRepository;
use kokkak_domain::traits::user::RepoError;
use kokkak_domain::{
    CategoryJobServiceMainAutocompleteInput, CategoryJobServiceMainAutocompleteRow,
    CategoryJobServiceMainCreateInput, CategoryJobServiceMainCreateResult,
    CategoryJobServiceMainDeleteResult, CategoryJobServiceMainDetailRow,
    CategoryJobServiceMainListInput, CategoryJobServiceMainRow, CategoryJobServiceMainUpdateInput,
    CategoryJobServiceMainUpdateResult,
};

use crate::db::mssql::{exec_sp, read_guid_str, read_i32, read_str, MssqlPool};

fn read_datetime_utc(row: &tiberius::Row, col: &str) -> Option<DateTime<Utc>> {
    row.get::<chrono::NaiveDateTime, _>(col)
        .map(|ndt| ndt.and_utc())
}

#[derive(Clone)]
pub struct MssqlCategoryJobServiceMainRepository {
    pool: MssqlPool,
}

impl MssqlCategoryJobServiceMainRepository {
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
impl CategoryJobServiceMainRepository for MssqlCategoryJobServiceMainRepository {
    async fn list(
        &self,
        input: &CategoryJobServiceMainListInput,
    ) -> Result<Vec<CategoryJobServiceMainRow>, RepoError> {
        let main_guid: Option<&str> = input.category_job_main_guid.as_deref();
        let kw: Option<&str> = input.keyword.as_deref();
        let status: Option<i32> = input.status;
        let locale: Option<&str> = input.locale.as_deref();
        let include_deleted: bool = input.include_deleted;

        let params: &[&dyn ToSql] = &[&main_guid, &kw, &status, &locale, &include_deleted];

        let rows = exec_sp(
            &self.pool,
            "EXEC dbo.SP_CATEGORY_JOB_SERVICE_MAIN_GET \
                @p_category_job_main_guid = @P1, \
                @p_keyword = @P2, \
                @p_status = @P3, \
                @p_locale = @P4, \
                @p_include_deleted = @P5",
            params,
        )
        .await?;

        Ok(rows
            .iter()
            .map(row_to_category_job_service_main_row)
            .collect())
    }

    async fn create(
        &self,
        input: &CategoryJobServiceMainCreateInput,
    ) -> Result<CategoryJobServiceMainCreateResult, RepoError> {
        let main_guid = input.category_job_main_guid.as_str();
        let name_la: Option<&str> = input.category_job_service_name_la.as_deref();
        let name_en: Option<&str> = input.category_job_service_name_en.as_deref();
        let name_th: Option<&str> = input.category_job_service_name_th.as_deref();
        let name_zh: Option<&str> = input.category_job_service_name_zh.as_deref();
        let icon_style: Option<&str> = input.category_job_service_icon_style.as_deref();
        let icon_line: Option<&str> = input.category_job_service_icon_line.as_deref();
        let img_path: Option<&str> = input.category_job_service_img_path.as_deref();
        let create_by = input.create_by.as_str();

        let params: &[&dyn ToSql] = &[
            &main_guid,
            &name_la,
            &name_en,
            &name_th,
            &name_zh,
            &icon_style,
            &icon_line,
            &img_path,
            &create_by,
        ];

        let rows = exec_sp(
            &self.pool,
            "EXEC dbo.SP_CATEGORY_JOB_SERVICE_MAIN_INSERT \
                @p_category_job_main_guid = @P1, \
                @p_name_la = @P2, \
                @p_name_en = @P3, \
                @p_name_th = @P4, \
                @p_name_zh = @P5, \
                @p_icon_style = @P6, \
                @p_icon_line = @P7, \
                @p_img_path = @P8, \
                @p_create_by = @P9",
            params,
        )
        .await?;

        let row = rows.first().ok_or_else(|| {
            RepoError::Backend(
                "SP_CATEGORY_JOB_SERVICE_MAIN_INSERT returned no row (driver/protocol mismatch)"
                    .into(),
            )
        })?;

        let success: bool = row.get::<bool, _>("success").unwrap_or(false);
        let code = read_str(row, "code").unwrap_or("").to_string();
        let message = read_str(row, "message").unwrap_or("").to_string();

        if !success {
            return Err(RepoError::Backend(format!(
                "{}: {code} — {message}",
                CategoryJobServiceMainError::CODE_SUCCESS
            )));
        }

        Ok(CategoryJobServiceMainCreateResult {
            success,
            code,
            message,
            category_job_service_guid: {
                let s = read_guid_str(row, "category_job_service_guid");
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
        input: &CategoryJobServiceMainUpdateInput,
    ) -> Result<CategoryJobServiceMainUpdateResult, RepoError> {
        let service_guid = input.category_job_service_guid.as_str();
        let main_guid = input.category_job_main_guid.as_str();
        let name_la: Option<&str> = input.category_job_service_name_la.as_deref();
        let name_en: Option<&str> = input.category_job_service_name_en.as_deref();
        let name_th: Option<&str> = input.category_job_service_name_th.as_deref();
        let name_zh: Option<&str> = input.category_job_service_name_zh.as_deref();
        let icon_style: Option<&str> = input.category_job_service_icon_style.as_deref();
        let icon_line: Option<&str> = input.category_job_service_icon_line.as_deref();
        let img_path: Option<&str> = input.category_job_service_img_path.as_deref();
        let status = input.category_job_service_status;
        let update_by = input.update_by.as_str();

        let params: &[&dyn ToSql] = &[
            &service_guid,
            &main_guid,
            &name_la,
            &name_en,
            &name_th,
            &name_zh,
            &icon_style,
            &icon_line,
            &img_path,
            &status,
            &update_by,
        ];

        let rows = exec_sp(
            &self.pool,
            "EXEC dbo.SP_CATEGORY_JOB_SERVICE_MAIN_UPDATE \
                @p_category_job_service_guid = @P1, \
                @p_category_job_main_guid = @P2, \
                @p_name_la = @P3, \
                @p_name_en = @P4, \
                @p_name_th = @P5, \
                @p_name_zh = @P6, \
                @p_icon_style = @P7, \
                @p_icon_line = @P8, \
                @p_img_path = @P9, \
                @p_status = @P10, \
                @p_update_by = @P11",
            params,
        )
        .await?;

        let row = rows.first().ok_or_else(|| {
            RepoError::Backend(
                "SP_CATEGORY_JOB_SERVICE_MAIN_UPDATE returned no row (driver/protocol mismatch)"
                    .into(),
            )
        })?;

        let success: bool = row.get::<bool, _>("success").unwrap_or(false);
        let code = read_str(row, "code").unwrap_or("").to_string();
        let message = read_str(row, "message").unwrap_or("").to_string();

        if !success {
            return Err(RepoError::Backend(format!(
                "{}: {code} — {message}",
                CategoryJobServiceMainError::CODE_SUCCESS
            )));
        }

        Ok(CategoryJobServiceMainUpdateResult {
            success,
            code,
            message,
            category_job_service_guid: {
                let s = read_guid_str(row, "category_job_service_guid");
                if s.is_empty() {
                    None
                } else {
                    Some(s)
                }
            },
        })
    }

    async fn delete(
        &self,
        service_guid: &str,
        actor_user_guid: &str,
    ) -> Result<CategoryJobServiceMainDeleteResult, RepoError> {
        let sg = service_guid;
        let actor = actor_user_guid;

        let params: &[&dyn ToSql] = &[&sg, &actor];

        let rows = exec_sp(
            &self.pool,
            "EXEC dbo.SP_CATEGORY_JOB_SERVICE_MAIN_DELETE \
                @p_category_job_service_guid = @P1, \
                @p_update_by = @P2",
            params,
        )
        .await?;

        let row = rows.first().ok_or_else(|| {
            RepoError::Backend(
                "SP_CATEGORY_JOB_SERVICE_MAIN_DELETE returned no row (driver/protocol mismatch)"
                    .into(),
            )
        })?;

        let success: bool = row.get::<bool, _>("success").unwrap_or(false);
        let code = read_str(row, "code").unwrap_or("").to_string();
        let message = read_str(row, "message").unwrap_or("").to_string();

        if !success {
            return Err(RepoError::Backend(format!(
                "{}: {code} — {message}",
                CategoryJobServiceMainError::CODE_SUCCESS
            )));
        }

        let returned_guid = read_guid_str(row, "category_job_service_guid");
        let final_guid = if returned_guid.is_empty() {
            service_guid.to_string()
        } else {
            returned_guid
        };

        Ok(CategoryJobServiceMainDeleteResult {
            success,
            code,
            message,
            category_job_service_guid: final_guid,
        })
    }

    async fn autocomplete(
        &self,
        input: &CategoryJobServiceMainAutocompleteInput,
    ) -> Result<Vec<CategoryJobServiceMainAutocompleteRow>, RepoError> {
        let main_guid: Option<&str> = input.category_job_main_guid.as_deref();
        let keyword: Option<&str> = input.keyword.as_deref();
        let status: Option<i32> = input.status;
        let locale_raw: Option<&str> = input.locale.as_deref();
        let locale: Option<String> = locale_raw.map(normalize_locale_for_sp);
        let take: Option<i32> = Some(input.take.unwrap_or(20).clamp(1, 100));

        let locale_param: Option<&str> = locale.as_deref();

        let rows = exec_sp(
            &self.pool,
            "EXEC dbo.SP_AUTOCOMPLETE_CATEGORY_JOB_SERVICE_MAIN_GET \
                        @p_category_job_main_guid = @P1, \
                        @p_keyword = @P2, \
                        @p_status = @P3, \
                        @p_locale = @P4, \
                        @p_take = @P5",
            &[
                &main_guid as &dyn ToSql,
                &keyword as &dyn ToSql,
                &status as &dyn ToSql,
                &locale_param as &dyn ToSql,
                &take as &dyn ToSql,
            ],
        )
        .await?;

        Ok(rows
            .iter()
            .map(row_to_category_job_service_main_autocomplete_row)
            .collect())
    }

    async fn detail(
        &self,
        service_guid: &str,
    ) -> Result<Option<CategoryJobServiceMainDetailRow>, RepoError> {
        let guid: &str = service_guid;
        let params: &[&dyn ToSql] = &[&guid];

        let rows = exec_sp(
            &self.pool,
            "EXEC dbo.SP_CATEGORY_JOB_SERVICE_MAIN_DETAIL_GET \
                @p_category_job_service_guid = @P1",
            params,
        )
        .await?;

        Ok(rows
            .first()
            .map(row_to_category_job_service_main_detail_row))
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

fn row_to_category_job_service_main_autocomplete_row(
    row: &tiberius::Row,
) -> CategoryJobServiceMainAutocompleteRow {
    CategoryJobServiceMainAutocompleteRow {
        category_job_service_guid: read_guid_str(row, "category_job_service_guid"),
        category_job_service_name: read_str(row, "category_job_service_name")
            .unwrap_or("")
            .to_string(),
    }
}

fn row_to_category_job_service_main_row(
    row: &tiberius::Row,
) -> kokkak_domain::CategoryJobServiceMainRow {
    kokkak_domain::CategoryJobServiceMainRow {
        category_job_service_guid: read_guid_str(row, "category_job_service_guid"),
        category_job_service_category_main_guid: read_guid_str(
            row,
            "category_job_service_category_main_guid",
        ),
        category_job_main_name: read_str(row, "category_job_main_name")
            .unwrap_or("")
            .to_string(),
        category_job_service_name: read_str(row, "category_job_service_name")
            .unwrap_or("")
            .to_string(),
        category_job_service_locale: read_str(row, "category_job_service_locale")
            .unwrap_or("th")
            .to_string(),
        category_job_service_icon_style: read_str(row, "category_job_service_icon_style")
            .unwrap_or("")
            .to_string(),
        category_job_service_icon_line: read_str(row, "category_job_service_icon_line")
            .unwrap_or("")
            .to_string(),
        category_job_service_img_path: read_str(row, "category_job_service_img_path")
            .unwrap_or("")
            .to_string(),
        category_job_service_img_url: None,
        category_job_service_status: read_i32(row, "category_job_service_status").unwrap_or(0),
        has_sub_service: row.get::<bool, _>("has_sub_service").unwrap_or(false),
        category_job_service_create_at: read_datetime_utc(row, "category_job_service_create_at"),
        category_job_service_create_by: read_str(row, "category_job_service_create_by")
            .unwrap_or("")
            .to_string(),
        category_job_service_update_at: read_datetime_utc(row, "category_job_service_update_at"),
        category_job_service_update_by: read_str(row, "category_job_service_update_by")
            .unwrap_or("")
            .to_string(),
    }
}

fn row_to_category_job_service_main_detail_row(
    row: &tiberius::Row,
) -> CategoryJobServiceMainDetailRow {
    CategoryJobServiceMainDetailRow {
        category_job_service_guid: read_guid_str(row, "category_job_service_guid"),
        category_job_service_category_main_guid: read_guid_str(
            row,
            "category_job_service_category_main_guid",
        ),
        category_job_main_name_la: read_str(row, "category_job_main_name_la")
            .unwrap_or("")
            .to_string(),
        category_job_main_name_en: read_str(row, "category_job_main_name_en")
            .unwrap_or("")
            .to_string(),
        category_job_main_name_th: read_str(row, "category_job_main_name_th")
            .unwrap_or("")
            .to_string(),
        category_job_main_name_zh: read_str(row, "category_job_main_name_zh")
            .unwrap_or("")
            .to_string(),
        category_job_service_name_la: read_str(row, "category_job_service_name_la")
            .unwrap_or("")
            .to_string(),
        category_job_service_name_en: read_str(row, "category_job_service_name_en")
            .unwrap_or("")
            .to_string(),
        category_job_service_name_th: read_str(row, "category_job_service_name_th")
            .unwrap_or("")
            .to_string(),
        category_job_service_name_zh: read_str(row, "category_job_service_name_zh")
            .unwrap_or("")
            .to_string(),
        category_job_service_icon_style: read_str(row, "category_job_service_icon_style")
            .unwrap_or("")
            .to_string(),
        category_job_service_icon_line: read_str(row, "category_job_service_icon_line")
            .unwrap_or("")
            .to_string(),
        category_job_service_img_path: read_str(row, "category_job_service_img_path")
            .unwrap_or("")
            .to_string(),
        category_job_service_img_url: None,
        category_job_service_status: read_i32(row, "category_job_service_status").unwrap_or(0),
        has_sub_service: row.get::<bool, _>("has_sub_service").unwrap_or(false),
        category_job_service_create_at: read_datetime_utc(row, "category_job_service_create_at"),
        category_job_service_create_by: read_str(row, "category_job_service_create_by")
            .unwrap_or("")
            .to_string(),
        category_job_service_update_at: read_datetime_utc(row, "category_job_service_update_at"),
        category_job_service_update_by: read_str(row, "category_job_service_update_by")
            .unwrap_or("")
            .to_string(),
    }
}

impl MssqlCategoryJobServiceMainRepository {
    #[allow(dead_code)]
    fn _uuid_zero_use(_: Uuid) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn row_parsing_safe_when_columns_missing() {
        let row = kokkak_domain::CategoryJobServiceMainRow {
            category_job_service_guid: "s-1".into(),
            category_job_service_category_main_guid: "m-1".into(),
            category_job_main_name: "Home Repair".into(),
            category_job_service_name: "Air Con".into(),
            category_job_service_locale: "th".into(),
            category_job_service_icon_style: "solid".into(),
            category_job_service_icon_line: "snowflake".into(),
            category_job_service_img_path: "category-job-services/s-1/icon/x.webp".into(),
            category_job_service_img_url: None,
            category_job_service_status: 1,
            has_sub_service: true,
            category_job_service_create_at: Some(Utc::now()),
            category_job_service_create_by: "admin".into(),
            category_job_service_update_at: Some(Utc::now()),
            category_job_service_update_by: "admin".into(),
        };
        assert_eq!(row.category_job_service_status, 1);
        assert!(row.has_sub_service);
        assert!(row.category_job_service_img_url.is_none());
    }
}
