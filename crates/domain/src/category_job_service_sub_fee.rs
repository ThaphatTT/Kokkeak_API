use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CategoryJobServiceSubFeeAdminRow {
    pub category_job_service_sub_fee_guid: String,

    pub category_job_service_sub_fee_header: String,

    pub category_job_service_sub_fee_description: String,

    pub category_job_service_sub_fee_price: Decimal,

    pub category_job_service_sub_fee_status: i32,

    pub category_job_service_sub_fee_icon: String,

    pub category_job_service_sub_fee_create_at: Option<DateTime<Utc>>,

    pub category_job_service_sub_fee_create_by: String,

    pub category_job_service_sub_fee_update_at: Option<DateTime<Utc>>,

    pub category_job_service_sub_fee_update_by: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CategoryJobServiceSubFeeListInput {
    pub category_job_service_sub_fee_guid: Option<String>,

    pub keyword: Option<String>,

    pub status: Option<i32>,

    pub locale: Option<String>,

    pub include_deleted: Option<bool>,

    pub page: Option<u32>,

    pub page_size: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CategoryJobServiceSubFeePage {
    pub items: Vec<CategoryJobServiceSubFeeAdminRow>,

    pub total_count: i64,

    pub page: u32,

    pub page_size: u32,

    pub total_page: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("SP_CATEGORY_JOB_SERVICE_SUB_FEE failed: {code} — {message}")]
pub struct CategoryJobServiceSubFeeError {
    pub code: String,

    pub message: String,
}

impl CategoryJobServiceSubFeeError {
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

    pub const CODE_INVALID_STATUS: &'static str = "INVALID_STATUS";

    pub const CODE_INVALID_PRICE: &'static str = "INVALID_PRICE";

    pub const CODE_PRICE_OUT_OF_RANGE: &'static str = "PRICE_OUT_OF_RANGE";

    pub const CODE_HEADER_TOO_LONG: &'static str = "HEADER_TOO_LONG";

    pub const CODE_UPDATE_ERROR: &'static str = "UPDATE_ERROR";

    pub const CODE_DELETE_SUCCESS: &'static str = "DELETE_SUCCESS";

    pub const CODE_DELETE_ERROR: &'static str = "DELETE_ERROR";
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CategoryJobServiceSubFeeCreateInput {
    #[serde(default)]
    pub category_job_service_sub_fee_guid: Option<String>,

    pub category_job_service_sub_fee_header_la: Option<String>,

    pub category_job_service_sub_fee_description_la: Option<String>,

    pub category_job_service_sub_fee_header_en: Option<String>,

    pub category_job_service_sub_fee_description_en: Option<String>,

    pub category_job_service_sub_fee_header_th: Option<String>,

    pub category_job_service_sub_fee_description_th: Option<String>,

    pub category_job_service_sub_fee_header_zh: Option<String>,

    pub category_job_service_sub_fee_description_zh: Option<String>,

    pub category_job_service_sub_fee_price: Decimal,

    pub category_job_service_sub_fee_status: i32,

    pub category_job_service_sub_fee_icon: Option<String>,

    pub create_by: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CategoryJobServiceSubFeeCreateResult {
    pub success: bool,

    pub code: String,

    pub message: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category_job_service_sub_fee_guid: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CategoryJobServiceSubFeeUpdateInput {
    pub category_job_service_sub_fee_guid: String,

    #[serde(default)]
    pub category_job_service_sub_fee_header_la: Option<String>,

    #[serde(default)]
    pub category_job_service_sub_fee_description_la: Option<String>,

    #[serde(default)]
    pub category_job_service_sub_fee_header_en: Option<String>,

    #[serde(default)]
    pub category_job_service_sub_fee_description_en: Option<String>,

    #[serde(default)]
    pub category_job_service_sub_fee_header_th: Option<String>,

    #[serde(default)]
    pub category_job_service_sub_fee_description_th: Option<String>,

    #[serde(default)]
    pub category_job_service_sub_fee_header_zh: Option<String>,

    #[serde(default)]
    pub category_job_service_sub_fee_description_zh: Option<String>,

    #[serde(default)]
    pub category_job_service_sub_fee_price: Option<Decimal>,

    #[serde(default)]
    pub category_job_service_sub_fee_status: Option<i32>,

    #[serde(default)]
    pub category_job_service_sub_fee_icon: Option<String>,

    pub update_by: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CategoryJobServiceSubFeeUpdateResult {
    pub success: bool,

    pub code: String,

    pub message: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category_job_service_sub_fee_guid: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CategoryJobServiceSubFeeDeleteInput {
    pub category_job_service_sub_fee_guid: String,

    pub update_by: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CategoryJobServiceSubFeeDeleteResult {
    pub success: bool,

    pub code: String,

    pub message: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category_job_service_sub_fee_guid: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CategoryJobServiceSubFeeAutocompleteInput {
    pub category_job_service_sub_fee_guid: Option<String>,

    pub keyword: Option<String>,

    pub status: Option<i32>,

    pub locale: Option<String>,

    pub limit: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CategoryJobServiceSubFeeAutocompleteRow {
    pub category_job_service_sub_fee_guid: String,

    pub category_job_service_sub_fee_header: String,

    pub category_job_service_sub_fee_description: String,

    pub category_job_service_sub_fee_price: Decimal,

    pub category_job_service_sub_fee_status: i32,

    pub category_job_service_sub_fee_icon: String,

    pub value: String,

    pub label: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn row_defaults_match_empty_strings_and_zero_decimal() {
        let r = CategoryJobServiceSubFeeAdminRow {
            category_job_service_sub_fee_guid: String::new(),
            category_job_service_sub_fee_header: String::new(),
            category_job_service_sub_fee_description: String::new(),
            category_job_service_sub_fee_price: Decimal::ZERO,
            category_job_service_sub_fee_status: 0,
            category_job_service_sub_fee_icon: String::new(),
            category_job_service_sub_fee_create_at: None,
            category_job_service_sub_fee_create_by: String::new(),
            category_job_service_sub_fee_update_at: None,
            category_job_service_sub_fee_update_by: String::new(),
        };
        assert_eq!(r.category_job_service_sub_fee_guid, "");
        assert_eq!(r.category_job_service_sub_fee_price, Decimal::ZERO);
        assert_eq!(r.category_job_service_sub_fee_status, 0);
    }

    #[test]
    fn list_input_default_all_none() {
        let i = CategoryJobServiceSubFeeListInput::default();
        assert!(i.category_job_service_sub_fee_guid.is_none());
        assert!(i.keyword.is_none());
        assert!(i.status.is_none());
        assert!(i.locale.is_none());
        assert!(i.include_deleted.is_none());
        assert!(i.page.is_none());
        assert!(i.page_size.is_none());
    }

    #[test]
    fn page_carries_total_and_paging() {
        let p = CategoryJobServiceSubFeePage {
            items: vec![],
            total_count: 42,
            page: 2,
            page_size: 20,
            total_page: 3,
        };
        assert_eq!(p.total_count, 42);
        assert_eq!(p.page, 2);
        assert_eq!(p.total_page, 3);
    }

    #[test]
    fn error_construction_keeps_code_and_message() {
        let e = CategoryJobServiceSubFeeError::new("E_FEE_NOT_FOUND", "fee missing");
        assert_eq!(e.code, "E_FEE_NOT_FOUND");
        assert_eq!(e.message, "fee missing");
    }
}
