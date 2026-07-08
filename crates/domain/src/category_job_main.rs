use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CategoryJobMainRow {
    pub category_job_main_guid: String,

    pub category_job_main_name: String,

    #[serde(skip_serializing)]
    pub category_job_main_locale: String,

    pub category_job_main_icon_style: String,

    pub category_job_main_icon_line: String,

    #[serde(skip_serializing)]
    pub category_job_main_img_path: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub category_job_main_img_url: Option<String>,

    pub category_job_main_status: i32,

    pub category_job_main_priority: i32,

    #[serde(default)]
    pub has_sub_service: bool,

    pub category_job_main_create_at: Option<DateTime<Utc>>,

    pub category_job_main_create_by: String,

    pub category_job_main_update_at: Option<DateTime<Utc>>,

    pub category_job_main_update_by: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CategoryJobMainListInput {
    pub keyword: Option<String>,

    pub status: Option<i32>,

    pub locale: Option<String>,

    pub page: u32,

    pub page_size: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CategoryJobMainPage {
    pub items: Vec<CategoryJobMainRow>,

    pub total_count: i64,

    pub page: u32,

    pub page_size: u32,

    pub total_page: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CategoryJobMainCreateInput {
    #[serde(default)]
    pub category_job_main_name_la: Option<String>,

    #[serde(default)]
    pub category_job_main_name_en: Option<String>,

    #[serde(default)]
    pub category_job_main_name_th: Option<String>,

    #[serde(default)]
    pub category_job_main_name_zh: Option<String>,

    #[serde(default)]
    pub category_job_main_icon_style: Option<String>,

    #[serde(default)]
    pub category_job_main_icon_line: Option<String>,

    #[serde(default)]
    pub category_job_main_img_path: Option<String>,

    #[serde(default)]
    pub category_job_main_priority: Option<i32>,

    pub create_by: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CategoryJobMainUpdateInput {
    pub category_job_main_guid: String,

    pub category_job_main_name: String,

    #[serde(default)]
    pub category_job_main_icon_style: Option<String>,

    #[serde(default)]
    pub category_job_main_icon_line: Option<String>,

    #[serde(default)]
    pub category_job_main_img_path: Option<String>,

    pub category_job_main_status: i32,

    pub category_job_main_priority: i32,

    pub update_by: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CategoryJobMainCreateResult {
    pub success: bool,

    pub code: String,

    pub message: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category_job_main_guid: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CategoryJobMainUpdateResult {
    pub success: bool,

    pub code: String,

    pub message: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category_job_main_guid: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CategoryJobMainAutocompleteInput {
    pub keyword: Option<String>,

    pub status: Option<i32>,

    pub locale: Option<String>,

    pub take: Option<i32>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CategoryJobMainAutocompleteRow {
    pub category_job_main_guid: String,

    pub category_job_main_name: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CategoryJobMainDeleteResult {
    pub success: bool,

    pub code: String,

    pub message: String,

    pub category_job_main_guid: String,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("SP_CATEGORY_JOB_MAIN failed: {code} — {message}")]
pub struct CategoryJobMainError {
    pub code: String,

    pub message: String,
}

impl CategoryJobMainError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }

    pub const CODE_SUCCESS: &'static str = "SUCCESS";

    pub const CODE_NOT_FOUND: &'static str = "CATEGORY_NOT_FOUND";

    pub const CODE_DUPLICATE_NAME: &'static str = "CATEGORY_NAME_DUPLICATE";

    pub const CODE_HAS_DEPENDENTS: &'static str = "CATEGORY_HAS_DEPENDENTS";

    pub const CODE_INVALID_STATUS: &'static str = "INVALID_STATUS";

    pub const CODE_NAME_REQUIRED: &'static str = "NAME_REQUIRED";

    pub fn is_success_code(code: &str) -> bool {
        code == Self::CODE_SUCCESS
    }
}
