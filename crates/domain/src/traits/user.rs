//! User repository port (พอร์ต User repository — M2 + M14).
//!
//! Returns `None` when the entity is missing; returns `Err(...)` when
//! the operation could not be performed (DB down, constraint
//! violation, etc.). `RepoError` is the **only** error type a
//! repository raises — adapters translate driver-specific errors
//! into the variants below.
//!
//! M14 renamed the login lookup from `find_by_email` → `find_by_username`
//! because NEW_DB `[user]` has no email column; login is now
//! `[user_username].user_username_username` (a free-form login id —
//! email, phone, or alphanumeric handle).

use async_trait::async_trait;
use thiserror::Error;
use uuid::Uuid;

use crate::user::User;

/// Repository-level error (one of the few `domain` types that maps
/// 1:1 to an HTTP status).
#[derive(Debug, Error)]
pub enum RepoError {
    /// 404 — entity not found.
    #[error("not found: {0}")]
    NotFound(String),

    /// 409 — unique-key violation (e.g. duplicate username).
    #[error("conflict: {0}")]
    Conflict(String),

    /// 500 — backend (DB / network) failure.
    #[error("backend error: {0}")]
    Backend(String),
}

/// User repository contract (สัญญา User repository).
#[async_trait]
pub trait UserRepository: Send + Sync {
    /// Find a user by primary key (`[user].user_guid`).
    async fn find_by_id(&self, id: Uuid) -> Result<Option<User>, RepoError>;

    /// Find a user by lowercased login username
    /// (`[user_username].user_username_username`). This is the
    /// canonical login lookup.
    async fn find_by_username(&self, username: &str) -> Result<Option<User>, RepoError>;

    /// Persist a brand-new user. Returns `Conflict` when the
    /// username is already taken.
    ///
    /// Implementations are responsible for inserting into all four
    /// NEW_DB tables atomically (`[user]` + `[user_username]` +
    /// `[user_user_role]` + the role-lookup against `[user_role]`).
    /// Adapters that cannot guarantee atomicity (e.g. JSON-DB sim)
    /// must document that deviation.
    async fn insert(&self, user: &User) -> Result<(), RepoError>;

    /// Replace an existing user. Returns `NotFound` if the id does
    /// not exist. Currently updates `[user]` + `[user_username]` only;
    /// role changes go through a dedicated admin endpoint (planned
    /// M15+).
    async fn update(&self, user: &User) -> Result<(), RepoError>;
}
