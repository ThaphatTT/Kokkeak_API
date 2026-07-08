use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CategoryJobServiceMainRow {
    pub category_job_service_guid: String,

    pub category_job_service_category_main_guid: String,

    pub category_job_main_name: String,

    pub category_job_service_name: String,

    #[serde(skip_serializing)]
    pub category_job_service_locale: String,

    pub category_job_service_icon_style: String,

    pub category_job_service_icon_line: String,

    #[serde(skip_serializing)]
    pub category_job_service_img_path: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub category_job_service_img_url: Option<String>,

    pub category_job_service_status: i32,

    pub has_sub_service: bool,

    pub category_job_service_create_at: Option<DateTime<Utc>>,

    pub category_job_service_create_by: String,

    pub category_job_service_update_at: Option<DateTime<Utc>>,

    pub category_job_service_update_by: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CategoryJobServiceMainListInput {
    #[serde(default)]
    pub category_job_main_guid: Option<String>,

    #[serde(default)]
    pub keyword: Option<String>,

    #[serde(default)]
    pub status: Option<i32>,

    #[serde(default)]
    pub locale: Option<String>,

    #[serde(default)]
    pub include_deleted: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CategoryJobServiceMainCreateInput {
    pub category_job_main_guid: String,

    #[serde(default)]
    pub category_job_service_name_la: Option<String>,

    #[serde(default)]
    pub category_job_service_name_en: Option<String>,

    #[serde(default)]
    pub category_job_service_name_th: Option<String>,

    #[serde(default)]
    pub category_job_service_name_zh: Option<String>,

    #[serde(default)]
    pub category_job_service_icon_style: Option<String>,

    #[serde(default)]
    pub category_job_service_icon_line: Option<String>,

    #[serde(default)]
    pub category_job_service_img_path: Option<String>,

    pub create_by: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CategoryJobServiceMainUpdateInput {
    pub category_job_service_guid: String,

    pub category_job_main_guid: String,

    pub category_job_service_name: String,

    #[serde(default)]
    pub category_job_service_icon_style: Option<String>,

    #[serde(default)]
    pub category_job_service_icon_line: Option<String>,

    #[serde(default)]
    pub category_job_service_img_path: Option<String>,

    pub category_job_service_status: i32,

    pub update_by: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CategoryJobServiceMainCreateResult {
    pub success: bool,

    pub code: String,

    pub message: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category_job_service_guid: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CategoryJobServiceMainUpdateResult {
    pub success: bool,

    pub code: String,

    pub message: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category_job_service_guid: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CategoryJobServiceMainDeleteResult {
    pub success: bool,

    pub code: String,

    pub message: String,

    pub category_job_service_guid: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CategoryJobServiceMainAutocompleteInput {
    pub category_job_main_guid: Option<String>,

    pub keyword: Option<String>,

    pub status: Option<i32>,

    pub locale: Option<String>,

    pub take: Option<i32>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CategoryJobServiceMainAutocompleteRow {
    pub category_job_service_guid: String,

    pub category_job_service_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("SP_CATEGORY_JOB_SERVICE_MAIN failed: {code} — {message}")]
pub struct CategoryJobServiceMainError {
    pub code: String,

    pub message: String,
}

impl CategoryJobServiceMainError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }

    pub const CODE_SUCCESS: &'static str = "SUCCESS";

    pub const CODE_NOT_FOUND: &'static str = "SERVICE_NOT_FOUND";

    pub const CODE_MAIN_NOT_FOUND: &'static str = "MAIN_NOT_FOUND";

    pub const CODE_INVALID_STATUS: &'static str = "INVALID_STATUS";

    pub const CODE_NAME_REQUIRED: &'static str = "NAME_REQUIRED";

    pub fn is_success_code(code: &str) -> bool {
        code == Self::CODE_SUCCESS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn row_defaults_are_zero_or_empty() {
        let r = CategoryJobServiceMainRow::default();
        assert_eq!(r.category_job_service_guid, "");
        assert_eq!(r.category_job_service_status, 0);
        assert_eq!(r.category_job_service_img_url, None);
        assert!(!r.has_sub_service);
    }

    #[test]
    fn create_input_carries_main_guid_and_actor() {
        let i = CategoryJobServiceMainCreateInput {
            category_job_main_guid: "m-1".into(),
            category_job_service_name_la: Some("Air Con Repair".into()),
            category_job_service_name_en: Some("Air Con Repair".into()),
            category_job_service_name_th: Some("Air Con Repair".into()),
            category_job_service_name_zh: Some("Air Con Repair".into()),
            category_job_service_icon_style: Some("solid".into()),
            category_job_service_icon_line: Some("snowflake".into()),
            category_job_service_img_path: Some("category-job-services/m-1/icon/x.webp".into()),
            create_by: "admin-1".into(),
        };
        assert_eq!(i.category_job_main_guid, "m-1");
        assert_eq!(i.create_by, "admin-1");
    }

    #[test]
    fn error_codes_are_stable_strings() {
        assert_eq!(CategoryJobServiceMainError::CODE_SUCCESS, "SUCCESS");
        assert_eq!(
            CategoryJobServiceMainError::CODE_NOT_FOUND,
            "SERVICE_NOT_FOUND"
        );
        assert_eq!(
            CategoryJobServiceMainError::CODE_MAIN_NOT_FOUND,
            "MAIN_NOT_FOUND"
        );
        assert!(CategoryJobServiceMainError::is_success_code("SUCCESS"));
        assert!(!CategoryJobServiceMainError::is_success_code("OTHER"));
    }
}
