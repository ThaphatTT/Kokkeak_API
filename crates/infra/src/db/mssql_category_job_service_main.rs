use async_trait::async_trait;
use chrono::{DateTime, Utc};
use tiberius::ToSql;

use kokkak_domain::category_job_service_main::CategoryJobServiceMainError;
use kokkak_domain::traits::category_job_service_main::CategoryJobServiceMainRepository;
use kokkak_domain::traits::user::RepoError;
use kokkak_domain::{
    CategoryJobServiceMainCreateInput, CategoryJobServiceMainCreateResult,
    CategoryJobServiceMainDeleteResult, CategoryJobServiceMainRow,
    CategoryJobServiceMainUpdateInput, CategoryJobServiceMainUpdateResult,
};

use crate::db::mssql::{exec_sp, read_guid_str, read_i32, read_str, MssqlPool};

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
        category_job_main_guid: &str,
        keyword: Option<&str>,
        include_inactive: bool,
    ) -> Result<Vec<CategoryJobServiceMainRow>, RepoError> {
        let main_guid = category_job_main_guid;
        let kw = keyword;
        let inactive = include_inactive;

        let params: &[&dyn ToSql] = &[&main_guid, &kw, &inactive];

        let rows = exec_sp(
            &self.pool,
            "EXEC dbo.SP_CATEGORY_JOB_SERVICE_MAIN_GET \
                    @p_category_job_main_guid = @P1, \
                    @p_keyword = @P2, \
                    @p_include_inactive = @P3",
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
        let name = input.category_job_service_name.as_str();
        let icon_style: Option<&str> = input.category_job_service_icon_style.as_deref();
        let icon_line: Option<&str> = input.category_job_service_icon_line.as_deref();
        let img_path: Option<&str> = input.category_job_service_img_path.as_deref();
        let create_by = input.create_by.as_str();

        let params: &[&dyn ToSql] = &[
            &main_guid,
            &name,
            &icon_style,
            &icon_line,
            &img_path,
            &create_by,
        ];

        let rows = exec_sp(
            &self.pool,
            "EXEC dbo.SP_CATEGORY_JOB_SERVICE_MAIN_CREATE \
                @p_category_job_main_guid = @P1, \
                @p_category_job_service_name = @P2, \
                @p_category_job_service_icon_style = @P3, \
                @p_category_job_service_icon_line = @P4, \
                @p_category_job_service_img_path = @P5, \
                @p_create_by = @P6",
            params,
        )
        .await?;

        let row = rows.first().ok_or_else(|| {
            RepoError::Backend(
                "SP_CATEGORY_JOB_SERVICE_MAIN_CREATE returned no row (driver/protocol mismatch)"
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
        let name = input.category_job_service_name.as_str();
        let icon_style: Option<&str> = input.category_job_service_icon_style.as_deref();
        let icon_line: Option<&str> = input.category_job_service_icon_line.as_deref();
        let img_path: Option<&str> = input.category_job_service_img_path.as_deref();
        let status = input.category_job_service_status;
        let update_by = input.update_by.as_str();

        let params: &[&dyn ToSql] = &[
            &service_guid,
            &main_guid,
            &name,
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
                @p_category_job_service_name = @P3, \
                @p_category_job_service_icon_style = @P4, \
                @p_category_job_service_icon_line = @P5, \
                @p_category_job_service_img_path = @P6, \
                @p_category_job_service_status = @P7, \
                @p_update_by = @P8",
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
        category_job_service_create_at: {
            row.get::<DateTime<Utc>, _>("category_job_service_create_at")
        },
        category_job_service_create_by: read_str(row, "category_job_service_create_by")
            .unwrap_or("")
            .to_string(),
        category_job_service_update_at: {
            row.get::<DateTime<Utc>, _>("category_job_service_update_at")
        },
        category_job_service_update_by: read_str(row, "category_job_service_update_by")
            .unwrap_or("")
            .to_string(),
    }
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
