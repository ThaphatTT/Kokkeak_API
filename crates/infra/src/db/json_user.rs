//! JSON-file-backed `UserRepository` (M2 + M14).
//!
//! Implements [`UserRepository`] using [`JsonStore`]. Production
//! swaps this for the tiberius-backed implementation that JOINs the
//! 4 NEW_DB tables.
//!
//! **M14 schema note**: the JSON-DB sim keeps the same single-file
//! shape (one `User` aggregate per record, keyed by user_guid). The
//! 4-table normalization in NEW_DB.txt is hidden behind the
//! repository port — the sim is just a dev convenience that
//! persists the aggregate shape directly. No migration is required
//! because tests always `remove_file` before each run.

use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use kokkak_domain::{RepoError, User, UserRepository};
use uuid::Uuid;

use crate::db::json::JsonStore;

/// Repository handle (Arc-shared cheap clone).
#[derive(Clone)]
pub struct JsonUserRepository {
    store: Arc<JsonStore<User>>,
}

impl JsonUserRepository {
    /// Open (or create) the JSON store at `path`. The primary key is
    /// the user UUID.
    pub async fn open(path: impl AsRef<Path>) -> Result<Self, RepoError> {
        let store = JsonStore::open(path.as_ref(), |u: &User| u.id.to_string())
            .await
            .map_err(|e| RepoError::Backend(e.to_string()))?;
        Ok(Self {
            store: Arc::new(store),
        })
    }

    /// File path on disk (for diagnostics).
    pub fn path(&self) -> &Path {
        self.store.path()
    }
}

#[async_trait]
impl UserRepository for JsonUserRepository {
    async fn find_by_id(&self, id: Uuid) -> Result<Option<User>, RepoError> {
        Ok(self.store.find(&id.to_string()).await)
    }

    async fn find_by_username(&self, username: &str) -> Result<Option<User>, RepoError> {
        let lower = username.trim().to_lowercase();
        Ok(self
            .store
            .find_by(|u| u.username.to_lowercase() == lower)
            .await)
    }

    async fn insert(&self, user: &User) -> Result<(), RepoError> {
        if self.store.contains_key(&user.id.to_string()).await {
            return Err(RepoError::Conflict(format!(
                "user with id {} already exists",
                user.id
            )));
        }
        let lower = user.username.to_lowercase();
        if self
            .store
            .find_by(|u| u.username.to_lowercase() == lower)
            .await
            .is_some()
        {
            return Err(RepoError::Conflict(format!(
                "username {} is already taken",
                user.username
            )));
        }
        self.store
            .upsert(user)
            .await
            .map_err(|e| RepoError::Backend(e.to_string()))?;
        Ok(())
    }

    async fn update(&self, user: &User) -> Result<(), RepoError> {
        if !self.store.contains_key(&user.id.to_string()).await {
            return Err(RepoError::NotFound(format!("user {} not found", user.id)));
        }
        self.store
            .upsert(user)
            .await
            .map_err(|e| RepoError::Backend(e.to_string()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use kokkak_domain::{Role, UserStatus};

    fn tmp(name: &str) -> std::path::PathBuf {
        std::env::temp_dir()
            .join("kokkak_user_repo_test")
            .join(name)
    }

    fn sample_user(username: &str) -> User {
        User {
            id: Uuid::new_v4(),
            first_name: "Alice".into(),
            last_name: "Wonder".into(),
            username: username.into(),
            password_hash: "$argon2id$...".into(),
            roles: vec![Role::Customer],
            status: UserStatus::Active,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[tokio::test]
    async fn insert_and_find_by_id() {
        let path = tmp("u1.json");
        let _ = std::fs::remove_file(&path);
        let repo = JsonUserRepository::open(&path).await.unwrap();
        let u = sample_user("alice");
        let id = u.id;
        repo.insert(&u).await.unwrap();
        let got = repo.find_by_id(id).await.unwrap().unwrap();
        assert_eq!(got.username, "alice");
        assert_eq!(got.first_name, "Alice");
    }

    #[tokio::test]
    async fn find_by_username_is_case_insensitive() {
        let path = tmp("u2.json");
        let _ = std::fs::remove_file(&path);
        let repo = JsonUserRepository::open(&path).await.unwrap();
        let u = sample_user("Alice");
        repo.insert(&u).await.unwrap();
        let got = repo.find_by_username("alice").await.unwrap().unwrap();
        assert_eq!(got.id, u.id);
        let got2 = repo.find_by_username("ALICE").await.unwrap().unwrap();
        assert_eq!(got2.id, u.id);
    }

    #[tokio::test]
    async fn duplicate_username_returns_conflict() {
        let path = tmp("u3.json");
        let _ = std::fs::remove_file(&path);
        let repo = JsonUserRepository::open(&path).await.unwrap();
        let u1 = sample_user("alice");
        let mut u2 = sample_user("alice");
        u2.id = Uuid::new_v4();
        repo.insert(&u1).await.unwrap();
        let err = repo.insert(&u2).await.unwrap_err();
        assert!(matches!(err, RepoError::Conflict(_)));
    }

    #[tokio::test]
    async fn update_replaces_existing() {
        let path = tmp("u4.json");
        let _ = std::fs::remove_file(&path);
        let repo = JsonUserRepository::open(&path).await.unwrap();
        let mut u = sample_user("alice");
        repo.insert(&u).await.unwrap();
        u.first_name = "Bob".into();
        repo.update(&u).await.unwrap();
        let got = repo.find_by_id(u.id).await.unwrap().unwrap();
        assert_eq!(got.first_name, "Bob");
    }

    #[tokio::test]
    async fn update_missing_returns_not_found() {
        let path = tmp("u5.json");
        let _ = std::fs::remove_file(&path);
        let repo = JsonUserRepository::open(&path).await.unwrap();
        let u = sample_user("alice");
        let err = repo.update(&u).await.unwrap_err();
        assert!(matches!(err, RepoError::NotFound(_)));
    }
}
