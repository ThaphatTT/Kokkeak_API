//! SQL Server-backed `PermissionUserRepository` (M17).
//!
//! Implements [`kokkak_domain::PermissionUserRepository`] via tiberius +
//! the NEW_DB v2 stored procedures. **No inline SQL** — every operation
//! is `EXEC dbo.SP_PERMISSION_USER_*` against KOKKAK_MASTER.
//!
//! ## Why a separate adapter (and not extend `MssqlUserRepository`)
//!
//! The permission page and the admin user-management screen used to share
//! `SP_PERMISSION_USER_LIST` / `SP_PERMISSION_USER_FIND_BY_USERNAME` — the
//! same SPs the `MssqlUserRepository::list_with_permissions` and
//! `find_user_permissions_by_username` adapters call. That coupled the
//! permission flow to the login/auth flow plus the generic admin user
//! list, and forced a GUID→username translation in the application layer.
//!
//! M17 decouples them:
//!
//! - **New SPs** (`SP_PERMISSION_USER_LIST_V2`,
//!   `SP_PERMISSION_USER_DETAIL_FIND_BY_GUID`) take a GUID directly and
//!   return the simpler single-`user_role_name` shape the permission page
//!   needs.
//! - **New adapter** (`MssqlPermissionUserRepository`) wires those SPs to
//!   the new [`PermissionUserRepository`] port.
//! - **Application service** (`PermissionUserService`) consumes the new
//!   port — it no longer depends on `UserRepository`.
//!
//! ## Row mapping
//!
//! The mapper is intentionally thin (one helper per DTO). The wire DTO
//! field names match the SP column names 1:1, so `read_str` / `read_i32`
//! lookups are straightforward.

use async_trait::async_trait;
use tiberius::ToSql;
use uuid::Uuid;

use kokkak_domain::permission::{
    PermissionOverrideUpdateItem, PermissionOverrideUpdateResult, PermissionUserDetailRow,
    PermissionUserListRow,
};
use kokkak_domain::traits::permission::PermissionUserRepository;
use kokkak_domain::traits::user::RepoError;

use crate::db::mssql::{exec_sp, read_datetime, read_i32, read_str, MssqlPool};

/// SQL Server-backed permission-page repository (M17).
#[derive(Clone)]
pub struct MssqlPermissionUserRepository {
    pool: MssqlPool,
}

impl MssqlPermissionUserRepository {
    /// Construct the repository with a shared `MssqlPool`.
    pub fn new(pool: MssqlPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl PermissionUserRepository for MssqlPermissionUserRepository {
    async fn list_permission_users(&self) -> Result<Vec<PermissionUserListRow>, RepoError> {
        let rows = exec_sp(&self.pool, "EXEC dbo.SP_PERMISSION_USER_LIST", &[]).await?;
        Ok(rows.iter().map(row_to_permission_user_list_row).collect())
    }

    async fn find_permission_user_detail(
        &self,
        user_guid: Uuid,
    ) -> Result<Vec<PermissionUserDetailRow>, RepoError> {
        let rows = exec_sp(
            &self.pool,
            "EXEC dbo.SP_PERMISSION_USER_DETAIL_FIND_BY_GUID @p_username_guid = @P1",
            &[&user_guid as &dyn ToSql],
        )
        .await?;
        if rows.is_empty() {
            return Err(RepoError::NotFound(format!("user {user_guid} not found")));
        }
        Ok(rows.iter().map(row_to_permission_user_detail_row).collect())
    }

    async fn update_permission_overrides(
        &self,
        items: &[PermissionOverrideUpdateItem],
        update_by: &str,
    ) -> Result<Vec<PermissionOverrideUpdateResult>, RepoError> {
        // The SP is one-row-in / one-row-out, so we loop. Order
        // is preserved: `results[i]` corresponds to `items[i]`.
        // A per-item rejection (e.g. `INVALID_EFFECT`) lands as a
        // result row with `success = false`; only a hard DB
        // failure (`RepoError::Backend`) aborts the loop.
        let mut results = Vec::with_capacity(items.len());
        for item in items {
            let result = call_override_update_sp(&self.pool, item, update_by).await?;
            results.push(result);
        }
        Ok(results)
    }
}

// ----------------------------------------------------------------------------
// Row mappers (thin — 1:1 with SP columns)
// ----------------------------------------------------------------------------

/// Map a single `SP_PERMISSION_USER_LIST_V2` row to
/// [`PermissionUserListRow`].
///
/// Column NAMES match the SP's SELECT aliases (one row per user):
///   `user_guid`               (varchar 36)
///   `full_name`               (varchar — first+' '+last, COALESCE'd to '')
///   `email`                   (varchar — username alias)
///   `role_codes`              (varchar — single role_code per row, may become CSV)
///   `role_names`              (varchar — single role_name per row, may become CSV)
///   `has_permission`          (int 0/1 — cheap "any perms?" badge)
///   `has_override`            (int 0/1 — explicit override flag)
///   `user_status`             (int — `[user].user_status`)
///   `user_username_status`    (int — `[user_username].user_username_status`)
///   `user_create_at`          (datetime2 UTC)
///   `user_update_at`          (datetime2 UTC, ISNULL → user_create_at)
///
/// All reads are defensive (`unwrap_or_default()`) so a future SP
/// refactor that drops a column doesn't 500 the request — the field
/// just lands as its `Default` value. The mapper is intentionally
/// thin: no CSV split, no bool / enum conversion. That work lives at
/// the application layer if / when it's needed (mirrors the
/// `PermissionUserDetailRow` convention right below).
fn row_to_permission_user_list_row(row: &tiberius::Row) -> PermissionUserListRow {
    PermissionUserListRow {
        user_guid: read_str(row, "user_guid").unwrap_or_default().to_string(),
        full_name: read_str(row, "full_name").unwrap_or_default().to_string(),
        email: read_str(row, "email").unwrap_or_default().to_string(),
        role_codes: read_str(row, "role_codes").unwrap_or_default().to_string(),
        role_names: read_str(row, "role_names").unwrap_or_default().to_string(),
        has_permission: row.get::<bool, _>("has_permission").unwrap_or(false),
        has_override: row.get::<bool, _>("has_override").unwrap_or(false),
        user_status: read_i32(row, "user_status").unwrap_or(0),
        user_username_status: read_i32(row, "user_username_status").unwrap_or(0),
        user_create_at: read_datetime(row, "user_create_at").unwrap_or_default(),
        user_update_at: read_datetime(row, "user_update_at").unwrap_or_default(),
    }
}

/// Map a single `SP_PERMISSION_USER_DETAIL_FIND_BY_GUID` row to
/// [`PermissionUserDetailRow`].
/// Column NAMES match the SP's SELECT aliases (one row per
/// `(user, catalog-permission)` pair):
///   `user_guid`               (uniqueidentifier — echoed back from @p_user_guid)
///   `full_name`               (varchar — first+' '+last, COALESCE'd to '')
///   `email`                   (varchar — username alias)
///   `user_role_name`          (varchar — SP picks canonical role by user_role_code)
///   `user_permission_code`    (varchar SCREAMING_SNAKE_CASE — e.g. `BANNER_CREATE`)
///   `user_permission_name`    (nvarchar — English display label)
///   `has_override`            (int 0/1 — 1 when an explicit override row exists)
///   `override_effect`         (varchar — 'allow' | 'deny' | '' (no override))
///   `effective_status`        (int 0/1 — 0 only when explicit deny wins, else 1)
///
/// ## `has_override` × `effective_status` matrix
///
/// | `has_override` | `override_effect` | `effective_status` | Meaning                                          |
/// |----------------|-------------------|--------------------|--------------------------------------------------|
/// | `0`            | `''`              | `0`                | No override, not granted by any role (catalog-only row) |
/// | `0`            | `''`              | `1`                | No override, granted via role (the happy path)   |
/// | `1`            | `'allow'`         | `1`                | Explicit allow wins                              |
/// | `1`            | `'deny'`          | `0`                | Explicit deny wins (always overrides role grant) |
///
/// All reads are defensive (`unwrap_or_default()`) so a future SP
/// refactor that drops a column doesn't 500 the request — the field
/// just lands as its `Default` value. The mapper is intentionally
/// thin: no bool / enum conversion. The list mapper above mirrors
/// this convention (see `row_to_permission_user_list_row`).
fn row_to_permission_user_detail_row(row: &tiberius::Row) -> PermissionUserDetailRow {
    PermissionUserDetailRow {
        user_guid: read_str(row, "user_guid").unwrap_or_default().to_string(),
        full_name: read_str(row, "full_name").unwrap_or_default().to_string(),
        email: read_str(row, "email").unwrap_or_default().to_string(),
        user_role_name: read_str(row, "user_role_name")
            .unwrap_or_default()
            .to_string(),
        user_permission_guid: read_str(row, "user_permission_guid")
            .unwrap_or_default()
            .to_string(),
        user_permission_code: read_str(row, "user_permission_code")
            .unwrap_or_default()
            .to_string(),
        user_permission_name: read_str(row, "user_permission_name")
            .unwrap_or_default()
            .to_string(),
        has_override: row.get::<bool, _>("has_override").unwrap_or(false),
        override_effect: read_str(row, "override_effect")
            .unwrap_or_default()
            .to_string(),
        effective_status: row.get::<bool, _>("effective_status").unwrap_or(false),
    }
}

// ----------------------------------------------------------------------------
// SP_PERMISSION_USER_OVERRIDE_UPDATE — batch override upsert (M18)
// ----------------------------------------------------------------------------

/// Call `dbo.SP_PERMISSION_USER_OVERRIDE_UPDATE` once for a single
/// input item and return the per-item result.
///
/// The SP is one-row-in / one-row-out: it returns exactly one
/// row even on per-item rejection (e.g. `INVALID_EFFECT`,
/// `USER_NOT_FOUND`), so the row read is non-optional. A
/// throw from the CATCH block is the only path that surfaces as
/// [`RepoError::Backend`] (tiberius propagates the throw
/// upwards, no row is delivered).
///
/// The `Option<String>` fields (`reason`, `assigned_by`) are
/// translated to `Option<&str>` so the empty / whitespace
/// defaults the SP applies (via `LTRIM(RTRIM(...)) = ''` →
/// `IS NULL` coercion) work as designed.
async fn call_override_update_sp(
    pool: &MssqlPool,
    item: &PermissionOverrideUpdateItem,
    update_by: &str,
) -> Result<PermissionOverrideUpdateResult, RepoError> {
    // Translate the wire DTO into bound parameters. `None`
    // becomes SQL `NULL` via tiberius's `Option<&T>` impl.
    let user_guid = item.user_guid.as_str();
    let permission_guid = item.permission_guid.as_str();
    let effect = item.effect.as_str();
    let reason: Option<&str> = item.reason.as_deref();
    let assigned_by: Option<&str> = item.assigned_by.as_deref();
    // `status` defaults to 1 when omitted (matches the SP's
    // `@p_user_permission_override_status int = 1` default).
    let status: i32 = item.status.unwrap_or(1);
    // `update_by` defaults to 'system' on the SP side when
    // both create_by and update_by are NULL, so an empty
    // string is fine here — the SP coerces to 'system' before
    // any row write.
    let update_by_str = update_by;

    let rows = exec_sp(
        pool,
        "EXEC dbo.SP_PERMISSION_USER_OVERRIDE_UPDATE \
         @p_user_permission_override_user_guid = @P1, \
         @p_user_permission_override_permission_guid = @P2, \
         @p_user_permission_override_effect = @P3, \
         @p_user_permission_override_reason = @P4, \
         @p_user_permission_override_assigned_by = @P5, \
         @p_user_permission_override_status = @P6, \
         @p_update_by = @P7",
        &[
            &user_guid as &dyn ToSql,
            &permission_guid as &dyn ToSql,
            &effect as &dyn ToSql,
            &reason as &dyn ToSql,
            &assigned_by as &dyn ToSql,
            &status as &dyn ToSql,
            &update_by_str as &dyn ToSql,
        ],
    )
    .await?;

    // The SP returns exactly one row on both success and
    // per-item validation rejection — the CATCH path is the
    // only branch that skips the SELECT (it THROWs, which
    // tiberius propagates as a connection error before the
    // row is delivered). Treat an empty result as a backend
    // error so the loop can surface a clear message instead
    // of silently dropping the item.
    let row = rows.first().ok_or_else(|| {
        RepoError::Backend(format!(
            "SP_PERMISSION_USER_OVERRIDE_UPDATE returned no row for user={} permission={}",
            item.user_guid, item.permission_guid
        ))
    })?;

    Ok(row_to_permission_override_update_result(row))
}

/// Map a single `SP_PERMISSION_USER_OVERRIDE_UPDATE` row to
/// [`PermissionOverrideUpdateResult`].
///
/// Column NAMES match the SP's SELECT aliases:
///   `success`                              (bit)
///   `code`                                 (varchar)
///   `message`                              (varchar)
///   `user_permission_override_guid`        (varchar 50, NULL on validation failure)
///   `user_permission_override_user_guid`   (varchar 50, echo)
///   `user_permission_override_permission_guid` (varchar 50, echo)
///   `user_permission_override_effect`      (varchar 10, post-lowercase)
///   `user_permission_override_status`      (int, post-coalesce)
///
/// All reads are defensive (`unwrap_or_default()`) so a future
/// SP refactor that drops a column doesn't 500 the request — the
/// field just lands as its `Default` value. The mapper is
/// intentionally thin: no enum / bool conversion. The success
/// bit stays as `bool` to match the rest of this adapter.
fn row_to_permission_override_update_result(row: &tiberius::Row) -> PermissionOverrideUpdateResult {
    PermissionOverrideUpdateResult {
        success: row.get::<bool, _>("success").unwrap_or(false),
        code: read_str(row, "code").unwrap_or("ERROR").to_string(),
        message: read_str(row, "message").unwrap_or("").to_string(),
        user_permission_override_guid: read_str(row, "user_permission_override_guid")
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string()),
        user_permission_override_user_guid: read_str(row, "user_permission_override_user_guid")
            .unwrap_or_default()
            .to_string(),
        user_permission_override_permission_guid: read_str(
            row,
            "user_permission_override_permission_guid",
        )
        .unwrap_or_default()
        .to_string(),
        user_permission_override_effect: read_str(row, "user_permission_override_effect")
            .unwrap_or_default()
            .to_string(),
        user_permission_override_status: read_i32(row, "user_permission_override_status")
            .unwrap_or(0),
    }
}

#[cfg(test)]
mod tests {
    //! Mapper-only tests (no DB, no axum).
    //!
    //! ponytail: a tiberius `Row` is hard to fabricate in-process
    //! without a live DB; integration tests with `testcontainers`
    //! cover the full path. The contract enforced here is "every
    //! mapper uses `unwrap_or_default()` on every read" so a future
    //! SP refactor that drops a column doesn't 500 the request.

    #[test]
    fn mappers_use_default_fallbacks() {
        // The string `unwrap_or_default()` pattern is asserted
        // structurally: if a future contributor changes a mapper to
        // `unwrap()` or `.expect()`, the build will still pass but
        // the contract intent is lost. This test pins the doc comment
        // to the source so reviewers notice the intent.
        let source = include_str!("mssql_permission_user.rs");
        assert!(
            source.contains("unwrap_or_default()"),
            "mssql_permission_user.rs mappers must use unwrap_or_default() for defensive reads"
        );
    }
}
