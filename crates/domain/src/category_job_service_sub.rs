use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CategoryJobServiceSubRow {
    pub category_job_service_sub_guid: String,

    pub category_job_service_sub_category_job_service_main_guid: String,

    pub category_job_service_name: String,

    pub category_job_service_sub_name: String,

    pub category_job_service_sub_start_price: Decimal,

    pub category_job_service_sub_description: String,

    pub category_job_service_sub_status: i32,

    pub category_job_service_sub_create_at: Option<DateTime<Utc>>,

    pub category_job_service_sub_create_by: String,

    pub category_job_service_sub_update_at: Option<DateTime<Utc>>,

    pub category_job_service_sub_update_by: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CategoryJobServiceSubImageRow {
    pub category_job_service_sub_img_guid: String,

    pub category_job_service_sub_img_category_job_service_sub_guid: String,

    pub category_job_service_sub_img_type: i32,

    pub category_job_service_sub_img_priority: i32,

    pub category_job_service_sub_img_path: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub category_job_service_sub_img_url: Option<String>,

    pub category_job_service_sub_img_status: i32,

    pub category_job_service_sub_img_create_at: Option<DateTime<Utc>>,

    pub category_job_service_sub_img_create_by: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CategoryJobServiceSubFeeRow {
    pub category_job_service_sub_fee_guid: String,

    pub category_job_service_sub_fee_category_job_service_sub_guid: String,

    pub category_job_service_sub_fee_name: String,

    pub category_job_service_sub_fee_price: Decimal,

    pub category_job_service_sub_fee_status: i32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CategoryJobServiceSubWarrantyRow {
    pub category_job_service_sub_warranty_guid: String,

    pub category_job_service_sub_warranty_category_job_service_sub_guid: String,

    pub category_job_service_sub_warranty_name: String,

    pub category_job_service_sub_warranty_day: i32,

    pub category_job_service_sub_warranty_status: i32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CategoryJobServiceSubDetailBundle {
    pub sub: CategoryJobServiceSubRow,

    pub images: Vec<CategoryJobServiceSubImageRow>,

    pub fees: Vec<CategoryJobServiceSubFeeRow>,

    pub warranties: Vec<CategoryJobServiceSubWarrantyRow>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CategoryJobServiceSubImageInput {
    pub img_b64: String,

    #[serde(default)]
    pub img_type: Option<i32>,

    #[serde(default)]
    pub img_priority: Option<i32>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CategoryJobServiceSubCreateInput {
    pub category_job_service_guid: String,

    pub category_job_service_sub_name: String,

    pub category_job_service_sub_start_price: Decimal,

    pub category_job_service_sub_description: String,

    pub create_by: String,

    pub images: Vec<CategoryJobServiceSubImageInput>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CategoryJobServiceSubUpdateInput {
    pub category_job_service_sub_guid: String,

    pub category_job_service_guid: String,

    pub category_job_service_sub_name: String,

    pub category_job_service_sub_start_price: Decimal,

    pub category_job_service_sub_description: String,

    pub category_job_service_sub_status: i32,

    pub update_by: String,

    pub images: Vec<CategoryJobServiceSubImageInput>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CategoryJobServiceSubCreateResult {
    pub success: bool,

    pub code: String,

    pub message: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category_job_service_sub_guid: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CategoryJobServiceSubUpdateResult {
    pub success: bool,

    pub code: String,

    pub message: String,

    pub category_job_service_sub_guid: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CategoryJobServiceSubDeleteResult {
    pub success: bool,

    pub code: String,

    pub message: String,

    pub category_job_service_sub_guid: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CategoryJobServiceSubImageCreateInput {
    pub category_job_service_sub_guid: String,

    pub img_type: i32,

    pub img_priority: i32,

    pub img_path: String,

    pub create_by: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CategoryJobServiceSubImageCreateResult {
    pub success: bool,

    pub code: String,

    pub message: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category_job_service_sub_img_guid: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CategoryJobServiceSubImageDeleteInput {
    pub category_job_service_sub_img_guid: String,

    pub update_by: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CategoryJobServiceSubImageDeleteResult {
    pub success: bool,

    pub code: String,

    pub message: String,

    pub category_job_service_sub_img_guid: String,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("SP_CATEGORY_JOB_SERVICE_SUB failed: {code} — {message}")]
pub struct CategoryJobServiceSubError {
    pub code: String,

    pub message: String,
}

impl CategoryJobServiceSubError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }

    pub const CODE_SUCCESS: &'static str = "SUCCESS";

    pub const CODE_NOT_FOUND: &'static str = "SUB_NOT_FOUND";

    pub const CODE_MAIN_NOT_FOUND: &'static str = "SERVICE_NOT_FOUND";

    pub const CODE_NAME_REQUIRED: &'static str = "NAME_REQUIRED";

    pub const CODE_INVALID_STATUS: &'static str = "INVALID_STATUS";

    pub fn is_success_code(code: &str) -> bool {
        code == Self::CODE_SUCCESS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn row_defaults_are_zero_or_empty() {
        let r = CategoryJobServiceSubRow::default();
        assert_eq!(r.category_job_service_sub_guid, "");
        assert_eq!(r.category_job_service_sub_status, 0);
        assert_eq!(r.category_job_service_sub_start_price, Decimal::ZERO);
    }

    #[test]
    fn image_input_carries_b64_type_priority() {
        let i = CategoryJobServiceSubImageInput {
            img_b64: "data:image/png;base64,xxx".into(),
            img_type: Some(2),
            img_priority: Some(0),
        };
        assert_eq!(i.img_b64, "data:image/png;base64,xxx");
        assert_eq!(i.img_type, Some(2));
        assert_eq!(i.img_priority, Some(0));
    }

    #[test]
    fn detail_bundle_default_is_empty_collections() {
        let b = CategoryJobServiceSubDetailBundle::default();
        assert!(b.images.is_empty());
        assert!(b.fees.is_empty());
        assert!(b.warranties.is_empty());
    }

    #[test]
    fn error_codes_are_stable_strings() {
        assert_eq!(CategoryJobServiceSubError::CODE_SUCCESS, "SUCCESS");
        assert_eq!(CategoryJobServiceSubError::CODE_NOT_FOUND, "SUB_NOT_FOUND");
        assert_eq!(
            CategoryJobServiceSubError::CODE_MAIN_NOT_FOUND,
            "SERVICE_NOT_FOUND"
        );
        assert!(CategoryJobServiceSubError::is_success_code("SUCCESS"));
        assert!(!CategoryJobServiceSubError::is_success_code("OTHER"));
    }
}
