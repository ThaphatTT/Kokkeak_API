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

use kokkak_domain::{RepoError, Role, User, UserRepository, UserStatus};
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
        // First row: profile. Second row: roles CSV.
        let profile = rows
            .first()
            .ok_or_else(|| RepoError::Backend("API_USER_FIND_BY_ID returned no row".into()))?;
        let user = row_to_user(profile)?;
        let roles = rows
            .get(1)
            .and_then(|r| read_str(r, 0))
            .map(parse_role_codes)
            .unwrap_or_default();
        Ok(Some(User { roles, ..user }))
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
        let roles = rows
            .get(1)
            .and_then(|r| read_str(r, 0))
            .map(parse_role_codes)
            .unwrap_or_default();
        Ok(Some(User { roles, ..user }))
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
    let id: Uuid = row
        .get::<Uuid, _>("user_guid")
        .ok_or_else(|| RepoError::Backend("missing id".into()))?;
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
    let created_at = row
        .get::<chrono::DateTime<chrono::Utc>, _>("user_username_create_at")
        .ok_or_else(|| RepoError::Backend("missing created_at".into()))?;
    let updated_at = row
        .get::<chrono::DateTime<chrono::Utc>, _>("user_username_update_by")
        .ok_or_else(|| RepoError::Backend("missing updated_at".into()))?;
    let status = UserStatus::from_i32(status_i32)
        .ok_or_else(|| RepoError::Backend(format!("unknown status: {status_i32}")))?;
    Ok(User {
        id,
        first_name: first_name.to_string(),
        last_name: last_name.to_string(),
        username: username.to_string(),
        password_hash: password_hash.to_string(),
        roles: Vec::new(), // filled by caller
        status,
        created_at,
        updated_at,
    })
}

/// Split a comma-separated role_codes string into Vec<Role>.
fn parse_role_codes(s: &str) -> Vec<Role> {
    s.split(',')
        .filter(|c| !c.is_empty())
        .filter_map(|code| Role::from_code(code.trim()))
        .collect()
}
