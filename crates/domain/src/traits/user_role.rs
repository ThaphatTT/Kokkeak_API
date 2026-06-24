//! User-role / permission repository port (M15-prep).
//!
//! Read-side only: the SP_USER_GROUP_ROLE stored procedure is a
//! flat matrix view, so the trait surface is one method. We
//! deliberately do **not** model CRUD on `user_role_permission`
//! yet — the admin UI for granting / revoking permissions lands
//! alongside the full role-management endpoints (M15+).
//!
//! The trait lives in `domain` (per AGENTS.md § 6): application
//! code depends on `dyn UserRoleRepository`, the SQL Server
//! adapter in `kokkak-infra` implements it.

use async_trait::async_trait;

use crate::permission::UserRolePermissionRow;
use crate::traits::user::RepoError;

/// Repository contract for the role × permission matrix.
///
/// Implementations are expected to call `dbo.SP_USER_GROUP_ROLE`
/// (or an equivalent) and translate each row into
/// [`UserRolePermissionRow`]. The returned `Vec` is empty when
/// the SP returns zero rows (e.g. no active roles yet, or the
/// supplied `mode` is not in the SP's whitelist) — the handler
/// turns that into an empty list, **not** a 404, because the
/// role catalogue legitimately has zero entries for a fresh
/// install.
#[async_trait]
pub trait UserRoleRepository: Send + Sync {
    /// Read the role × permission matrix for a given mode.
    ///
    /// `mode` is a free-form pass-through literal that the SP
    /// uses to scope which role set to return (e.g.
    /// `SELECT_ADMIN`, `SELECT_EMPLOYEE`). The mode values are
    /// application-defined and may be extended over time; the
    /// Rust side does not enforce a closed set.
    async fn list_permissions(&self, mode: &str) -> Result<Vec<UserRolePermissionRow>, RepoError>;
}
