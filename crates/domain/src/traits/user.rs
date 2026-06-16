//! User repository port (พอร์ต User repository — M2).
//!
//! Returns `None` when the entity is missing; returns `Err(...)` when
//! the operation could not be performed (DB down, constraint
//! violation, etc.). `RepoError` is the **only** error type a
//! repository raises — adapters translate driver-specific errors
//! into the variants below.

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

    /// 409 — unique-key violation (e.g. duplicate email).
    #[error("conflict: {0}")]
    Conflict(String),

    /// 500 — backend (DB / network) failure.
    #[error("backend error: {0}")]
    Backend(String),
}

/// User repository contract (สัญญา User repository).
#[async_trait]
pub trait UserRepository: Send + Sync {
    /// Find a user by primary key.
    async fn find_by_id(&self, id: Uuid) -> Result<Option<User>, RepoError>;

    /// Find a user by lowercased email (the canonical login lookup).
    async fn find_by_email(&self, email: &str) -> Result<Option<User>, RepoError>;

    /// Persist a brand-new user. Returns `Conflict` when the email
    /// is already taken.
    async fn insert(&self, user: &User) -> Result<(), RepoError>;

    /// Replace an existing user. Returns `NotFound` if the id does
    /// not exist.
    async fn update(&self, user: &User) -> Result<(), RepoError>;
}
