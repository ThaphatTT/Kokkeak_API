//! User domain (เอนทิตี้ผู้ใช้ — M2 + M14).
//!
//! `User` is the central aggregate for everyone who can log in:
//! customers, technicians, and admin staff. The same struct carries
//! the role + permission set; per-role policy lives in application /
//! API middleware (AGENTS.md § 12.3).
//!
//! **Persistence** matches `kokkeak/NEW_DB.txt` schema v2 (4 tables):
//! - profile fields → `[user]`
//! - login + password hash → `[user_username]`
//! - roles → `[user_role]` joined via `[user_user_role]`
//!
//! Repositories JOIN the four tables at read time and run a 3-table
//! INSERT in a single transaction at write time. The domain stays
//! aggregate-shaped so the rest of the code is unaware of the
//! underlying normalization (Ponytail: minimum surface change).
//!
//! **Locale**: removed from this aggregate (NEW_DB `[user]` has no
//! locale column). Per-request locale is resolved from `Accept-Language`
//! header / `?lang=` query / JWT claim via M11 middleware.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Top-level role. Multi-role users are rare in the legacy system
/// (KOKKAK mostly does single-role per user) but the model is
/// open-ended via `Vec<Role>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum Role {
    /// End user who books a handyman / technician.
    Customer,
    /// Service provider who gets matched with orders.
    Technician,
    /// Backoffice staff.
    Admin,
    /// Super-admin (all permissions). Granted very sparingly.
    SuperAdmin,
}

impl Role {
    /// Canonical snake_case identifier (stable, switch-friendly).
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Customer => "CUSTOMER",
            Self::Technician => "TECHNICIAN",
            Self::Admin => "ADMIN",
            Self::SuperAdmin => "SUPER_ADMIN",
        }
    }

    /// Parse from the snake_case string. Returns `None` for unknown codes.
    /// Used by the MSSQL repo to translate `user_role.user_role_code` →
    /// Rust enum when assembling the `User` aggregate from a JOIN.
    pub fn from_code(s: &str) -> Option<Self> {
        match s {
            "FINANCE_EXPORT" => Some(Self::Customer),
            "CUSTOMER" => Some(Self::Customer),
            "TECHNICIAN" => Some(Self::Technician),
            "ADMIN" => Some(Self::Admin),
            "SUPER_ADMIN" => Some(Self::SuperAdmin),
            _ => None,
        }
    }
}

/// Granular permission codes sent to the frontend so each page can
/// decide what to render.
///
/// Wire format is the SCREAMING_SNAKE_CASE string (`Permission::code()`)
/// so the frontend can match it directly without a Rust-side enum
/// mirror. The MSSQL stored procedure returns a comma-separated list of
/// these codes (e.g. `PAGE_DASHBOARD_VIEW,JOBS_CREATE`) which the repo
/// parses via `Permission::from_code`.
///
/// **Naming convention** (consistent with AGENTS.md § 12.3):
/// - `PAGE_*`     → page-level visibility (sidebar / route guard)
/// - non-PAGE     → action-level (button / API capability)
///
/// Adding a new permission = add a variant here + ask the DBA to add the
/// code to `user_admin_panel_permission.user_admin_panel_permission_code`.
/// The Rust side logs a WARN for unknown codes so missing entries are
/// observable, not silent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Permission {
    // ── Page visibility ──────────────────────────────────────────────
    /// Dashboard page visible (KPIs, charts).
    PageDashboardView,

    /// Jobs (orders) list page visible.
    PageJobsView,
    /// Finance page visible.
    PageFinanceView,
    /// Invoices page visible.
    PageInvoicesView,
    /// KYC page visible.
    PageKycView,
    /// Reports page visible.
    PageReportsView,
    /// Users management page visible.
    PageUsersView,
    /// Permission matrix page visible.
    PagePermissionsView,
    /// Basic settings page visible.
    PageBasicSettingsView,
    /// Service catalog page visible.
    PageServiceView,

    // ── Jobs / orders actions ────────────────────────────────────────
    /// Create a new job.
    JobsCreate,
    /// Edit an existing job.
    JobsUpdate,
    /// Cancel / delete a job.
    JobsDelete,
    /// Export jobs to CSV / Excel.
    JobsExport,

    // ── Finance actions ──────────────────────────────────────────────
    /// Release escrow (transfer from platform → technician).
    FinanceEscrowRelease,
    /// Export finance data.
    FinanceExport,

    // ── Invoices actions ─────────────────────────────────────────────
    /// Create a new invoice.
    InvoicesCreate,
    /// Edit an existing invoice.
    InvoicesUpdate,
    /// Export invoices.
    InvoicesExport,

    // ── KYC actions ──────────────────────────────────────────────────
    /// Approve a KYC submission.
    KycApprove,
    /// Reject a KYC submission.
    KycReject,

    // ── Reports actions ──────────────────────────────────────────────
    /// Export reports.
    ReportsExport,

    // ── User management actions ──────────────────────────────────────
    /// Create a new user.
    UsersCreate,
    /// Edit an existing user.
    UsersUpdate,
    /// Delete / disable a user.
    UsersDelete,

    // ── Permission matrix actions ────────────────────────────────────
    /// Update role / permission assignments.
    PermissionsUpdate,

    // ── Basic settings actions ───────────────────────────────────────
    /// Update platform-wide basic settings.
    BasicSettingsUpdate,

    // ── Service catalog actions ──────────────────────────────────────
    /// Create a service category.
    ServiceCreate,
    /// Edit a service category.
    ServiceUpdate,
    /// Delete a service category.
    ServiceDelete,
}

impl Permission {
    /// Canonical SCREAMING_SNAKE_CASE identifier (stable, switch-friendly).
    /// This is what the DBA stores and what the frontend receives.
    pub fn code(&self) -> &'static str {
        match self {
            Self::PageDashboardView => "PAGE_DASHBOARD_VIEW",

            Self::PageJobsView => "PAGE_JOBS_VIEW",
            Self::JobsCreate => "JOBS_CREATE",
            Self::JobsUpdate => "JOBS_UPDATE",
            Self::JobsDelete => "JOBS_DELETE",
            Self::JobsExport => "JOBS_EXPORT",

            Self::PageFinanceView => "PAGE_FINANCE_VIEW",
            Self::FinanceEscrowRelease => "FINANCE_ESCROW_RELEASE",
            Self::FinanceExport => "FINANCE_EXPORT",

            Self::PageInvoicesView => "PAGE_INVOICES_VIEW",
            Self::InvoicesCreate => "INVOICES_CREATE",
            Self::InvoicesUpdate => "INVOICES_UPDATE",
            Self::InvoicesExport => "INVOICES_EXPORT",

            Self::PageKycView => "PAGE_KYC_VIEW",
            Self::KycApprove => "KYC_APPROVE",
            Self::KycReject => "KYC_REJECT",

            Self::PageReportsView => "PAGE_REPORTS_VIEW",
            Self::ReportsExport => "REPORTS_EXPORT",

            Self::PageUsersView => "PAGE_USERS_VIEW",
            Self::UsersCreate => "USERS_CREATE",
            Self::UsersUpdate => "USERS_UPDATE",
            Self::UsersDelete => "USERS_DELETE",

            Self::PagePermissionsView => "PAGE_PERMISSIONS_VIEW",
            Self::PermissionsUpdate => "PERMISSIONS_UPDATE",

            Self::PageBasicSettingsView => "PAGE_BASIC_SETTINGS_VIEW",
            Self::BasicSettingsUpdate => "BASIC_SETTINGS_UPDATE",

            Self::PageServiceView => "PAGE_SERVICE_VIEW",
            Self::ServiceCreate => "SERVICE_CREATE",
            Self::ServiceUpdate => "SERVICE_UPDATE",
            Self::ServiceDelete => "SERVICE_DELETE",
        }
    }

    /// Parse from the SCREAMING_SNAKE_CASE string stored in
    /// `user_admin_panel_permission.user_admin_panel_permission_code`.
    /// Returns `None` for unknown codes so the MSSQL repo can log a
    /// WARN (DBA may have added a new permission before the Rust enum
    /// was updated) instead of panicking at startup.
    pub fn from_code(code: &str) -> Option<Self> {
        match code {
            "PAGE_DASHBOARD_VIEW" => Some(Self::PageDashboardView),

            "PAGE_JOBS_VIEW" => Some(Self::PageJobsView),
            "JOBS_CREATE" => Some(Self::JobsCreate),
            "JOBS_UPDATE" => Some(Self::JobsUpdate),
            "JOBS_DELETE" => Some(Self::JobsDelete),
            "JOBS_EXPORT" => Some(Self::JobsExport),

            "PAGE_FINANCE_VIEW" => Some(Self::PageFinanceView),
            "FINANCE_ESCROW_RELEASE" => Some(Self::FinanceEscrowRelease),
            "FINANCE_EXPORT" => Some(Self::FinanceExport),

            "PAGE_INVOICES_VIEW" => Some(Self::PageInvoicesView),
            "INVOICES_CREATE" => Some(Self::InvoicesCreate),
            "INVOICES_UPDATE" => Some(Self::InvoicesUpdate),
            "INVOICES_EXPORT" => Some(Self::InvoicesExport),

            "PAGE_KYC_VIEW" => Some(Self::PageKycView),
            "KYC_APPROVE" => Some(Self::KycApprove),
            "KYC_REJECT" => Some(Self::KycReject),

            "PAGE_REPORTS_VIEW" => Some(Self::PageReportsView),
            "REPORTS_EXPORT" => Some(Self::ReportsExport),

            "PAGE_USERS_VIEW" => Some(Self::PageUsersView),
            "USERS_CREATE" => Some(Self::UsersCreate),
            "USERS_UPDATE" => Some(Self::UsersUpdate),
            "USERS_DELETE" => Some(Self::UsersDelete),

            "PAGE_PERMISSIONS_VIEW" => Some(Self::PagePermissionsView),
            "PERMISSIONS_UPDATE" => Some(Self::PermissionsUpdate),

            "PAGE_BASIC_SETTINGS_VIEW" => Some(Self::PageBasicSettingsView),
            "BASIC_SETTINGS_UPDATE" => Some(Self::BasicSettingsUpdate),

            "PAGE_SERVICE_VIEW" => Some(Self::PageServiceView),
            "SERVICE_CREATE" => Some(Self::ServiceCreate),
            "SERVICE_UPDATE" => Some(Self::ServiceUpdate),
            "SERVICE_DELETE" => Some(Self::ServiceDelete),

            _ => None,
        }
    }
}

/// Lifecycle status of a user account.
///
/// Persisted as `INT` in `[user].user_status` per NEW_DB.txt.
/// Convention: `0 = Pending`, `1 = Active`, `2 = Suspended`, `3 = Deleted`.
/// The MSSQL repo maps at the boundary (read/write); the JSON-DB sim
/// keeps the enum's serde form for dev convenience.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UserStatus {
    /// Newly created; cannot log in yet (email verification etc.).
    Pending,
    /// Normal active user.
    Active,
    /// Disabled by admin (login rejected).
    Suspended,
    /// Soft-deleted; kept for audit. Cannot log in.
    Deleted,
}

impl UserStatus {
    /// `true` iff this status can log in.
    pub fn can_login(&self) -> bool {
        matches!(self, Self::Active)
    }

    /// Snake_case identifier (for JSON-DB sim).
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Active => "active",
            Self::Suspended => "suspended",
            Self::Deleted => "deleted",
        }
    }

    /// NEW_DB `INT` representation (`[user].user_status`).
    pub fn as_i32(&self) -> i32 {
        match self {
            Self::Pending => 0,
            Self::Active => 1,
            Self::Suspended => 2,
            Self::Deleted => 3,
        }
    }

    /// Parse from NEW_DB `INT`. Returns `None` for unknown values so the
    /// MSSQL repo surfaces a typed `RepoError::Backend` instead of
    /// silently coercing garbage.
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

/// The canonical user aggregate.
///
/// Fields are loaded by the repository from the 4 NEW_DB tables via JOIN.
/// The struct is the unit the application / API layers see — the
/// 4-table normalization is hidden in the persistence adapter.
///
/// **Security note**: `password_hash` is the argon2 PHC string. Plain
/// passwords never live on the struct (AGENTS.md § 12.1).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    /// Stable identifier (`[user].user_guid`).
    pub id: Uuid,
    /// First name (`[user].user_first_name`).
    pub first_name: String,
    /// Last name (`[user].user_last_name`).
    pub last_name: String,
    /// Login username (`[user_username].user_username_username`).
    /// Lowercased on write. Unique across all users (DB-enforced).
    pub username: String,
    /// argon2 PHC string (`[user_username].user_username_password`).
    /// Never logged (AGENTS.md § 12.1).
    pub password_hash: String,
    /// Roles joined from `[user_role]` via `[user_user_role]`.
    pub roles: Vec<Role>,
    /// Effective permissions (role grants + explicit allow − deny)
    /// joined from `[user_admin_panel_permission]` via the SP. Returned
    /// to the frontend in `PublicUser.permissions` so each page can
    /// decide what to render without a second round-trip.
    pub permissions: Vec<Permission>,
    /// Account lifecycle (`[user].user_status`).
    pub status: UserStatus,
    /// UTC timestamp of account creation (`[user].user_create_at`).
    pub created_at: DateTime<Utc>,
    /// UTC timestamp of the last profile change (`[user].user_update_at`).
    pub updated_at: DateTime<Utc>,
}

impl User {
    /// `true` iff the user has the given role.
    pub fn has_role(&self, role: Role) -> bool {
        self.roles.contains(&role)
    }

    /// `true` iff the user is admin-level (Admin or SuperAdmin).
    pub fn is_admin(&self) -> bool {
        self.roles
            .iter()
            .any(|r| matches!(r, Role::Admin | Role::SuperAdmin))
    }

    /// `true` iff the user is a super-admin.
    pub fn is_super_admin(&self) -> bool {
        self.roles.contains(&Role::SuperAdmin)
    }

    /// `true` iff the user holds the given permission.
    pub fn has_permission(&self, permission: Permission) -> bool {
        self.permissions.contains(&permission)
    }

    /// Convenience: emit the permission list as the wire codes
    /// (`SCREAMING_SNAKE_CASE`) consumed by the frontend. Useful when
    /// building ad-hoc responses that bypass `PublicUser`.
    pub fn permission_codes(&self) -> Vec<&'static str> {
        self.permissions.iter().map(|p| p.code()).collect()
    }

    /// `true` iff the user can authenticate right now.
    pub fn can_authenticate(&self) -> bool {
        self.status.can_login()
    }

    /// Display name assembled from first + last. Useful for admin
    /// panels and audit logs where the legacy `display_name` field
    /// used to live. Returns a trimmed single string.
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
        // password_hash must be in the JSON representation (it is
        // the repository / DB that hides it from the API surface).
        assert!(v["password_hash"].is_string());
    }

    #[test]
    fn permission_code_round_trip_covers_all_variants() {
        // Every variant we ship must round-trip through code() and
        // from_code(). Adding a new variant without updating both
        // arms of this test will fail here.
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
        ];
        for p in samples {
            assert_eq!(Permission::from_code(p.code()), Some(p));
        }
        // A foreign code returns None (not a panic).
        assert_eq!(Permission::from_code("SOMETHING_NEW"), None);
    }

    #[test]
    fn permission_serializes_as_screaming_snake_case_code() {
        // Wire format is the code string, not the Rust variant name —
        // this is what the frontend matches on.
        let json = serde_json::to_string(&Permission::JobsCreate).unwrap();
        assert_eq!(json, "\"JOBS_CREATE\"");

        let json = serde_json::to_string(&Permission::PageDashboardView).unwrap();
        assert_eq!(json, "\"PAGE_DASHBOARD_VIEW\"");

        let list =
            serde_json::to_string(&vec![Permission::PageJobsView, Permission::JobsCreate]).unwrap();
        assert_eq!(list, "[\"PAGE_JOBS_VIEW\",\"JOBS_CREATE\"]");
    }

    #[test]
    fn user_has_permission_and_permission_codes() {
        let mut u = sample_user(UserStatus::Active);
        u.permissions = vec![Permission::PageJobsView, Permission::JobsCreate];

        assert!(u.has_permission(Permission::PageJobsView));
        assert!(!u.has_permission(Permission::PageUsersView));

        let codes = u.permission_codes();
        assert_eq!(codes, vec!["PAGE_JOBS_VIEW", "JOBS_CREATE"]);
    }
}
