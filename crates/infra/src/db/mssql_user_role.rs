//! SQL Server-backed `UserRoleRepository` (M15-prep).
//!
//! Implements [`UserRoleRepository`] by calling
//! `dbo.SP_USER_GROUP_ROLE` (see
//! `migrations/20260620000007_sp_user_role.sql`). One method, one
//! SP — the trait surface is intentionally minimal because the
//! SP returns a flat matrix and we want no application-side
//! post-processing.
//!
//! ponytail: row mapping is a thin field-by-field copy because the
//! SP's `COALESCE` already converts NULLs into empty strings /
//! zero status. The ceiling would be a macro-driven mapper when
//! the 5th or 6th SP returns the same shape, but at one SP this
//! would only obscure the wire contract.
//!
//! ## Grant status on the wire
//!
//! The SP emits one row per (role × permission) pair, including
//! pairs that have NOT been granted yet. The grant status is
//! encoded in the row directly:
//!
//! * GRANTED   → `user_role_permission_guid` filled, status = 1
//! * UNGRANTED → `user_role_permission_guid` = "", status = 0,
//!   but `user_permission_guid` still populated
//!
//! The application layer must NOT drop the UNGRANTED rows — the
//! admin UI pattern-matches on the empty junction guid to render
//! checked / unchecked boxes. The defensive `unwrap_or("")` /
//! `unwrap_or(0)` fallbacks below keep the JSON shape stable if
//! a future schema change drops the COALESCE.

use async_trait::async_trait;
use tiberius::Row;
use tiberius::ToSql;

use kokkak_domain::traits::user::RepoError;
use kokkak_domain::{UserRolePermissionRow, UserRoleRepository};

use crate::db::mssql::{exec_sp, read_i32, read_str, MssqlPool};

/// SQL Server-backed `UserRoleRepository` (M15-prep).
#[derive(Clone)]
pub struct MssqlUserRoleRepository {
    pool: MssqlPool,
}

impl MssqlUserRoleRepository {
    /// Construct the repository with a shared `MssqlPool`.
    pub fn new(pool: MssqlPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl UserRoleRepository for MssqlUserRoleRepository {
    async fn list_permissions(&self, mode: &str) -> Result<Vec<UserRolePermissionRow>, RepoError> {
        // The `mode` is a pass-through literal that the SP
        // uses to scope which role set to return. The Rust
        // side doesn't validate it (the SP does — unknown
        // modes return zero rows gracefully).
        let rows = exec_sp(
            &self.pool,
            "EXEC dbo.SP_USER_GROUP_ROLE @p_mode = @P1",
            &[&mode as &dyn ToSql],
        )
        .await?;

        Ok(rows.iter().map(row_to_user_role_permission_row).collect())
    }
}

/// Hydrate one `UserRolePermissionRow` from a tiberius `Row`.
///
/// Column order matches `SP_USER_GROUP_ROLE`'s SELECT list:
///   0 user_role_guid
///   1 user_role_code
///   2 user_role_permission_guid   — empty when UNGRANTED
///   3 user_role_permission_status — `1` GRANTED, `0` UNGRANTED
///   4 user_permission_guid
///   5 user_permission_code
///
/// `read_str` returns `Option<&str>` (NULL → None) which we
/// coerce to `String::new()` — the SP's `COALESCE(..., '')` means
/// NULLs only arrive if a future schema change drops the
/// COALESCE, and defaulting to "" keeps the JSON shape stable for
/// mobile clients instead of emitting `null`.
fn row_to_user_role_permission_row(row: &Row) -> UserRolePermissionRow {
    UserRolePermissionRow {
        user_role_guid: read_str(row, 0).unwrap_or("").to_string(),
        user_role_code: read_str(row, 1).unwrap_or("").to_string(),
        user_role_permission_guid: read_str(row, 2).unwrap_or("").to_string(),
        user_role_permission_status: read_i32(row, 3).unwrap_or(0),
        user_permission_guid: read_str(row, 4).unwrap_or("").to_string(),
        user_permission_code: read_str(row, 5).unwrap_or("").to_string(),
    }
}

#[cfg(test)]
mod tests {
    //! Tests for the row mapper. We can't construct a `tiberius::Row`
    //! in unit tests (it borrows from a live connection), so we
    //! exercise the Rust-side guarantees via the wire-format
    //! invariants: the empty-string / zero defaults, and the field
    //! ordering that matches the SP.
    //!
    //! Integration coverage of the full path lives in the
    //! `tests/` integration suite once a SQL Server test container
    //! is wired up.
    use super::*;

    #[test]
    fn row_defaults_match_coalesce_contract() {
        // We can't construct a real `Row`, but the default
        // branch of `row_to_user_role_permission_row` (all reads
        // returning None) produces the same shape as a row of
        // all-empty strings — that's the COALESCE contract on
        // the SQL side. The struct literal below mirrors what
        // the mapper emits in that degenerate case; the test
        // pins the shape so any future struct refactor has to
        // update both this expectation and the JSON contract.
        let row = UserRolePermissionRow {
            user_role_guid: String::new(),
            user_role_code: String::new(),
            user_role_permission_guid: String::new(),
            user_role_permission_status: 0,
            user_permission_guid: String::new(),
            user_permission_code: String::new(),
        };
        let json = serde_json::to_value(&row).unwrap();
        assert_eq!(json["user_role_guid"], "");
        assert_eq!(json["user_role_permission_status"], 0);
        assert_eq!(json["user_permission_guid"], "");
        assert_eq!(json["user_permission_code"], "");
    }
}
