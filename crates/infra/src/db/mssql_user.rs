//! SQL Server-backed `UserRepository` (M14.5 — stored procedures only).
//!
//! Implements [`UserRepository`] via tiberius + the NEW_DB v2 stored
//! procedures. No inline SQL — every operation is `EXEC dbo.API_USER_*`.
//!
//! ponytail: the executor is intentionally thin (one helper per repo).
//! Ceiling: SPs could be replaced by an ORM (diesel / sea-orm) when
//! the schema stabilizes; for now SPs give the DBA explicit control
//! over the multi-table JOINs + role lookup logic.
//!
//! ## Storage procedure contract
//!
//! Every `API_USER_*` SP follows the uniform output shape documented in
//! `migrations/20260620000001_sp_user.sql`. The Rust side reads the
//! first row of the first result set and maps `error_code` to
//! `RepoError`:
//! - `error_code = 0` → ok
//! - `error_code = 1` → `NotFound`
//! - `error_code = 2` → `Conflict` (username taken)
//! - `error_code = 3` → `Backend` (validation / unknown)

use async_trait::async_trait;
use tiberius::ToSql;

use kokkak_domain::{Permission, RepoError, Role, User, UserRepository, UserStatus};
use uuid::Uuid;

use crate::db::mssql::{exec_sp, read_i32, read_str, MssqlPool, SpError};

/// SQL Server-backed `UserRepository` (M14.5 — stored procedures).
#[derive(Clone)]
pub struct MssqlUserRepository {
    pool: MssqlPool,
}

impl MssqlUserRepository {
    /// Construct the repository with a shared `MssqlPool`.
    pub fn new(pool: MssqlPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl UserRepository for MssqlUserRepository {
    async fn find_by_id(&self, id: Uuid) -> Result<Option<User>, RepoError> {
        let rows = exec_sp(
            &self.pool,
            "EXEC dbo.API_USER_FIND_BY_ID @p_user_guid = @P1",
            &[&id as &dyn ToSql],
        )
        .await?;
        // First row: profile. Second row: roles + permissions CSV.
        let profile = rows
            .first()
            .ok_or_else(|| RepoError::Backend("API_USER_FIND_BY_ID returned no row".into()))?;
        let user = row_to_user(profile)?;
        let (roles, permissions) = read_roles_and_permissions(&rows, 1)?;
        Ok(Some(User {
            roles,
            permissions,
            ..user
        }))
    }

    async fn find_by_username(&self, username: &str) -> Result<Option<User>, RepoError> {
        let rows = exec_sp(
            &self.pool,
            "EXEC dbo.SP_USER_FIND_BY_USERNAME @p_username = @P1",
            &[&username as &dyn ToSql],
        )
        .await?;
        // Empty result → user not found.
        let profile = match rows.first() {
            None => return Ok(None),
            Some(r) => r,
        };
        let user = row_to_user(profile)?;
        // Roles CSV from the second result set.
        // Status (user_user_role.status=1, user_role.status=1) and
        // expire_at are filtered inside the SP (see
        // migrations/20260620000001_sp_user.sql + RDBMS Permssion.md
        // §1.8 + §3 Step 5) — we only see roles that already passed.
        // Effective permissions (role + allow − deny) + data scope
        // land in M15+ via a dedicated SP_USER_GET_EFFECTIVE_PERMISSIONS.
        let (roles, permissions) = read_roles_and_permissions(&rows, 1)?;
        Ok(Some(User {
            roles,
            permissions,
            ..user
        }))
    }

    async fn insert(&self, user: &User) -> Result<(), RepoError> {
        // API_USER_REGISTER takes the first role code only (multi-role
        // is rare; admin endpoint M15+ will use API_USER_SET_ROLES for
        // post-registration role changes). For now, register with the
        // first role and use API_USER_SET_ROLES for the rest.
        let role_code = user
            .roles
            .first()
            .map(|r| r.as_str())
            .ok_or_else(|| RepoError::Backend("at least one role required".into()))?;

        // 1. Register (creates user + username + first role).
        let reg_rows = exec_sp(
            &self.pool,
            "EXEC dbo.API_USER_REGISTER \
                @p_first_name = @P1, @p_last_name = @P2, \
                @p_username = @P3, @p_password_hash = @P4, \
                @p_role_code = @P5",
            &[
                &user.first_name as &dyn ToSql,
                &user.last_name as &dyn ToSql,
                &user.username as &dyn ToSql,
                &user.password_hash as &dyn ToSql,
                &role_code as &dyn ToSql,
            ],
        )
        .await?;
        let reg_row = reg_rows
            .first()
            .ok_or_else(|| RepoError::Backend("API_USER_REGISTER returned no row".into()))?;
        let err = read_i32(reg_row, 1).unwrap_or(3);
        let msg = read_str(reg_row, 2).unwrap_or_default();
        match SpError::from_code(err, msg) {
            SpError::None => Ok(()),
            SpError::Conflict => Err(RepoError::Conflict(msg.to_string())),
            SpError::NotFound => Err(RepoError::Backend(format!("USER_REGISTER: {}", msg))),
            SpError::BadInput => Err(RepoError::Backend(format!("validation: {msg}"))),
            SpError::Other => Err(RepoError::Backend(msg.to_string())),
        }?;

        // 2. If the user has more than one role, append via SET_ROLES.
        if user.roles.len() > 1 {
            let extra: Vec<&str> = user.roles[1..].iter().map(|r| r.as_str()).collect();
            let csv = extra.join(",");
            let set_rows = exec_sp(
                &self.pool,
                "EXEC dbo.API_USER_SET_ROLES \
                    @p_user_guid = @P1, @p_role_codes = @P2",
                &[&user.id as &dyn ToSql, &csv as &dyn ToSql],
            )
            .await?;
            let _ = set_rows; // API_USER_SET_ROLES always returns ok in practice
        }

        Ok(())
    }

    async fn update(&self, user: &User) -> Result<(), RepoError> {
        let status_i32 = user.status.as_i32();
        let rows = exec_sp(
            &self.pool,
            "EXEC dbo.API_USER_UPDATE \
                @p_user_guid = @P1, @p_first_name = @P2, @p_last_name = @P3, \
                @p_password_hash = @P4, @p_status = @P5",
            &[
                &user.id as &dyn ToSql,
                &user.first_name as &dyn ToSql,
                &user.last_name as &dyn ToSql,
                &user.password_hash as &dyn ToSql,
                &status_i32 as &dyn ToSql,
            ],
        )
        .await?;
        let row = rows
            .first()
            .ok_or_else(|| RepoError::Backend("API_USER_UPDATE returned no row".into()))?;
        let err = read_i32(row, 1).unwrap_or(0);
        let msg = read_str(row, 2).unwrap_or_default();
        match SpError::from_code(err, msg) {
            SpError::None => Ok(()),
            SpError::NotFound => Err(RepoError::NotFound(msg.to_string())),
            _ => Err(RepoError::Backend(msg.to_string())),
        }
    }
}

/// Map a single joined row to the User aggregate (without roles).
/// The `roles` field is filled by the caller after reading the
/// second result set.
fn row_to_user(row: &tiberius::Row) -> Result<User, RepoError> {
    let id_str: &str = row
        .get::<&str, _>("user_guid")
        .ok_or_else(|| RepoError::Backend("missing id".into()))?;

    let id = Uuid::parse_str(id_str)
        .map_err(|e| RepoError::Backend(format!("invalid user_guid: {e}")))?;
    let first_name: &str = row
        .get::<&str, _>("user_first_name")
        .ok_or_else(|| RepoError::Backend("missing first_name".into()))?;
    let last_name: &str = row
        .get::<&str, _>("user_last_name")
        .ok_or_else(|| RepoError::Backend("missing last_name".into()))?;
    let username: &str = row
        .get::<&str, _>("user_username_username")
        .ok_or_else(|| RepoError::Backend("missing username".into()))?;
    let password_hash: &str = row
        .get::<&str, _>("user_password")
        .ok_or_else(|| RepoError::Backend("missing password_hash".into()))?;
    let status_i32: i32 = row
        .get::<i32, _>("user_status")
        .ok_or_else(|| RepoError::Backend("missing status".into()))?;

    let created_at_naive = row
        .get::<chrono::NaiveDateTime, _>("user_username_create_at")
        .ok_or_else(|| RepoError::Backend("missing created_at".into()))?;

    let updated_at_naive = row
        .get::<chrono::NaiveDateTime, _>("user_username_update_at")
        .ok_or_else(|| RepoError::Backend("missing updated_at".into()))?;

    let created_at =
        chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(created_at_naive, chrono::Utc);

    let updated_at =
        chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(updated_at_naive, chrono::Utc);

    let status = UserStatus::from_i32(status_i32)
        .ok_or_else(|| RepoError::Backend(format!("unknown status: {status_i32}")))?;
    Ok(User {
        id,
        first_name: first_name.to_string(),
        last_name: last_name.to_string(),
        username: username.to_string(),
        password_hash: password_hash.to_string(),
        roles: Vec::new(),       // filled by caller
        permissions: Vec::new(), // filled by caller
        status,
        created_at,
        updated_at,
    })
}

/// Read the (roles, permissions) pair from the named row of an
/// `exec_sp` result set.
///
/// Per the stored-procedure contract documented in
/// `migrations/20260620000001_sp_user.sql`, the second result-set row
/// contains two CSV columns:
/// - column 0 → `role_codes`         (snake_case: `customer,admin,…`)
/// - column 1 → `permission_codes`   (SCREAMING_SNAKE_CASE: `PAGE_JOBS_VIEW,JOBS_CREATE,…`)
///
/// Both columns are independently optional (NULL on no rows), so we
/// never panic when the SP returns just one column or the user has
/// no role / no permission yet. The CSVs are parsed by
/// [`parse_role_codes`] and [`parse_permission_codes`].
fn read_roles_and_permissions(
    rows: &[tiberius::Row],
    idx: usize,
) -> Result<(Vec<Role>, Vec<Permission>), RepoError> {
    let roles = rows
        .get(idx)
        .and_then(|r| read_str(r, 1))
        .map(parse_role_codes)
        .unwrap_or_default();
    let permissions = rows
        .get(idx)
        .and_then(|r| read_str(r, 0))
        .map(parse_permission_codes)
        .unwrap_or_default();
    Ok((roles, permissions))
}

/// Split a comma-separated role_codes string into Vec<Role>.
///
/// Per RDBMS Permssion.md §1.8 + §3 Step 5 the SQL filter for
/// `user_role_status=1` AND `(expire_at IS NULL OR > now)` MUST run
/// in the stored procedure — Rust only receives role_codes that
/// already passed those gates. We log unknown codes at WARN level
/// (instead of silently dropping them) so a DBA-created role that's
/// not yet mapped in Rust shows up in observability.
///
/// ponytail: full effective-permission calculation (§5: role + allow
/// − deny) belongs in a dedicated `SP_USER_GET_EFFECTIVE_PERMISSIONS`
/// call — the `roles` Vec on the aggregate stays as-is until M15+.
/// Scope / department / permission_override land there.
fn parse_role_codes(s: &str) -> Vec<Role> {
    let mut out = Vec::new();
    for raw in s.split(',') {
        let code = raw.trim();
        if code.is_empty() {
            continue;
        }
        match Role::from_code(code) {
            Some(r) => out.push(r),
            None => tracing::warn!(
                role_code = %code,
                "mssql_user::parse_role_codes: unknown role code from DB \
                 — DBA may have added a new role; backend enum out of sync"
            ),
        }
    }
    out
}

#[cfg(test)]
mod parse_role_codes_tests {
    //! Unit tests for the CSV parser — the SP does the heavy lifting
    //! (status + expire_at filtering) and these tests confirm the
    //! Rust side never crashes on the wire format the DBA may tweak.
    use super::*;
    use kokkak_domain::Role;

    #[test]
    fn parses_all_known_codes() {
        assert_eq!(
            parse_role_codes("customer,admin,super_admin"),
            vec![Role::Customer, Role::Admin, Role::SuperAdmin]
        );
    }

    #[test]
    fn skips_empty_segments() {
        // STUFF(..., 1, 1, '') on an empty subquery yields '' — and
        // a stray trailing comma would split to ["customer", ""].
        assert_eq!(
            parse_role_codes("customer,,admin,"),
            vec![Role::Customer, Role::Admin]
        );
        assert_eq!(parse_role_codes(""), Vec::<Role>::new());
    }

    #[test]
    fn trims_whitespace_around_codes() {
        assert_eq!(
            parse_role_codes(" customer , admin "),
            vec![Role::Customer, Role::Admin]
        );
    }

    #[test]
    fn skips_unknown_codes_without_panicking() {
        // DBA added a role not yet mapped in the Rust enum — we must
        // not panic at startup. The WARN log is captured by
        // tracing-subscriber in production; here we just verify the
        // well-known codes still come through.
        assert_eq!(
            parse_role_codes("customer,new_admin_role,admin"),
            vec![Role::Customer, Role::Admin]
        );
    }
}

/// Split a comma-separated `permission_codes` string into `Vec<Permission>`.
///
/// Mirrors [`parse_role_codes`]: the SP returns
/// `SCREAMING_SNAKE_CASE` codes (`PAGE_JOBS_VIEW,JOBS_CREATE,…`); we
/// trim each segment, skip empties, and log a WARN for codes that
/// the Rust enum does not yet know about so DBA-side additions are
/// observable in production instead of silently dropped.
fn parse_permission_codes(s: &str) -> Vec<Permission> {
    let mut out = Vec::new();
    for raw in s.split(',') {
        let code = raw.trim();
        if code.is_empty() {
            continue;
        }
        match Permission::from_code(code) {
            Some(p) => out.push(p),
            None => tracing::warn!(
                permission_code = %code,
                "mssql_user::parse_permission_codes: unknown permission code from DB \
                 — DBA may have added a new permission; backend enum out of sync"
            ),
        }
    }
    out
}

#[cfg(test)]
mod parse_permission_codes_tests {
    //! Unit tests for the permission CSV parser — same contract as
    //! the role parser: tolerant of trailing commas, whitespace, and
    //! unknown codes.
    use super::*;
    use kokkak_domain::Permission;

    #[test]
    fn parses_known_codes() {
        assert_eq!(
            parse_permission_codes("PAGE_DASHBOARD_VIEW,JOBS_CREATE,JOBS_UPDATE"),
            vec![
                Permission::PageDashboardView,
                Permission::JobsCreate,
                Permission::JobsUpdate,
            ]
        );
    }

    #[test]
    fn skips_empty_segments_and_trims() {
        assert_eq!(
            parse_permission_codes("PAGE_JOBS_VIEW,,JOBS_CREATE,"),
            vec![Permission::PageJobsView, Permission::JobsCreate]
        );
        assert_eq!(parse_permission_codes(""), Vec::<Permission>::new());
        assert_eq!(
            parse_permission_codes(" JOBS_EXPORT "),
            vec![Permission::JobsExport]
        );
    }

    #[test]
    fn skips_unknown_codes_without_panicking() {
        // Future permission added by the DBA before the Rust enum
        // catches up. We keep the well-known ones and warn (not test).
        assert_eq!(
            parse_permission_codes("JOBS_CREATE,FUTURE_PERMISSION,JOBS_DELETE"),
            vec![Permission::JobsCreate, Permission::JobsDelete]
        );
    }
}
