//! User use cases (M2 + M14).
//!
//! - `get_me`: fetch the public view of the current user.
//! - `get_user`: fetch the full `User` aggregate (used by chat /
//!   payment use cases that need the role list / status).

use std::sync::Arc;

use kokkak_domain::{AuthError, PublicUser, UserRepository};
use uuid::Uuid;

/// User use case bundle (M2 + M14).
pub struct UserService {
    users: Arc<dyn UserRepository>,
}

impl UserService {
    /// Construct the service with a `UserRepository` port.
    pub fn new(users: Arc<dyn UserRepository>) -> Self {
        Self { users }
    }

    /// Fetch the public view of the given user (used by the `GET /users/me` route).
    pub async fn get_me(&self, user_id: Uuid) -> Result<PublicUser, AuthError> {
        let user = self
            .users
            .find_by_id(user_id)
            .await
            .map_err(|e| AuthError::Backend(e.to_string()))?
            .ok_or(AuthError::InvalidCredentials)?;
        Ok(PublicUser::from(&user))
    }

    /// Fetch the full `User` (used by chat + payment use cases
    /// that need the role list / status).
    pub async fn get_user(&self, user_id: Uuid) -> Result<kokkak_domain::User, AuthError> {
        self.users
            .find_by_id(user_id)
            .await
            .map_err(|e| AuthError::Backend(e.to_string()))?
            .ok_or(AuthError::InvalidCredentials)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// In-memory mock of UserRepository for unit tests.
    /// Stores users in a HashMap; collision on username returns Conflict.
    #[derive(Default)]
    struct MockUserRepository {
        by_id: std::sync::Mutex<std::collections::HashMap<uuid::Uuid, User>>,
        by_username: std::sync::Mutex<std::collections::HashMap<String, Uuid>>,
    }

    #[async_trait::async_trait]
    impl UserRepository for MockUserRepository {
        async fn find_by_id(&self, id: Uuid) -> Result<Option<User>, kokkak_domain::RepoError> {
            Ok(self.by_id.lock().unwrap().get(&id).cloned())
        }
        async fn find_by_username(
            &self,
            username: &str,
        ) -> Result<Option<User>, kokkak_domain::RepoError> {
            let key = username.trim().to_lowercase();
            let by_un = self.by_username.lock().unwrap();
            let by_id = self.by_id.lock().unwrap();
            Ok(by_un.get(&key).and_then(|id| by_id.get(id).cloned()))
        }
        async fn insert(&self, user: &User) -> Result<(), kokkak_domain::RepoError> {
            let key = user.username.trim().to_lowercase();
            let mut by_un = self.by_username.lock().unwrap();
            if by_un.contains_key(&key) {
                return Err(kokkak_domain::RepoError::Conflict(format!(
                    "username {} taken",
                    user.username
                )));
            }
            by_un.insert(key, user.id);
            self.by_id.lock().unwrap().insert(user.id, user.clone());
            Ok(())
        }
        async fn update(&self, user: &User) -> Result<(), kokkak_domain::RepoError> {
            let mut by_id = self.by_id.lock().unwrap();
            if !by_id.contains_key(&user.id) {
                return Err(kokkak_domain::RepoError::NotFound(format!(
                    "user {} not found",
                    user.id
                )));
            }
            by_id.insert(user.id, user.clone());
            Ok(())
        }
    }

    use chrono::Utc;
    use kokkak_domain::{Role, User, UserStatus};
    // skip MssqlUserRepository;

    #[tokio::test]
    async fn get_me_returns_public_view() {
        let repo: Arc<dyn UserRepository> = Arc::new(MockUserRepository::default());
        let id = Uuid::new_v4();
        let u = User {
            id,
            first_name: "A".into(),
            last_name: "B".into(),
            username: "ab".into(),
            password_hash: "$argon2".into(),
            roles: vec![Role::Customer],
            status: UserStatus::Active,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        repo.insert(&u).await.unwrap();
        let svc = UserService::new(repo);
        let me = svc.get_me(id).await.unwrap();
        assert_eq!(me.username, "ab");
        assert_eq!(me.first_name, "A");
        assert_eq!(me.last_name, "B");
    }

    #[tokio::test]
    async fn get_me_unknown_user_fails() {
        let repo: Arc<dyn UserRepository> = Arc::new(MockUserRepository::default());
        let svc = UserService::new(repo);
        let err = svc.get_me(Uuid::new_v4()).await.unwrap_err();
        assert!(matches!(err, AuthError::InvalidCredentials));
    }
}
