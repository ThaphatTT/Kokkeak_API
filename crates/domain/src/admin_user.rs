

use rust_decimal::Decimal;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdminInsertUserResult {

    pub user_guid: String,

    pub user_username_guid: String,

    pub username: String,

    pub assigned_role_guid: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("SP_USER_INSERT_FULL failed: {code} — {message}")]
pub struct AdminInsertUserError {

    pub code: String,

    pub message: String,
}

impl AdminInsertUserError {

    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdminUpdateUserResult {

    pub user_guid: String,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("SP_USER_UPDATE_FULL failed: {code} — {message}")]
pub struct AdminUpdateUserError {

    pub code: String,

    pub message: String,
}

impl AdminUpdateUserError {

    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct DaySchedule {

    pub is_working: bool,

    pub start_time: Option<chrono::NaiveTime>,

    pub end_time: Option<chrono::NaiveTime>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct WeeklySchedule {

    pub monday: DaySchedule,

    pub tuesday: DaySchedule,

    pub wednesday: DaySchedule,

    pub thursday: DaySchedule,

    pub friday: DaySchedule,

    pub saturday: DaySchedule,

    pub sunday: DaySchedule,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[allow(missing_docs)]
pub struct AdminInsertUserRequest {

    pub actor_user_username_guid: String,

    pub user_guid: Option<String>,

    pub first_name: String,

    pub last_name: String,

    pub id_card: Option<String>,

    pub tel: Option<String>,

    pub email: String,

    pub gender: Option<String>,

    pub country_guid: Option<String>,

    pub province: Option<String>,

    pub district: Option<String>,

    pub sub_district: Option<String>,

    pub village: Option<String>,

    pub post: Option<String>,

    pub description: Option<String>,

    pub is_foreign: bool,

    pub is_customer_company: bool,

    pub is_customer: bool,

    pub is_admin: bool,

    pub is_employee: bool,

    pub is_freelance: bool,

    pub status: i32,

    pub username: String,

    pub password_hash: String,

    pub profile_img_path: Option<String>,

    pub company_guid: Option<String>,

    pub company_name: Option<String>,

    pub company_tel: Option<String>,

    pub company_type: Option<i32>,

    pub company_status: i32,

    pub department_guid: Option<String>,

    pub department_team_guid: Option<String>,

    pub position_guid: Option<String>,

    pub position_start_at: Option<chrono::DateTime<chrono::Utc>>,

    pub salary_amount: Option<Decimal>,

    pub salary_currency: Option<String>,

    pub schedule: WeeklySchedule,

    pub bank_name: Option<String>,

    pub bank_code: Option<String>,

    pub bank_account_no: Option<String>,

    pub bank_account_name: Option<String>,

    pub bank_book_img_path: Option<String>,

    pub id_card_front_path: Option<String>,

    pub id_card_back_path: Option<String>,

    pub proof_of_address_path: Option<String>,

    pub source_of_funds_statement_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(missing_docs)]
pub struct AdminUpdateUserRequest {

    pub actor_user_username_guid: String,

    pub user_guid: String,

    pub first_name: String,

    pub last_name: String,

    pub id_card: Option<String>,

    pub tel: Option<String>,

    pub email: String,

    pub gender: Option<String>,

    pub country_guid: Option<String>,

    pub province: Option<String>,

    pub district: Option<String>,

    pub sub_district: Option<String>,

    pub village: Option<String>,

    pub post: Option<String>,

    pub description: Option<String>,

    pub is_foreign: bool,

    pub is_customer_company: bool,

    pub is_customer: bool,

    pub is_admin: bool,

    pub is_employee: bool,

    pub is_freelance: bool,

    pub status: i32,

    pub username: String,

    pub profile_img_path: Option<String>,

    pub company_guid: Option<String>,

    pub company_name: Option<String>,

    pub company_tel: Option<String>,

    pub company_type: Option<i32>,

    pub company_status: i32,

    pub department_guid: Option<String>,

    pub department_team_guid: Option<String>,

    pub position_guid: Option<String>,

    pub position_start_at: Option<chrono::DateTime<chrono::Utc>>,

    pub salary_amount: Option<Decimal>,

    pub salary_currency: Option<String>,

    pub schedule: WeeklySchedule,

    pub bank_name: Option<String>,

    pub bank_code: Option<String>,

    pub bank_account_no: Option<String>,

    pub bank_account_name: Option<String>,

    pub bank_book_img_path: Option<String>,

    pub id_card_front_path: Option<String>,

    pub id_card_back_path: Option<String>,

    pub proof_of_address_path: Option<String>,

    pub source_of_funds_statement_path: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[allow(missing_docs)]
pub struct AdminUserListPagingInput {

    pub keyword: String,

    pub user_status: Option<i32>,

    pub user_is_customer: Option<bool>,

    pub user_is_employee: Option<bool>,

    pub user_is_freelance: Option<bool>,

    pub department_guid: Option<String>,

    pub department_team_guid: Option<String>,

    pub position_guid: Option<String>,

    pub page: u32,

    pub page_size: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct UserListPagingRow {

    pub total_count: i64,

    pub page: i32,

    pub page_size: i32,

    pub user_guid: String,

    pub full_name: String,

    pub phone: String,

    pub user_status: i32,

    pub user_status_name: String,

    pub user_is_customer: bool,

    pub user_is_employee: bool,

    pub user_is_freelance: bool,

    pub role_name: String,

    pub department_guid: String,

    pub department_name: String,

    pub department_team_guid: String,

    pub department_team_name: String,

    pub position_guid: String,

    pub position_name: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AdminUserListPagingPage {

    pub items: Vec<UserListPagingRow>,

    pub total_count: i64,

    pub page: i32,

    pub page_size: i32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[allow(missing_docs)]
pub struct AdminUserDetailProfileImage {

    pub user_img_profile_guid: String,

    pub profile_img_path: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_img_url: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[allow(missing_docs)]
pub struct AdminUserDetailCompany {

    pub user_company_guid: String,

    pub company_guid: String,

    pub company_name: String,

    pub company_tel: String,

    pub user_company_name: String,

    pub user_company_tel: String,

    pub user_company_type: i32,

    pub user_company_status: i32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[allow(missing_docs)]
pub struct AdminUserDetailRoles {

    pub role_codes: String,

    pub role_names: String,

    pub user_is_admin: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[allow(missing_docs)]
pub struct AdminUserDetailScope {

    pub department_guid: String,

    pub department_code: String,

    pub department_name: String,

    pub department_team_guid: String,

    pub department_team_code: String,

    pub department_team_name: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[allow(missing_docs)]
pub struct AdminUserDetailPosition {

    pub user_position_guid: String,

    pub position_guid: String,

    pub position_code: String,

    pub position_name: String,

    pub position_level: i32,

    pub position_start_at: Option<chrono::DateTime<chrono::Utc>>,

    pub position_end_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[allow(missing_docs)]
pub struct AdminUserDetailSalary {

    pub user_salary_guid: String,

    pub salary_amount: Decimal,

    pub salary_currency: String,

    pub salary_type: i32,

    pub salary_effective_from: Option<chrono::DateTime<chrono::Utc>>,

    pub salary_effective_to: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[allow(missing_docs)]
pub struct AdminUserDetailBankAccount {

    pub user_bank_account_guid: String,

    pub bank_name: String,

    pub bank_code: String,

    pub branch_name: String,

    pub bank_account_name: String,

    pub bank_account_no: String,

    pub bank_account_no_masked: String,

    pub bank_account_type: i32,

    pub bank_account_is_default: bool,

    pub bank_account_verified_status: i32,

    pub bank_book_img_path: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub bank_book_img_url: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[allow(missing_docs)]
pub struct AdminUserDetailAttachment {

    pub user_details_attachment_guid: String,

    pub attachment_path: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub attachment_url: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[allow(missing_docs)]
pub struct AdminUserDetailUsername {

    pub user_username_guid: String,

    pub username: String,

    pub status: i32,

    pub created_at: Option<chrono::DateTime<chrono::Utc>>,

    pub updated_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[allow(missing_docs)]
pub struct AdminUserDetailCountry {

    pub country_guid: String,

    pub country_code: String,

    pub country_name: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[allow(missing_docs)]
pub struct AdminUserDetail {

    pub user_guid: String,
    pub user_first_name: String,
    pub user_last_name: String,

    pub full_name: String,
    pub user_id_card: String,
    pub user_tel: String,
    pub user_email: String,
    pub user_gender: String,

    pub user_is_foreign: bool,
    pub user_country_guid: String,

    pub user_province: String,
    pub user_district: String,
    pub user_sub_district: String,
    pub user_village: String,
    pub user_post: String,
    pub user_description: String,

    pub user_is_customer_company: bool,
    pub user_is_customer: bool,
    pub user_is_employee: bool,
    pub user_is_freelance: bool,

    pub user_is_admin: bool,

    pub user_status: i32,

    pub user_status_name: String,

    pub user_create_at: Option<chrono::DateTime<chrono::Utc>>,
    pub user_create_by: String,
    pub user_update_at: Option<chrono::DateTime<chrono::Utc>>,
    pub user_update_by: String,

    pub username: Option<AdminUserDetailUsername>,

    pub profile_image: Option<AdminUserDetailProfileImage>,

    pub country: Option<AdminUserDetailCountry>,

    pub company: Option<AdminUserDetailCompany>,

    pub roles: Option<AdminUserDetailRoles>,

    pub scope: Option<AdminUserDetailScope>,

    pub position: Option<AdminUserDetailPosition>,

    pub salary: Option<AdminUserDetailSalary>,

    pub schedule: Option<WeeklySchedule>,

    pub user_work_day_template_guid: String,

    pub bank_account: Option<AdminUserDetailBankAccount>,

    pub id_card_front: Option<AdminUserDetailAttachment>,
    pub id_card_back: Option<AdminUserDetailAttachment>,
    pub proof_of_address: Option<AdminUserDetailAttachment>,
    pub source_of_funds_statement: Option<AdminUserDetailAttachment>,
}
