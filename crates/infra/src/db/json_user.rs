//! JSON-file-backed `UserRepository` (M2).
//!
//! Implements [`UserRepository`] using [`JsonStore`]. Production
//! swaps this for the tiberius-backed implementation (M5+).

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

    async fn find_by_email(&self, email: &str) -> Result<Option<User>, RepoError> {
        let lower = email.trim().to_lowercase();
        Ok(self
            .store
            .find_by(|u| u.email.to_lowercase() == lower)
            .await)
    }

    async fn insert(&self, user: &User) -> Result<(), RepoError> {
        if self.store.contains_key(&user.id.to_string()).await {
            return Err(RepoError::Conflict(format!(
                "user with id {} already exists",
                user.id
            )));
        }
        let lower = user.email.to_lowercase();
        if self
            .store
            .find_by(|u| u.email.to_lowercase() == lower)
            .await
            .is_some()
        {
            return Err(RepoError::Conflict(format!(
                "email {} is already taken",
                user.email
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

    fn sample_user(email: &str) -> User {
        User {
            id: Uuid::new_v4(),
            email: email.into(),
            display_name: "Alice".into(),
            password_hash: "$argon2id$...".into(),
            roles: vec![Role::Customer],
            status: UserStatus::Active,
            locale: "lo".into(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[tokio::test]
    async fn insert_and_find_by_id() {
        let path = tmp("u1.json");
        let _ = std::fs::remove_file(&path);
        let repo = JsonUserRepository::open(&path).await.unwrap();
        let u = sample_user("a@b.com");
        let id = u.id;
        repo.insert(&u).await.unwrap();
        let got = repo.find_by_id(id).await.unwrap().unwrap();
        assert_eq!(got.email, "a@b.com");
    }

    #[tokio::test]
    async fn find_by_email_is_case_insensitive() {
        let path = tmp("u2.json");
        let _ = std::fs::remove_file(&path);
        let repo = JsonUserRepository::open(&path).await.unwrap();
        let u = sample_user("A@B.com");
        repo.insert(&u).await.unwrap();
        let got = repo.find_by_email("a@b.com").await.unwrap().unwrap();
        assert_eq!(got.id, u.id);
    }

    #[tokio::test]
    async fn duplicate_email_returns_conflict() {
        let path = tmp("u3.json");
        let _ = std::fs::remove_file(&path);
        let repo = JsonUserRepository::open(&path).await.unwrap();
        let u1 = sample_user("a@b.com");
        let mut u2 = sample_user("a@b.com");
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
        let mut u = sample_user("a@b.com");
        repo.insert(&u).await.unwrap();
        u.display_name = "Bob".into();
        repo.update(&u).await.unwrap();
        let got = repo.find_by_id(u.id).await.unwrap().unwrap();
        assert_eq!(got.display_name, "Bob");
    }

    #[tokio::test]
    async fn update_missing_returns_not_found() {
        let path = tmp("u5.json");
        let _ = std::fs::remove_file(&path);
        let repo = JsonUserRepository::open(&path).await.unwrap();
        let u = sample_user("a@b.com");
        let err = repo.update(&u).await.unwrap_err();
        assert!(matches!(err, RepoError::NotFound(_)));
    }
}
