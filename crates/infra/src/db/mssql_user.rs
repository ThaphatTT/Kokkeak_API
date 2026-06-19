//! SQL Server-backed `UserRepository` (M14).
//!
//! Implements [`UserRepository`] via tiberius bound parameters
//! (AGENTS.md § 7.3 — never format strings into SQL).
//!
//! ## Schema (NEW_DB.txt v2 — 4 tables)
//!
//! ```sql
//! -- profile (no auth fields, no email column)
//! [user]              user_guid PK, user_first_name, user_last_name,
//!                     user_status INT, user_create_at, user_update_at, ...
//!
//! -- login + password hash (separated for security)
//! [user_username]     user_username_guid PK, user_username_user_guid FK,
//!                     user_username_username UNIQUE, user_username_password
//!
//! -- role catalog
//! [user_role]         user_role_guid PK, user_role_code UNIQUE, user_role_name UNIQUE
//!
//! -- M:N user <-> role junction
//! [user_user_role]    user_user_role_guid PK, user_user_role_user_guid FK,
//!                     user_user_role_role_guid FK, user_user_role_status, ...
//! ```
//!
//! ## Reads
//!
//! Single SELECT joining all 4 tables. Roles are aggregated as a
//! comma-separated string and split in Rust (tiberius does not
//! stream arrays). Returns `None` when the user has no
//! `[user_username]` row (orphan profile).
//!
//! ## Writes
//!
//! `insert` runs an explicit transaction so the 3 INSERTs
//! (`[user]`, `[user_username]`, one `[user_user_role]` per role)
//! commit atomically. The role GUIDs are looked up from
//! `[user_role]` by code.
//!
//! `update` updates `[user]` + `[user_username]` only (role changes
//! go through a dedicated admin endpoint, M15+).
//!
//! ponytail: the read query is long because the schema spans 4 tables.
//! Ceiling: if hot reads justify it, switch to a stored procedure
//! `API_USER_FIND_BY_ID` / `API_USER_FIND_BY_USERNAME` to keep
//! SQL out of Rust. Defer until the access pattern demands it.

use async_trait::async_trait;
use futures::TryStreamExt;
use tiberius::ToSql;

use kokkak_domain::{RepoError, Role, User, UserRepository, UserStatus};
use uuid::Uuid;

use crate::db::mssql::MssqlPool;

/// Repository handle. Cheap to clone (the pool is `Arc`-shared).
#[derive(Clone)]
pub struct MssqlUserRepository {
    pool: MssqlPool,
}

impl MssqlUserRepository {
    pub fn new(pool: MssqlPool) -> Self {
        Self { pool }
    }

    /// Read the joined row for one user (by guid or username).
    /// Returns `None` when the user profile does not exist OR when
    /// it exists but has no `[user_username]` row.
    ///
    /// `id_filter` / `username_filter` are the `WHERE` predicates;
    /// pass `None` for one when using the other.
    async fn find_joined(
        &self,
        id_filter: Option<Uuid>,
        username_filter: Option<&str>,
    ) -> Result<Option<User>, RepoError> {
        let mut conn = self
            .pool
            .get()
            .await
            .map_err(|e| RepoError::Backend(format!("acquire: {e}")))?;

        // Single JOIN query — roles are aggregated as comma-separated
        // codes. tiberius cannot stream nested arrays, so we collapse
        // them at the SQL boundary and split in Rust.
        let sql = "\
            SELECT \
                u.user_guid, \
                u.user_first_name, \
                u.user_last_name, \
                u.user_status, \
                u.user_create_at, \
                u.user_update_at, \
                un.user_username_username, \
                un.user_username_password, \
                STUFF(( \
                    SELECT ',' + ur.user_role_code \
                    FROM [user_user_role] uur \
                    INNER JOIN [user_role] ur ON ur.user_role_guid = uur.user_user_role_role_guid \
                    WHERE uur.user_user_role_user_guid = u.user_guid \
                      AND uur.user_user_role_status = 1 \
                    FOR XML PATH('') \
                ), 1, 1, '') AS role_codes \
            FROM [user] u \
            INNER JOIN [user_username] un ON un.user_username_user_guid = u.user_guid \
            WHERE u.user_status <> 3 \
              AND (@P1 IS NULL OR u.user_guid = @P1) \
              AND (@P2 IS NULL OR LOWER(un.user_username_username) = LOWER(@P2))";

        let p1: Option<Uuid> = id_filter;
        let p2: Option<String> = username_filter.map(|s| s.trim().to_lowercase());

        let rows = conn
            .query(sql, &[&p1 as &dyn ToSql, &p2 as &dyn ToSql])
            .await
            .map_err(|e| RepoError::Backend(e.to_string()))?;

        let collected: Vec<tiberius::Row> = {
            let mut s = rows.into_row_stream();
            let mut out = Vec::new();
            while let Some(row) = s
                .try_next()
                .await
                .map_err(|e| RepoError::Backend(e.to_string()))?
            {
                out.push(row);
            }
            out
        };

        if let Some(row) = collected.into_iter().next() {
            return Ok(Some(row_to_user(&row)?));
        }
        Ok(None)
    }
}

#[async_trait]
impl UserRepository for MssqlUserRepository {
    async fn find_by_id(&self, id: Uuid) -> Result<Option<User>, RepoError> {
        self.find_joined(Some(id), None).await
    }

    async fn find_by_username(&self, username: &str) -> Result<Option<User>, RepoError> {
        self.find_joined(None, Some(username)).await
    }

    async fn insert(&self, user: &User) -> Result<(), RepoError> {
        let mut conn = self
            .pool
            .get()
            .await
            .map_err(|e| RepoError::Backend(format!("acquire: {e}")))?;

        // Manual transaction on the same connection. tiberius
        // executes each statement as its own batch; BEGIN/COMMIT
        // brackets ensure atomicity across the 3 INSERTs.
        conn.execute("BEGIN TRAN", &[])
            .await
            .map_err(|e| RepoError::Backend(format!("begin tran: {e}")))?;

        let insert_result: Result<(), RepoError> = async {
            // 1) [user] profile row
            let status_i32 = user.status.as_i32();
            conn.execute(
                "INSERT INTO [user] (\
                    user_guid, user_first_name, user_last_name, user_status, \
                    user_create_at, user_create_by, user_update_at, user_update_by\
                ) VALUES (@P1, @P2, @P3, @P4, @P5, @P6, @P7, @P8)",
                &[
                    &user.id as &dyn ToSql,
                    &user.first_name as &dyn ToSql,
                    &user.last_name as &dyn ToSql,
                    &status_i32 as &dyn ToSql,
                    &user.created_at as &dyn ToSql,
                    &user.id as &dyn ToSql, // create_by = self for self-registration
                    &user.updated_at as &dyn ToSql,
                    &user.id as &dyn ToSql, // update_by = self
                ],
            )
            .await
            .map_err(|e| {
                let s = e.to_string();
                RepoError::Backend(format!("insert [user]: {s}"))
            })?;

            // 2) [user_username] credentials row
            let username_id = Uuid::new_v4();
            conn.execute(
                "INSERT INTO [user_username] (\
                    user_username_guid, user_username_user_guid, user_username_username, \
                    user_username_password, user_username_status, \
                    user_username_create_at, user_username_create_by, \
                    user_username_update_at, user_username_update_by\
                ) VALUES (@P1, @P2, @P3, @P4, 1, @P5, @P2, @P6, @P2)",
                &[
                    &username_id as &dyn ToSql,
                    &user.id as &dyn ToSql,
                    &user.username as &dyn ToSql,
                    &user.password_hash as &dyn ToSql,
                    &user.created_at as &dyn ToSql,
                    &user.updated_at as &dyn ToSql,
                ],
            )
            .await
            .map_err(|e| {
                let s = e.to_string();
                if s.contains("UNIQUE") || s.contains("duplicate") || s.contains("2627") {
                    RepoError::Conflict(format!("username {} is already taken", user.username))
                } else {
                    RepoError::Backend(format!("insert [user_username]: {s}"))
                }
            })?;

            // 3) [user_user_role] junction rows — one per role
            for role in &user.roles {
                let code = role.as_str();
                // Look up the role_guid from the seeded catalog
                let role_rows = conn
                    .query(
                        "SELECT user_role_guid FROM [user_role] WHERE user_role_code = @P1 AND user_role_status = 1",
                        &[&code as &dyn ToSql],
                    )
                    .await
                    .map_err(|e| RepoError::Backend(format!("lookup role {code}: {e}")))?;
                let mut collected: Vec<tiberius::Row> = Vec::new();
                {
                    let mut s = role_rows.into_row_stream();
                    while let Some(r) = s
                        .try_next()
                        .await
                        .map_err(|e| RepoError::Backend(format!("role row: {e}")))?
                    {
                        collected.push(r);
                    }
                }
                let role_guid: Option<Uuid> =
                    collected.first().and_then(|r| r.get::<Uuid, _>(0));
                let role_guid = role_guid.ok_or_else(|| {
                    RepoError::Backend(format!("role '{code}' not found in [user_role]"))
                })?;

                let assign_id = Uuid::new_v4();
                conn.execute(
                    "INSERT INTO [user_user_role] (\
                        user_user_role_guid, user_user_role_user_guid, user_user_role_role_guid, \
                        user_user_role_status, user_user_role_assigned_by, user_user_role_assigned_at, \
                        user_user_role_create_at, user_user_role_create_by, \
                        user_user_role_update_at, user_user_role_update_by\
                    ) VALUES (@P1, @P2, @P3, 1, @P2, @P4, @P5, @P2, @P6, @P2)",
                    &[
                        &assign_id as &dyn ToSql,
                        &user.id as &dyn ToSql,
                        &role_guid as &dyn ToSql,
                        &user.created_at as &dyn ToSql,
                        &user.created_at as &dyn ToSql,
                        &user.updated_at as &dyn ToSql,
                    ],
                )
                .await
                .map_err(|e| RepoError::Backend(format!("insert [user_user_role]: {e}")))?;
            }

            Ok(())
        }
        .await;

        match insert_result {
            Ok(()) => {
                conn.execute("COMMIT", &[])
                    .await
                    .map_err(|e| RepoError::Backend(format!("commit: {e}")))?;
                Ok(())
            }
            Err(e) => {
                // Best-effort rollback; ignore failure (auto-rollback
                // happens when the connection is dropped anyway).
                let _ = conn.execute("ROLLBACK", &[]).await;
                Err(e)
            }
        }
    }

    async fn update(&self, user: &User) -> Result<(), RepoError> {
        let mut conn = self
            .pool
            .get()
            .await
            .map_err(|e| RepoError::Backend(format!("acquire: {e}")))?;

        let status_i32 = user.status.as_i32();
        let affected = conn
            .execute(
                "UPDATE [user] SET \
                    user_first_name = @P1, \
                    user_last_name = @P2, \
                    user_status = @P3, \
                    user_update_at = @P4, \
                    user_update_by = @P4 \
                 WHERE user_guid = @P5",
                &[
                    &user.first_name as &dyn ToSql,
                    &user.last_name as &dyn ToSql,
                    &status_i32 as &dyn ToSql,
                    &user.updated_at as &dyn ToSql,
                    &user.id as &dyn ToSql,
                ],
            )
            .await
            .map_err(|e| RepoError::Backend(format!("update [user]: {e}")))?;
        if affected.rows_affected().iter().sum::<u64>() == 0 {
            return Err(RepoError::NotFound(format!("user {} not found", user.id)));
        }

        // Update credentials row in [user_username] (must exist for
        // any user we just updated — find_by_id filters on the JOIN).
        let creds = conn
            .execute(
                "UPDATE [user_username] SET \
                    user_username_password = @P1, \
                    user_username_update_at = @P2, \
                    user_username_update_by = @P3 \
                 WHERE user_username_user_guid = @P4",
                &[
                    &user.password_hash as &dyn ToSql,
                    &user.updated_at as &dyn ToSql,
                    &user.id as &dyn ToSql,
                    &user.id as &dyn ToSql,
                ],
            )
            .await
            .map_err(|e| RepoError::Backend(format!("update [user_username]: {e}")))?;
        if creds.rows_affected().iter().sum::<u64>() == 0 {
            return Err(RepoError::NotFound(format!(
                "credentials for user {} not found",
                user.id
            )));
        }

        Ok(())
    }
}

/// Map a joined row into a `User` aggregate.
fn row_to_user(row: &tiberius::Row) -> Result<User, RepoError> {
    let id: Uuid = row
        .get::<Uuid, _>(0)
        .ok_or_else(|| RepoError::Backend("missing user_guid".into()))?;
    let first_name: &str = row
        .get::<&str, _>(1)
        .ok_or_else(|| RepoError::Backend("missing user_first_name".into()))?;
    let last_name: &str = row
        .get::<&str, _>(2)
        .ok_or_else(|| RepoError::Backend("missing user_last_name".into()))?;
    let status_i32: i32 = row
        .get::<i32, _>(3)
        .ok_or_else(|| RepoError::Backend("missing user_status".into()))?;
    let created_at = row
        .get::<chrono::DateTime<chrono::Utc>, _>(4)
        .ok_or_else(|| RepoError::Backend("missing user_create_at".into()))?;
    let updated_at = row
        .get::<chrono::DateTime<chrono::Utc>, _>(5)
        .ok_or_else(|| RepoError::Backend("missing user_update_at".into()))?;
    let username: &str = row
        .get::<&str, _>(6)
        .ok_or_else(|| RepoError::Backend("missing user_username_username".into()))?;
    let password_hash: &str = row
        .get::<&str, _>(7)
        .ok_or_else(|| RepoError::Backend("missing user_username_password".into()))?;
    let role_codes: Option<&str> = row.get::<&str, _>(8);

    let status = UserStatus::from_i32(status_i32)
        .ok_or_else(|| RepoError::Backend(format!("unknown user_status: {status_i32}")))?;
    let roles = parse_role_codes(role_codes.unwrap_or(""));

    Ok(User {
        id,
        first_name: first_name.to_string(),
        last_name: last_name.to_string(),
        username: username.to_string(),
        password_hash: password_hash.to_string(),
        roles,
        status,
        created_at,
        updated_at,
    })
}

/// Split the comma-separated role_codes string from the JOIN aggregate.
/// Empty string → empty `Vec<Role>`.
fn parse_role_codes(s: &str) -> Vec<Role> {
    s.split(',')
        .filter(|c| !c.is_empty())
        .filter_map(|code| Role::from_code(code.trim()))
        .collect()
}
