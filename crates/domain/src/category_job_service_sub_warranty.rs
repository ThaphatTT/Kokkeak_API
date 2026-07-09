use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CategoryJobServiceSubWarrantyDetailRow {
    pub category_job_service_sub_warranty_guid: String,

    pub category_job_service_sub_warranty_description: String,

    pub category_job_service_sub_warranty_warranty_amount_day: i32,

    pub category_job_service_sub_warranty_status: i32,

    pub category_job_service_sub_warranty_icon: String,

    pub category_job_service_sub_warranty_create_at: Option<DateTime<Utc>>,

    pub category_job_service_sub_warranty_create_by: String,

    pub category_job_service_sub_warranty_update_at: Option<DateTime<Utc>>,

    pub category_job_service_sub_warranty_update_by: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CategoryJobServiceSubWarrantyListInput {
    pub category_job_service_sub_warranty_guid: Option<String>,

    pub keyword: Option<String>,

    pub status: Option<i32>,

    pub locale: Option<String>,

    pub page: Option<u32>,

    pub page_size: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CategoryJobServiceSubWarrantyPage {
    pub items: Vec<CategoryJobServiceSubWarrantyDetailRow>,

    pub total_count: i64,

    pub page: u32,

    pub page_size: u32,

    pub total_page: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("SP_CATEGORY_JOB_SERVICE_SUB_WARRANTY failed: {code} — {message}")]
pub struct CategoryJobServiceSubWarrantyError {
    pub code: String,

    pub message: String,
}

impl CategoryJobServiceSubWarrantyError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }

    pub const CODE_SUCCESS: &'static str = "INSERT_SUCCESS";

    pub const CODE_DUPLICATE: &'static str = "DUPLICATE_GUID";

    pub const CODE_INSERT_ERROR: &'static str = "INSERT_ERROR";

    pub const CODE_UPDATE_SUCCESS: &'static str = "UPDATE_SUCCESS";

    pub const CODE_GUID_REQUIRED: &'static str = "GUID_REQUIRED";

    pub const CODE_NOT_FOUND: &'static str = "NOT_FOUND";

    pub const CODE_UPDATE_ERROR: &'static str = "UPDATE_ERROR";

    pub const CODE_DELETE_SUCCESS: &'static str = "DELETE_SUCCESS";

    pub const CODE_DELETE_ERROR: &'static str = "DELETE_ERROR";
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CategoryJobServiceSubWarrantyCreateInput {
    #[serde(default)]
    pub category_job_service_sub_warranty_guid: Option<String>,

    pub category_job_service_sub_warranty_description_la: Option<String>,

    pub category_job_service_sub_warranty_description_en: Option<String>,

    pub category_job_service_sub_warranty_description_th: Option<String>,

    pub category_job_service_sub_warranty_description_zh: Option<String>,

    pub category_job_service_sub_warranty_warranty_amount_day: i32,

    pub category_job_service_sub_warranty_status: i32,

    pub category_job_service_sub_warranty_icon: Option<String>,

    pub create_by: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CategoryJobServiceSubWarrantyCreateResult {
    pub success: bool,

    pub code: String,

    pub message: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category_job_service_sub_warranty_guid: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CategoryJobServiceSubWarrantyUpdateInput {
    pub category_job_service_sub_warranty_guid: String,

    #[serde(default)]
    pub category_job_service_sub_warranty_description_la: Option<String>,

    #[serde(default)]
    pub category_job_service_sub_warranty_description_en: Option<String>,

    #[serde(default)]
    pub category_job_service_sub_warranty_description_th: Option<String>,

    #[serde(default)]
    pub category_job_service_sub_warranty_description_zh: Option<String>,

    #[serde(default)]
    pub category_job_service_sub_warranty_warranty_amount_day: Option<i32>,

    #[serde(default)]
    pub category_job_service_sub_warranty_status: Option<i32>,

    #[serde(default)]
    pub category_job_service_sub_warranty_icon: Option<String>,

    pub update_by: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CategoryJobServiceSubWarrantyUpdateResult {
    pub success: bool,

    pub code: String,

    pub message: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category_job_service_sub_warranty_guid: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CategoryJobServiceSubWarrantyDeleteInput {
    pub category_job_service_sub_warranty_guid: String,

    pub update_by: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CategoryJobServiceSubWarrantyDeleteResult {
    pub success: bool,

    pub code: String,

    pub message: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category_job_service_sub_warranty_guid: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn row_defaults_match_empty_strings() {
        let r = CategoryJobServiceSubWarrantyDetailRow {
            category_job_service_sub_warranty_guid: String::new(),
            category_job_service_sub_warranty_description: String::new(),
            category_job_service_sub_warranty_warranty_amount_day: 0,
            category_job_service_sub_warranty_status: 0,
            category_job_service_sub_warranty_icon: String::new(),
            category_job_service_sub_warranty_create_at: None,
            category_job_service_sub_warranty_create_by: String::new(),
            category_job_service_sub_warranty_update_at: None,
            category_job_service_sub_warranty_update_by: String::new(),
        };
        assert_eq!(r.category_job_service_sub_warranty_description, "");
        assert_eq!(r.category_job_service_sub_warranty_warranty_amount_day, 0);
        assert_eq!(r.category_job_service_sub_warranty_status, 0);
    }

    #[test]
    fn list_input_default_all_none() {
        let i = CategoryJobServiceSubWarrantyListInput::default();
        assert!(i.category_job_service_sub_warranty_guid.is_none());
        assert!(i.keyword.is_none());
        assert!(i.status.is_none());
        assert!(i.locale.is_none());
        assert!(i.page.is_none());
        assert!(i.page_size.is_none());
    }
}
