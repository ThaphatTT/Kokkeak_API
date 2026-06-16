//! User use cases (M2).
//!
//! - `get_me`: fetch the public view of the current user.

use std::sync::Arc;

use kokkak_domain::{AuthError, PublicUser, UserRepository};
use uuid::Uuid;

pub struct UserService {
    users: Arc<dyn UserRepository>,
}

impl UserService {
    pub fn new(users: Arc<dyn UserRepository>) -> Self {
        Self { users }
    }

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
    /// that need the locale / status / role list).
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
    use chrono::Utc;
    use kokkak_domain::{Role, User, UserStatus};
    use kokkak_infra::db::json_user::JsonUserRepository;
    use std::path::PathBuf;

    #[tokio::test]
    async fn get_me_returns_public_view() {
        let path: PathBuf = std::env::temp_dir()
            .join("kokkak_user_service_test")
            .join(format!("u-{}.json", Uuid::new_v4()));
        let _ = std::fs::create_dir_all(path.parent().unwrap());
        let _ = std::fs::remove_file(&path);
        let repo = JsonUserRepository::open(&path).await.unwrap();
        let id = Uuid::new_v4();
        let u = User {
            id,
            email: "a@b.com".into(),
            display_name: "A".into(),
            password_hash: "$argon2".into(),
            roles: vec![Role::Customer],
            status: UserStatus::Active,
            locale: "lo".into(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        repo.insert(&u).await.unwrap();
        let svc = UserService::new(Arc::new(repo));
        let me = svc.get_me(id).await.unwrap();
        assert_eq!(me.email, "a@b.com");
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn get_me_unknown_user_fails() {
        let path: PathBuf = std::env::temp_dir()
            .join("kokkak_user_service_test")
            .join(format!("u-{}.json", Uuid::new_v4()));
        let _ = std::fs::create_dir_all(path.parent().unwrap());
        let _ = std::fs::remove_file(&path);
        let repo = JsonUserRepository::open(&path).await.unwrap();
        let svc = UserService::new(Arc::new(repo));
        let err = svc.get_me(Uuid::new_v4()).await.unwrap_err();
        assert!(matches!(err, AuthError::InvalidCredentials));
        let _ = std::fs::remove_file(&path);
    }
}
