//! User domain (เอนทิตี้ผู้ใช้ — M2).
//!
//! `User` is the central entity for everyone who can log in:
//! customers, technicians, and admin staff. The same struct carries
//! the role + permission set; per-role policy lives in application /
//! API middleware (AGENTS.md § 12.3).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Top-level role. Multi-role users are rare in the legacy system
/// (KOKKAK mostly does single-role per user) but the model is
/// open-ended via `Vec<Role>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
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
}

/// Lifecycle status of a user account.
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

    /// Snake_case identifier.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Active => "active",
            Self::Suspended => "suspended",
            Self::Deleted => "deleted",
        }
    }
}

/// The canonical user entity.
///
/// **Security note**: `password_hash` is the argon2 PHC string. Plain
/// passwords never live on the struct (AGENTS.md § 12.1).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    /// Stable identifier (UUID v4 for the JSON-DB simulation; v7 in
    /// the SQL Server migration).
    pub id: Uuid,
    /// Login email, lowercased. Unique per user.
    pub email: String,
    /// Display name (free text, shown in admin panels).
    pub display_name: String,
    /// argon2 PHC string. Never logged (AGENTS.md § 12.1).
    pub password_hash: String,
    /// One or more roles. Most users have exactly one.
    pub roles: Vec<Role>,
    /// Account lifecycle.
    pub status: UserStatus,
    /// Locale preference (`"th"`, `"en"`, `"lo"`).
    pub locale: String,
    /// UTC timestamp of account creation.
    pub created_at: DateTime<Utc>,
    /// UTC timestamp of the last profile change.
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
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_user(status: UserStatus) -> User {
        User {
            id: Uuid::new_v4(),
            email: "a@b.com".into(),
            display_name: "Alice".into(),
            password_hash: "$argon2id$...".into(),
            roles: vec![Role::Customer],
            status,
            locale: "lo".into(),
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
    fn status_can_login_matches_active() {
        assert!(UserStatus::Active.can_login());
        assert!(!UserStatus::Pending.can_login());
        assert!(!UserStatus::Suspended.can_login());
        assert!(!UserStatus::Deleted.can_login());
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
    fn user_json_round_trip() {
        let u = sample_user(UserStatus::Active);
        let s = serde_json::to_string(&u).unwrap();
        let v: serde_json::Value = serde_json::from_str(&s).unwrap();
        assert_eq!(v["email"], "a@b.com");
        assert_eq!(v["roles"][0], "customer");
        assert_eq!(v["status"], "active");
    }
}
