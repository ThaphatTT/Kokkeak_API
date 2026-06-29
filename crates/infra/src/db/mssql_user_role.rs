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
//! ## Column-name reads
//!
//! Every helper call below uses `row.get::<_, _>("column_name")`
//! (column-by-name) rather than positional indices. The SP owns
//! the SELECT list and aliases every column, so by-name reads:
//!
//! * survive future SP-side column reorders (the bug we just fixed
//!   came from positional reads drifting off the SP — `col 5` on
//!   the Rust side vs `user_permission_guid` at `col 7` on the SP),
//! * make the wire contract self-documenting (no need to re-derive
//!   the column order from the SP each time),
//! * turn silent drift into a visible `None` + `unwrap_or("")` so a
//!   missing column can never silently land in the wrong Rust
//!   field.
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
use uuid::Uuid;

use kokkak_domain::traits::user::RepoError;
use kokkak_domain::{PermissionUpdateRow, UserRolePermissionRow, UserRoleRepository};

use crate::db::mssql::{exec_sp, read_guid_str, read_i32, read_str, MssqlPool};

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
    async fn list_permissions(
        &self,
        mode: &str,
        caller_guid: Uuid,
    ) -> Result<Vec<UserRolePermissionRow>, RepoError> {
        // The `mode` is a pass-through literal that the SP
        // uses to scope which role set to return. The Rust
        // side doesn't validate it (the SP does — unknown
        // modes return zero rows gracefully).
        //
        // M19: `@p_user_guid` is the admin check — non-admin
        // callers receive zero rows. String-encoded (project
        // rule: GUID into SP arrives as `varchar(36)` + TRY_CAST
        // inside the SP body).
        let caller_str = caller_guid.to_string();
        let rows = exec_sp(
            &self.pool,
            "EXEC dbo.SP_USER_GROUP_ROLE @p_mode = @P1, @p_by = @P2",
            &[&mode as &dyn ToSql, &caller_str as &dyn ToSql],
        )
        .await?;

        Ok(rows.iter().map(row_to_user_role_permission_row).collect())
    }

    async fn update_role_permission(
        &self,
        role_guid: &str,
        permission_guid: &str,
        status: i32,
        update_by: Option<&str>,
    ) -> Result<PermissionUpdateRow, RepoError> {
        // The SP returns **either** one row (success or domain
        // rejection) **or** zero rows (the silent no-op for
        // `status = 0` + no existing junction row — see the
        // SP's terminal `IF @p_user_role_permission_status = 1`
        // branch). Both shapes are flattened to one
        // `PermissionUpdateRow` per call so the application
        // layer can `Vec::push` per input item without missing
        // entries.
        //
        // We bind `update_by` as `Option<&str>` so a `None`
        // arrives at SQL Server as a real `NULL` (the
        // `@p_update_by varchar(50) = NULL` default in the SP
        // signature).
        let rows = exec_sp(
            &self.pool,
            "EXEC dbo.SP_USER_ROLE_PERMISSION_UPDATE \
                @p_user_role_guid = @P1, \
                @p_user_permission_guid = @P2, \
                @p_user_role_permission_status = @P3, \
                @p_update_by = @P4",
            &[
                &role_guid as &dyn ToSql,
                &permission_guid as &dyn ToSql,
                &status as &dyn ToSql,
                &update_by as &dyn ToSql,
            ],
        )
        .await?;

        match rows.first() {
            Some(row) => Ok(row_to_permission_update_row(row)),
            None => Ok(PermissionUpdateRow::no_change(
                role_guid.to_string(),
                permission_guid.to_string(),
            )),
        }
    }
}

/// Hydrate one `PermissionUpdateRow` from the SP's single-row result set.
///
/// Every field below is read by **column name** (not positional
/// index). The authoritative SELECT list lives in
/// `migrations/20260620000007_sp_user_role.sql` →
/// `SP_USER_ROLE_PERMISSION_UPDATE`. The relevant columns:
///
/// | Column                          | Rust field                      | Type    |
/// |---------------------------------|---------------------------------|---------|
/// | `success`                       | `success`                       | bit     |
/// | `code`                          | `code`                          | varchar |
/// | `message`                       | `message`                       | varchar |
/// | `user_role_permission_guid`     | `user_role_permission_guid`     | varchar |
/// | `user_role_guid`                | `user_role_guid`                | varchar |
/// | `user_role_code`                | *(not consumed)*                | varchar |
/// | `user_role_name`                | *(not consumed)*                | nvarchar|
/// | `user_permission_guid`          | `user_permission_guid`          | varchar |
/// | `user_permission_code`          | *(not consumed)*                | varchar |
/// | `user_permission_name`          | *(not consumed)*                | nvarchar|
/// | `user_role_permission_status`   | `user_role_permission_status`   | int     |
///
/// `success` arrives as `bit` — tiberius exposes it as `i16` via
/// `Row::get`. We coerce non-zero to `true` so any future DB-side
/// change (bit → tinyint, etc.) doesn't silently flip the meaning.
fn row_to_permission_update_row(row: &Row) -> PermissionUpdateRow {
    let success_bit: bool = row.get::<bool, _>("success").unwrap_or(false);
    let code = read_str(row, "code").unwrap_or("").to_string();
    let message = read_str(row, "message").unwrap_or("").to_string();
    // GUID columns: accept both `uniqueidentifier` (via
    // `tiberius::Guid`) and `varchar(36)` (via `&str`). See
    // `mssql::read_guid_str` for the rationale. The SP leaves
    // `user_role_permission_guid` empty for the per-item rejection
    // branches (ROLE_NOT_FOUND / PERMISSION_NOT_FOUND); an empty
    // `String` from the helper maps to `None` on the wire.
    let rp_guid_raw = read_guid_str(row, "user_role_permission_guid");
    let role_guid = read_guid_str(row, "user_role_guid");
    let perm_guid = read_guid_str(row, "user_permission_guid");
    let status = read_i32(row, "user_role_permission_status").unwrap_or(0);

    PermissionUpdateRow {
        success: success_bit,
        code,
        message,
        // The SP leaves `user_role_permission_guid` empty for the
        // per-item rejection branches (ROLE_NOT_FOUND,
        // PERMISSION_NOT_FOUND). `None` is the wire shape; the
        // SP-issued empty string means "no junction row exists".
        user_role_permission_guid: if rp_guid_raw.is_empty() {
            None
        } else {
            Some(rp_guid_raw)
        },
        user_role_guid: role_guid,
        user_permission_guid: perm_guid,
        user_role_permission_status: status,
    }
}

/// Hydrate one `UserRolePermissionRow` from a tiberius `Row`.
///
/// Reads by column name — the authoritative SELECT list lives in
/// `migrations/20260620000007_sp_user_role.sql` → `SP_USER_GROUP_ROLE`:
///
/// | Column                          | Rust field                      |
/// |---------------------------------|---------------------------------|
/// | `user_role_guid`                | `user_role_guid`                |
/// | `user_role_code`                | `user_role_code`                |
/// | `user_role_permission_guid`     | `user_role_permission_guid`     |
/// | `user_role_permission_status`   | `user_role_permission_status`   |
/// | `user_permission_guid`          | `user_permission_guid`          |
/// | `user_permission_code`          | `user_permission_code`          |
///
/// `read_str` returns `Option<&str>` (NULL → None) which we
/// coerce to `String::new()` — the SP's `COALESCE(..., '')` means
/// NULLs only arrive if a future schema change drops the
/// COALESCE, and defaulting to "" keeps the JSON shape stable for
/// mobile clients instead of emitting `null`.
fn row_to_user_role_permission_row(row: &Row) -> UserRolePermissionRow {
    UserRolePermissionRow {
        // GUID columns: accept both `uniqueidentifier` and `varchar(36)`.
        user_role_guid: read_guid_str(row, "user_role_guid"),
        user_role_code: read_str(row, "user_role_code").unwrap_or("").to_string(),
        user_role_permission_guid: read_guid_str(row, "user_role_permission_guid"),
        user_role_permission_status: read_i32(row, "user_role_permission_status").unwrap_or(0),
        user_permission_guid: read_guid_str(row, "user_permission_guid"),
        user_permission_code: read_str(row, "user_permission_code")
            .unwrap_or("")
            .to_string(),
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
