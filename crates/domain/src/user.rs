

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum Role {

    Customer,

    Technician,

    Admin,

    SuperAdmin,
}

impl Role {

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Customer => "customer",
            Self::Technician => "technician",
            Self::Admin => "admin",
            Self::SuperAdmin => "super_admin",
        }
    }

    pub fn from_code(s: &str) -> Option<Self> {
        let lower = s.to_ascii_lowercase();
        match lower.as_str() {
            "customer" => Some(Self::Customer),
            "technician" => Some(Self::Technician),
            "admin" => Some(Self::Admin),
            "super_admin" => Some(Self::SuperAdmin),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Permission {

    #[serde(rename = "DASHBOARD_VIEW")]
    PageDashboardView,

    #[serde(rename = "JOBS_VIEW")]
    PageJobsView,

    #[serde(rename = "FINANCE_VIEW")]
    PageFinanceView,

    #[serde(rename = "INVOICES_VIEW")]
    PageInvoicesView,

    #[serde(rename = "KYC_VIEW")]
    PageKycView,

    #[serde(rename = "REPORTS_VIEW")]
    PageReportsView,

    #[serde(rename = "USERS_VIEW")]
    PageUsersView,

    #[serde(rename = "PERMISSIONS_VIEW")]
    PagePermissionsView,

    #[serde(rename = "BASIC_SETTINGS_VIEW")]
    PageBasicSettingsView,

    #[serde(rename = "SERVICE_VIEW")]
    PageServiceView,

    JobsCreate,

    JobsUpdate,

    JobsDelete,

    JobsExport,

    FinanceEscrowRelease,

    FinanceExport,

    InvoicesCreate,

    InvoicesUpdate,

    InvoicesExport,

    KycApprove,

    KycReject,

    ReportsExport,

    UsersCreate,

    UsersUpdate,

    UsersDelete,

    PermissionsUpdate,

    BasicSettingsUpdate,

    ServiceCreate,

    ServiceUpdate,

    ServiceDelete,

    BannerCreate,

    BannerUpdate,

    BannerDelete,

    CompaniesCreate,

    CompaniesUpdate,

    CompaniesDelete,

    CompaniesExport,

    MarketingCreate,

    MarketingUpdate,

    MarketingDelete,

    MarketingPublish,

    MarketingExport,

    SocialUpdate,

    SettingsUpdate,

    SettingView,
}

impl Permission {

    pub fn code(&self) -> &'static str {
        match self {
            Self::PageDashboardView => "DASHBOARD_VIEW",

            Self::PageJobsView => "JOBS_VIEW",
            Self::JobsCreate => "JOBS_CREATE",
            Self::JobsUpdate => "JOBS_UPDATE",
            Self::JobsDelete => "JOBS_DELETE",
            Self::JobsExport => "JOBS_EXPORT",

            Self::PageFinanceView => "FINANCE_VIEW",
            Self::FinanceEscrowRelease => "FINANCE_ESCROW_RELEASE",
            Self::FinanceExport => "FINANCE_EXPORT",

            Self::PageInvoicesView => "INVOICES_VIEW",
            Self::InvoicesCreate => "INVOICES_CREATE",
            Self::InvoicesUpdate => "INVOICES_UPDATE",
            Self::InvoicesExport => "INVOICES_EXPORT",

            Self::PageKycView => "KYC_VIEW",
            Self::KycApprove => "KYC_APPROVE",
            Self::KycReject => "KYC_REJECT",

            Self::PageReportsView => "REPORTS_VIEW",
            Self::ReportsExport => "REPORTS_EXPORT",

            Self::PageUsersView => "USERS_VIEW",
            Self::UsersCreate => "USERS_CREATE",
            Self::UsersUpdate => "USERS_UPDATE",
            Self::UsersDelete => "USERS_DELETE",

            Self::PagePermissionsView => "PERMISSIONS_VIEW",
            Self::PermissionsUpdate => "PERMISSIONS_UPDATE",

            Self::PageBasicSettingsView => "BASIC_SETTINGS_VIEW",
            Self::BasicSettingsUpdate => "BASIC_SETTINGS_UPDATE",

            Self::PageServiceView => "SERVICE_VIEW",
            Self::ServiceCreate => "SERVICE_CREATE",
            Self::ServiceUpdate => "SERVICE_UPDATE",
            Self::ServiceDelete => "SERVICE_DELETE",

            Self::BannerCreate => "BANNER_CREATE",
            Self::BannerUpdate => "BANNER_UPDATE",
            Self::BannerDelete => "BANNER_DELETE",

            Self::CompaniesCreate => "COMPANIES_CREATE",
            Self::CompaniesUpdate => "COMPANIES_UPDATE",
            Self::CompaniesDelete => "COMPANIES_DELETE",
            Self::CompaniesExport => "COMPANIES_EXPORT",

            Self::MarketingCreate => "MARKETING_CREATE",
            Self::MarketingUpdate => "MARKETING_UPDATE",
            Self::MarketingDelete => "MARKETING_DELETE",
            Self::MarketingPublish => "MARKETING_PUBLISH",
            Self::MarketingExport => "MARKETING_EXPORT",

            Self::SocialUpdate => "SOCIAL_UPDATE",

            Self::SettingsUpdate => "SETTINGS_UPDATE",
            Self::SettingView => "SETTING_VIEW",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        match code {
            "DASHBOARD_VIEW" => Some(Self::PageDashboardView),

            "JOBS_VIEW" => Some(Self::PageJobsView),
            "JOBS_CREATE" => Some(Self::JobsCreate),
            "JOBS_UPDATE" => Some(Self::JobsUpdate),
            "JOBS_DELETE" => Some(Self::JobsDelete),
            "JOBS_EXPORT" => Some(Self::JobsExport),

            "FINANCE_VIEW" => Some(Self::PageFinanceView),
            "FINANCE_ESCROW_RELEASE" => Some(Self::FinanceEscrowRelease),
            "FINANCE_EXPORT" => Some(Self::FinanceExport),

            "INVOICES_VIEW" => Some(Self::PageInvoicesView),
            "INVOICES_CREATE" => Some(Self::InvoicesCreate),
            "INVOICES_UPDATE" => Some(Self::InvoicesUpdate),
            "INVOICES_EXPORT" => Some(Self::InvoicesExport),

            "KYC_VIEW" => Some(Self::PageKycView),
            "KYC_APPROVE" => Some(Self::KycApprove),
            "KYC_REJECT" => Some(Self::KycReject),

            "REPORTS_VIEW" => Some(Self::PageReportsView),
            "REPORTS_EXPORT" => Some(Self::ReportsExport),

            "USERS_VIEW" => Some(Self::PageUsersView),
            "USERS_CREATE" => Some(Self::UsersCreate),
            "USERS_UPDATE" => Some(Self::UsersUpdate),
            "USERS_DELETE" => Some(Self::UsersDelete),

            "PERMISSIONS_VIEW" => Some(Self::PagePermissionsView),
            "PERMISSIONS_UPDATE" => Some(Self::PermissionsUpdate),

            "BASIC_SETTINGS_VIEW" => Some(Self::PageBasicSettingsView),
            "BASIC_SETTINGS_UPDATE" => Some(Self::BasicSettingsUpdate),

            "SERVICE_VIEW" => Some(Self::PageServiceView),
            "SERVICE_CREATE" => Some(Self::ServiceCreate),
            "SERVICE_UPDATE" => Some(Self::ServiceUpdate),
            "SERVICE_DELETE" => Some(Self::ServiceDelete),

            "BANNER_CREATE" => Some(Self::BannerCreate),
            "BANNER_UPDATE" => Some(Self::BannerUpdate),
            "BANNER_DELETE" => Some(Self::BannerDelete),

            "COMPANIES_CREATE" => Some(Self::CompaniesCreate),
            "COMPANIES_UPDATE" => Some(Self::CompaniesUpdate),
            "COMPANIES_DELETE" => Some(Self::CompaniesDelete),
            "COMPANIES_EXPORT" => Some(Self::CompaniesExport),

            "MARKETING_CREATE" => Some(Self::MarketingCreate),
            "MARKETING_UPDATE" => Some(Self::MarketingUpdate),
            "MARKETING_DELETE" => Some(Self::MarketingDelete),
            "MARKETING_PUBLISH" => Some(Self::MarketingPublish),
            "MARKETING_EXPORT" => Some(Self::MarketingExport),

            "SOCIAL_UPDATE" => Some(Self::SocialUpdate),

            "SETTINGS_UPDATE" => Some(Self::SettingsUpdate),
            "SETTING_VIEW" => Some(Self::SettingView),

            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UserStatus {

    Pending,

    Active,

    Suspended,

    Deleted,
}

impl UserStatus {

    pub fn can_login(&self) -> bool {
        matches!(self, Self::Active)
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Active => "active",
            Self::Suspended => "suspended",
            Self::Deleted => "deleted",
        }
    }

    pub fn as_i32(&self) -> i32 {
        match self {
            Self::Pending => 0,
            Self::Active => 1,
            Self::Suspended => 2,
            Self::Deleted => 3,
        }
    }

    pub fn from_i32(v: i32) -> Option<Self> {
        match v {
            0 => Some(Self::Pending),
            1 => Some(Self::Active),
            2 => Some(Self::Suspended),
            3 => Some(Self::Deleted),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {

    pub id: Uuid,

    pub first_name: String,

    pub last_name: String,

    pub username: String,

    pub password_hash: String,

    pub roles: Vec<Role>,

    pub permissions: Vec<Permission>,

    pub status: UserStatus,

    pub created_at: DateTime<Utc>,

    pub updated_at: DateTime<Utc>,
}

impl User {

    pub fn has_role(&self, role: Role) -> bool {
        self.roles.contains(&role)
    }

    pub fn is_admin(&self) -> bool {
        self.roles
            .iter()
            .any(|r| matches!(r, Role::Admin | Role::SuperAdmin))
    }

    pub fn is_super_admin(&self) -> bool {
        self.roles.contains(&Role::SuperAdmin)
    }

    pub fn has_permission(&self, permission: Permission) -> bool {
        self.permissions.contains(&permission)
    }

    pub fn permission_codes(&self) -> Vec<&'static str> {
        self.permissions.iter().map(|p| p.code()).collect()
    }

    pub fn can_authenticate(&self) -> bool {
        self.status.can_login()
    }

    pub fn display_name(&self) -> String {
        let first = self.first_name.trim();
        let last = self.last_name.trim();
        if first.is_empty() && last.is_empty() {
            String::new()
        } else if first.is_empty() {
            last.to_string()
        } else if last.is_empty() {
            first.to_string()
        } else {
            format!("{first} {last}")
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct UserListRow {

    pub user_guid: String,

    pub full_name: String,

    pub email: String,

    pub role_codes: Vec<String>,

    pub role_names: Vec<String>,

    pub has_permission: bool,

    pub has_override: bool,

    pub user_status: UserStatus,

    pub user_username_status: i32,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_user(status: UserStatus) -> User {
        User {
            id: Uuid::new_v4(),
            first_name: "Alice".into(),
            last_name: "Wonder".into(),
            username: "alice".into(),
            password_hash: "$argon2id$...".into(),
            roles: vec![Role::Customer],
            permissions: Vec::new(),
            status,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn role_as_str_matches_serde() {
        assert_eq!(Role::Customer.as_str(), "customer");
        assert_eq!(Role::Technician.as_str(), "technician");
        assert_eq!(Role::Admin.as_str(), "admin");
        assert_eq!(Role::SuperAdmin.as_str(), "super_admin");
    }

    #[test]
    fn role_from_code_round_trip() {
        for r in [
            Role::Customer,
            Role::Technician,
            Role::Admin,
            Role::SuperAdmin,
        ] {
            assert_eq!(Role::from_code(r.as_str()), Some(r));
        }
        assert_eq!(Role::from_code("ghost"), None);
    }

    #[test]
    fn status_can_login_matches_active() {
        assert!(UserStatus::Active.can_login());
        assert!(!UserStatus::Pending.can_login());
        assert!(!UserStatus::Suspended.can_login());
        assert!(!UserStatus::Deleted.can_login());
    }

    #[test]
    fn status_i32_round_trip() {
        for s in [
            UserStatus::Pending,
            UserStatus::Active,
            UserStatus::Suspended,
            UserStatus::Deleted,
        ] {
            assert_eq!(UserStatus::from_i32(s.as_i32()), Some(s));
        }
        assert_eq!(UserStatus::from_i32(99), None);
    }

    #[test]
    fn has_role_checks_membership() {
        let u = sample_user(UserStatus::Active);
        assert!(u.has_role(Role::Customer));
        assert!(!u.has_role(Role::Admin));
    }

    #[test]
    fn is_admin_for_admin_and_super_admin() {
        let mut u = sample_user(UserStatus::Active);
        assert!(!u.is_admin());
        u.roles = vec![Role::Admin];
        assert!(u.is_admin());
        assert!(!u.is_super_admin());
        u.roles = vec![Role::SuperAdmin];
        assert!(u.is_admin());
        assert!(u.is_super_admin());
    }

    #[test]
    fn can_authenticate_reflects_status() {
        assert!(sample_user(UserStatus::Active).can_authenticate());
        assert!(!sample_user(UserStatus::Suspended).can_authenticate());
        assert!(!sample_user(UserStatus::Pending).can_authenticate());
    }

    #[test]
    fn display_name_assembles_first_and_last() {
        let u = sample_user(UserStatus::Active);
        assert_eq!(u.display_name(), "Alice Wonder");
    }

    #[test]
    fn display_name_handles_empty_fields() {
        let mut u = sample_user(UserStatus::Active);
        u.first_name = "".into();
        u.last_name = "Solo".into();
        assert_eq!(u.display_name(), "Solo");
        u.first_name = "Only".into();
        u.last_name = "".into();
        assert_eq!(u.display_name(), "Only");
        u.first_name = "".into();
        u.last_name = "".into();
        assert_eq!(u.display_name(), "");
    }

    #[test]
    fn user_json_round_trip() {
        let u = sample_user(UserStatus::Active);
        let s = serde_json::to_string(&u).unwrap();
        let v: serde_json::Value = serde_json::from_str(&s).unwrap();
        assert_eq!(v["username"], "alice");
        assert_eq!(v["first_name"], "Alice");
        assert_eq!(v["last_name"], "Wonder");
        assert_eq!(v["roles"][0], "customer");
        assert_eq!(v["status"], "active");

        assert!(v["password_hash"].is_string());
    }

    #[test]
    fn permission_code_round_trip_covers_all_variants() {

        let samples = [
            Permission::PageDashboardView,
            Permission::PageJobsView,
            Permission::JobsCreate,
            Permission::JobsUpdate,
            Permission::JobsDelete,
            Permission::JobsExport,
            Permission::PageFinanceView,
            Permission::FinanceEscrowRelease,
            Permission::FinanceExport,
            Permission::PageInvoicesView,
            Permission::InvoicesCreate,
            Permission::InvoicesUpdate,
            Permission::InvoicesExport,
            Permission::PageKycView,
            Permission::KycApprove,
            Permission::KycReject,
            Permission::PageReportsView,
            Permission::ReportsExport,
            Permission::PageUsersView,
            Permission::UsersCreate,
            Permission::UsersUpdate,
            Permission::UsersDelete,
            Permission::PagePermissionsView,
            Permission::PermissionsUpdate,
            Permission::PageBasicSettingsView,
            Permission::BasicSettingsUpdate,
            Permission::PageServiceView,
            Permission::ServiceCreate,
            Permission::ServiceUpdate,
            Permission::ServiceDelete,
            Permission::BannerCreate,
            Permission::BannerUpdate,
            Permission::BannerDelete,
            Permission::CompaniesCreate,
            Permission::CompaniesUpdate,
            Permission::CompaniesDelete,
            Permission::CompaniesExport,
            Permission::MarketingCreate,
            Permission::MarketingUpdate,
            Permission::MarketingDelete,
            Permission::MarketingPublish,
            Permission::MarketingExport,
            Permission::SocialUpdate,
            Permission::SettingsUpdate,
            Permission::SettingView,
        ];
        for p in samples {
            assert_eq!(Permission::from_code(p.code()), Some(p));
        }

        assert_eq!(Permission::from_code("SOMETHING_NEW"), None);
    }

    #[test]
    fn permission_serializes_as_screaming_snake_case_code() {

        let json = serde_json::to_string(&Permission::JobsCreate).unwrap();
        assert_eq!(json, "\"JOBS_CREATE\"");

        let json = serde_json::to_string(&Permission::PageDashboardView).unwrap();
        assert_eq!(json, "\"DASHBOARD_VIEW\"");

        let list =
            serde_json::to_string(&vec![Permission::PageJobsView, Permission::JobsCreate]).unwrap();
        assert_eq!(list, "[\"JOBS_VIEW\",\"JOBS_CREATE\"]");
    }

    #[test]
    fn user_has_permission_and_permission_codes() {
        let mut u = sample_user(UserStatus::Active);
        u.permissions = vec![Permission::PageJobsView, Permission::JobsCreate];

        assert!(u.has_permission(Permission::PageJobsView));
        assert!(!u.has_permission(Permission::PageUsersView));

        let codes = u.permission_codes();
        assert_eq!(codes, vec!["JOBS_VIEW", "JOBS_CREATE"]);
    }
}
