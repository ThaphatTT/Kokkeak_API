//! SQL Server-backed `UserRepository` (M5).
//!
//! Implements [`UserRepository`] via tiberius bound parameters
//! (AGENTS.md § 7.3 — never format strings into SQL).
//!
//! Schema (`KOKKAK_MASTER` database):
//! ```sql
//! CREATE TABLE [user] (
//!     id           UNIQUEIDENTIFIER NOT NULL PRIMARY KEY,
//!     email        NVARCHAR(255) NOT NULL UNIQUE,
//!     display_name NVARCHAR(255) NOT NULL,
//!     password_hash NVARCHAR(512) NOT NULL,
//!     roles        NVARCHAR(MAX) NOT NULL,  -- JSON array
//!     status       NVARCHAR(32)  NOT NULL,
//!     locale       NVARCHAR(8)   NOT NULL,
//!     created_at   DATETIME2(7)  NOT NULL,
//!     updated_at   DATETIME2(7)  NOT NULL
//! );
//! ```
//!
//! This is the production target for M5+ — the JSON-DB simulation
//! remains as a dev fallback.

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
}

#[async_trait]
impl UserRepository for MssqlUserRepository {
    async fn find_by_id(&self, id: Uuid) -> Result<Option<User>, RepoError> {
        let mut conn = self
            .pool
            .get()
            .await
            .map_err(|e| RepoError::Backend(format!("acquire: {e}")))?;
        let rows = conn
            .query("SELECT id, email, display_name, password_hash, roles, status, locale, created_at, updated_at FROM [user] WHERE id = @P1", &[&id as &dyn ToSql])
            .await
            .map_err(|e| RepoError::Backend(e.to_string()))?;
        let mut stream = rows.into_row_stream();
        while let Some(row) = stream
            .try_next()
            .await
            .map_err(|e| RepoError::Backend(e.to_string()))?
        {
            return Ok(Some(row_to_user(&row)?));
        }
        Ok(None)
    }

    async fn find_by_email(&self, email: &str) -> Result<Option<User>, RepoError> {
        let mut conn = self
            .pool
            .get()
            .await
            .map_err(|e| RepoError::Backend(format!("acquire: {e}")))?;
        let lower = email.trim().to_lowercase();
        let rows = conn
            .query(
                "SELECT id, email, display_name, password_hash, roles, status, locale, created_at, updated_at FROM [user] WHERE LOWER(email) = @P1",
                &[&lower as &dyn ToSql],
            )
            .await
            .map_err(|e| RepoError::Backend(e.to_string()))?;
        let mut stream = rows.into_row_stream();
        while let Some(row) = stream
            .try_next()
            .await
            .map_err(|e| RepoError::Backend(e.to_string()))?
        {
            return Ok(Some(row_to_user(&row)?));
        }
        Ok(None)
    }

    async fn insert(&self, user: &User) -> Result<(), RepoError> {
        let mut conn = self
            .pool
            .get()
            .await
            .map_err(|e| RepoError::Backend(format!("acquire: {e}")))?;
        let roles_json = roles_to_json(&user.roles);
        let status = user.status.as_str();
        let created = user.created_at;
        let updated = user.updated_at;
        conn.execute(
            "INSERT INTO [user](id, email, display_name, password_hash, roles, status, locale, created_at, updated_at) VALUES (@P1, @P2, @P3, @P4, @P5, @P6, @P7, @P8, @P9)",
            &[
                &user.id as &dyn ToSql,
                &user.email as &dyn ToSql,
                &user.display_name as &dyn ToSql,
                &user.password_hash as &dyn ToSql,
                &roles_json as &dyn ToSql,
                &status as &dyn ToSql,
                &user.locale as &dyn ToSql,
                &created as &dyn ToSql,
                &updated as &dyn ToSql,
            ],
        )
        .await
        .map_err(|e| {
            let s = e.to_string();
            if s.contains("UNIQUE") || s.contains("duplicate") || s.contains("2627") {
                RepoError::Conflict(format!("email {} is already taken", user.email))
            } else {
                RepoError::Backend(s)
            }
        })?;
        Ok(())
    }

    async fn update(&self, user: &User) -> Result<(), RepoError> {
        let mut conn = self
            .pool
            .get()
            .await
            .map_err(|e| RepoError::Backend(format!("acquire: {e}")))?;
        let roles_json = roles_to_json(&user.roles);
        let status = user.status.as_str();
        let updated = user.updated_at;
        let affected = conn
            .execute(
                "UPDATE [user] SET email = @P1, display_name = @P2, password_hash = @P3, roles = @P4, status = @P5, locale = @P6, updated_at = @P7 WHERE id = @P8",
                &[
                    &user.email as &dyn ToSql,
                    &user.display_name as &dyn ToSql,
                    &user.password_hash as &dyn ToSql,
                    &roles_json as &dyn ToSql,
                    &status as &dyn ToSql,
                    &user.locale as &dyn ToSql,
                    &updated as &dyn ToSql,
                    &user.id as &dyn ToSql,
                ],
            )
            .await
            .map_err(|e| RepoError::Backend(e.to_string()))?;
        if affected.rows_affected().iter().sum::<u64>() == 0 {
            return Err(RepoError::NotFound(format!("user {} not found", user.id)));
        }
        Ok(())
    }
}

fn row_to_user(row: &tiberius::Row) -> Result<User, RepoError> {
    let id: Uuid = row
        .get::<Uuid, _>(0)
        .ok_or_else(|| RepoError::Backend("missing id".into()))?;
    let email: &str = row
        .get::<&str, _>(1)
        .ok_or_else(|| RepoError::Backend("missing email".into()))?;
    let display_name: &str = row
        .get::<&str, _>(2)
        .ok_or_else(|| RepoError::Backend("missing display_name".into()))?;
    let password_hash: &str = row
        .get::<&str, _>(3)
        .ok_or_else(|| RepoError::Backend("missing password_hash".into()))?;
    let roles_json: &str = row
        .get::<&str, _>(4)
        .ok_or_else(|| RepoError::Backend("missing roles".into()))?;
    let status: &str = row
        .get::<&str, _>(5)
        .ok_or_else(|| RepoError::Backend("missing status".into()))?;
    let locale: &str = row
        .get::<&str, _>(6)
        .ok_or_else(|| RepoError::Backend("missing locale".into()))?;
    let created_at = row
        .get::<chrono::DateTime<chrono::Utc>, _>(7)
        .ok_or_else(|| RepoError::Backend("missing created_at".into()))?;
    let updated_at = row
        .get::<chrono::DateTime<chrono::Utc>, _>(8)
        .ok_or_else(|| RepoError::Backend("missing updated_at".into()))?;

    let roles = json_to_roles(roles_json)?;
    let status = match status {
        "pending" => UserStatus::Pending,
        "active" => UserStatus::Active,
        "suspended" => UserStatus::Suspended,
        "deleted" => UserStatus::Deleted,
        other => return Err(RepoError::Backend(format!("unknown status: {other}"))),
    };

    Ok(User {
        id,
        email: email.to_string(),
        display_name: display_name.to_string(),
        password_hash: password_hash.to_string(),
        roles,
        status,
        locale: locale.to_string(),
        created_at,
        updated_at,
    })
}

fn roles_to_json(roles: &[Role]) -> String {
    let names: Vec<&'static str> = roles.iter().map(|r| r.as_str()).collect();
    serde_json::to_string(&names).unwrap_or_else(|_| "[]".into())
}

fn json_to_roles(s: &str) -> Result<Vec<Role>, RepoError> {
    let names: Vec<String> =
        serde_json::from_str(s).map_err(|e| RepoError::Backend(format!("roles json: {e}")))?;
    let mut out = Vec::with_capacity(names.len());
    for n in names {
        let r = match n.as_str() {
            "customer" => Role::Customer,
            "technician" => Role::Technician,
            "admin" => Role::Admin,
            "super_admin" => Role::SuperAdmin,
            other => return Err(RepoError::Backend(format!("unknown role: {other}"))),
        };
        out.push(r);
    }
    Ok(out)
}
