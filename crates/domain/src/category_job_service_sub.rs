use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CategoryJobServiceSubRow {
    pub category_job_service_sub_guid: String,

    pub category_job_service_sub_category_job_service_main_guid: String,

    pub category_job_service_sub_category_job_service_sub_fee_guid: String,

    pub category_job_service_sub_category_job_service_sub_warranty_guid: String,

    pub category_job_service_name: String,

    pub category_job_service_sub_name: String,

    #[serde(skip_serializing)]
    pub category_job_service_sub_locale: String,

    pub category_job_service_sub_start_price: Decimal,

    pub category_job_service_sub_description: String,

    pub category_job_service_sub_status: i32,

    #[serde(default, skip_serializing)]
    pub main_img_path: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub main_img_url: Option<String>,

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

    pub category_job_service_sub_img_type_language: i32,

    pub category_job_service_sub_img_priority: i32,

    #[serde(skip_serializing)]
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
pub struct CategoryJobServiceSubDetailRow {
    pub category_job_service_guid: String,

    pub category_job_service_sub_guid: String,

    pub category_job_service_sub_category_job_service_main_guid: String,

    pub category_job_service_sub_name_la: String,

    pub category_job_service_sub_name_en: String,

    pub category_job_service_sub_name_th: String,

    pub category_job_service_sub_name_zh: String,

    pub category_job_service_sub_description_la: String,

    pub category_job_service_sub_description_en: String,

    pub category_job_service_sub_description_th: String,

    pub category_job_service_sub_description_zh: String,

    pub category_job_service_sub_start_price: Decimal,

    pub category_job_service_sub_status: i32,

    pub category_job_service_sub_create_at: Option<DateTime<Utc>>,

    pub category_job_service_sub_create_by: String,

    pub category_job_service_sub_update_at: Option<DateTime<Utc>>,

    pub category_job_service_sub_update_by: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CategoryJobServiceSubDetailWarrantyRow {
    pub category_job_service_sub_warranty_guid: String,

    pub category_job_service_sub_warranty_map_sort_order: i32,

    pub category_job_service_sub_warranty_description: String,

    pub category_job_service_sub_warranty_locale: String,

    pub category_job_service_sub_warranty_warranty_amount_day: i32,

    pub category_job_service_sub_warranty_icon: String,

    pub category_job_service_sub_warranty_status: i32,

    pub category_job_service_sub_warranty_map_create_at: Option<DateTime<Utc>>,

    pub category_job_service_sub_warranty_map_create_by: String,

    pub category_job_service_sub_warranty_map_update_at: Option<DateTime<Utc>>,

    pub category_job_service_sub_warranty_map_update_by: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CategoryJobServiceSubDetailFeeRow {
    pub category_job_service_sub_fee_guid: String,

    pub category_job_service_sub_fee_map_sort_order: i32,

    pub category_job_service_sub_fee_header: String,

    pub category_job_service_sub_fee_description: String,

    pub category_job_service_sub_fee_locale: String,

    pub category_job_service_sub_fee_icon: String,

    pub category_job_service_sub_fee_price: Decimal,

    pub category_job_service_sub_fee_status: i32,

    pub category_job_service_sub_fee_map_create_at: Option<DateTime<Utc>>,

    pub category_job_service_sub_fee_map_create_by: String,

    pub category_job_service_sub_fee_map_update_at: Option<DateTime<Utc>>,

    pub category_job_service_sub_fee_map_update_by: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CategoryJobServiceSubDetailImageRow {
    pub category_job_service_sub_img_guid: String,

    pub category_job_service_sub_img_category_job_service_sub_guid: String,

    pub category_job_service_sub_img_type: i32,

    pub category_job_service_sub_img_type_language: i32,

    pub category_job_service_sub_img_priority: i32,

    #[serde(skip_serializing)]
    pub category_job_service_sub_img_img_path: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub category_job_service_sub_img_url: Option<String>,

    pub category_job_service_sub_img_status: i32,

    pub category_job_service_sub_img_create_at: Option<DateTime<Utc>>,

    pub category_job_service_sub_img_create_by: String,

    pub category_job_service_sub_img_update_at: Option<DateTime<Utc>>,

    pub category_job_service_sub_img_update_by: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CategoryJobServiceSubDetailBundle {
    pub sub: CategoryJobServiceSubDetailRow,

    pub warranties: Vec<CategoryJobServiceSubDetailWarrantyRow>,

    pub fees: Vec<CategoryJobServiceSubDetailFeeRow>,

    pub images: Vec<CategoryJobServiceSubDetailImageRow>,
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
pub struct CategoryJobServiceSubUpdateSpInput {
    pub category_job_service_sub_guid: String,

    #[serde(default)]
    pub category_job_service_main_guid: Option<String>,

    #[serde(default)]
    pub category_job_service_sub_name_la: Option<String>,

    #[serde(default)]
    pub category_job_service_sub_name_en: Option<String>,

    #[serde(default)]
    pub category_job_service_sub_name_th: Option<String>,

    #[serde(default)]
    pub category_job_service_sub_name_zh: Option<String>,

    #[serde(default)]
    pub category_job_service_sub_start_price: Option<Decimal>,

    #[serde(default)]
    pub category_job_service_sub_description_la: Option<String>,

    #[serde(default)]
    pub category_job_service_sub_description_en: Option<String>,

    #[serde(default)]
    pub category_job_service_sub_description_th: Option<String>,

    #[serde(default)]
    pub category_job_service_sub_description_zh: Option<String>,

    #[serde(default)]
    pub category_job_service_sub_status: Option<i32>,

    #[serde(default)]
    pub warranties: Vec<CategoryJobServiceSubCreateSpWarrantyInput>,

    #[serde(default)]
    pub fees: Vec<CategoryJobServiceSubCreateSpFeeInput>,

    #[serde(default)]
    pub images: Vec<CategoryJobServiceSubCreateSpImageInput>,

    #[serde(default = "default_replace_images")]
    pub replace_images: bool,

    pub update_by: String,
}

fn default_replace_images() -> bool {
    true
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CategoryJobServiceSubUpdateSpResult {
    pub success: bool,

    pub code: String,

    pub message: String,

    pub category_job_service_sub_guid: String,

    #[serde(default)]
    pub warranty_count: i32,

    #[serde(default)]
    pub fee_count: i32,

    #[serde(default)]
    pub image_count: i32,
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

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CategoryJobServiceSubCreateSpImageInput {
    pub img_path: String,

    #[serde(default)]
    pub img_type: Option<i32>,

    #[serde(default)]
    pub img_type_language: Option<i32>,

    #[serde(default)]
    pub priority: Option<i32>,

    #[serde(default, rename = "status")]
    pub img_status: Option<i32>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CategoryJobServiceSubCreateSpWarrantyInput {
    pub guid: String,

    pub sort_order: i32,

    #[serde(default, rename = "status")]
    pub map_status: Option<i32>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CategoryJobServiceSubCreateSpFeeInput {
    pub guid: String,

    pub sort_order: i32,

    #[serde(default, rename = "status")]
    pub map_status: Option<i32>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CategoryJobServiceSubCreateSpInput {
    #[serde(default)]
    pub category_job_service_sub_guid: Option<String>,

    pub category_job_service_main_guid: String,

    #[serde(default)]
    pub category_job_service_sub_name_la: Option<String>,

    #[serde(default)]
    pub category_job_service_sub_name_en: Option<String>,

    #[serde(default)]
    pub category_job_service_sub_name_th: Option<String>,

    #[serde(default)]
    pub category_job_service_sub_name_zh: Option<String>,

    pub category_job_service_sub_start_price: Decimal,

    #[serde(default)]
    pub category_job_service_sub_description_la: Option<String>,

    #[serde(default)]
    pub category_job_service_sub_description_en: Option<String>,

    #[serde(default)]
    pub category_job_service_sub_description_th: Option<String>,

    #[serde(default)]
    pub category_job_service_sub_description_zh: Option<String>,

    #[serde(default = "default_status")]
    pub category_job_service_sub_status: i32,

    #[serde(default)]
    pub warranties: Vec<CategoryJobServiceSubCreateSpWarrantyInput>,

    #[serde(default)]
    pub fees: Vec<CategoryJobServiceSubCreateSpFeeInput>,

    #[serde(default)]
    pub images: Vec<CategoryJobServiceSubCreateSpImageInput>,

    pub create_by: String,
}

fn default_status() -> i32 {
    1
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CategoryJobServiceSubCreateSpResult {
    pub success: bool,

    pub code: String,

    pub message: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category_job_service_sub_guid: Option<String>,

    #[serde(default)]
    pub warranty_count: i32,

    #[serde(default)]
    pub fee_count: i32,

    #[serde(default)]
    pub image_count: i32,
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
        assert_eq!(r.category_job_service_sub_locale, "");
        assert_eq!(r.main_img_path, "");
        assert!(r.main_img_url.is_none());
        assert_eq!(
            r.category_job_service_sub_category_job_service_sub_fee_guid,
            ""
        );
        assert_eq!(
            r.category_job_service_sub_category_job_service_sub_warranty_guid,
            ""
        );
    }

    #[test]
    fn image_row_default_includes_language() {
        let r = CategoryJobServiceSubImageRow::default();
        assert_eq!(r.category_job_service_sub_img_type_language, 0);
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

    #[test]
    fn create_sp_input_default() {
        let i = CategoryJobServiceSubCreateSpInput::default();
        assert!(i.category_job_service_sub_guid.is_none());
        assert!(i.warranties.is_empty());
        assert!(i.fees.is_empty());
        assert!(i.images.is_empty());
        assert_eq!(i.category_job_service_sub_status, 1);
    }

    #[test]
    fn create_sp_warranty_input_default() {
        let w = CategoryJobServiceSubCreateSpWarrantyInput::default();
        assert!(w.guid.is_empty());
        assert_eq!(w.sort_order, 0);
        assert!(w.map_status.is_none());
    }
}
