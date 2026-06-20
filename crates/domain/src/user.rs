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
            Self::Customer => "customer",
            Self::Technician => "technician",
            Self::Admin => "admin",
            Self::SuperAdmin => "super_admin",
        }
    }

    /// Parse from the snake_case string. Returns `None` for unknown codes.
    /// Used by the MSSQL repo to translate `user_role.user_role_code` →
    /// Rust enum when assembling the `User` aggregate from a JOIN.
    pub fn from_code(s: &str) -> Option<Self> {
        match s {
            "customer" => Some(Self::Customer),
            "technician" => Some(Self::Technician),
            "admin" => Some(Self::Admin),
            "super_admin" => Some(Self::SuperAdmin),
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
}
