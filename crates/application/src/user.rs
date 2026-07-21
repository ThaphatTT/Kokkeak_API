use std::sync::Arc;

use kokkak_domain::traits::user::RepoError;
use kokkak_domain::{AuthError, PublicUser, UserListRow, UserRepository};
use uuid::Uuid;

pub struct UserListPage {
    pub items: Vec<UserListRow>,

    pub next_cursor: Option<String>,
}

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

    pub async fn get_user(&self, user_id: Uuid) -> Result<kokkak_domain::User, AuthError> {
        self.users
            .find_by_id(user_id)
            .await
            .map_err(|e| AuthError::Backend(e.to_string()))?
            .ok_or(AuthError::InvalidCredentials)
    }

    pub async fn autocomplete(
        &self,
        input: kokkak_domain::UserAutocompleteInput,
    ) -> Result<kokkak_domain::UserAutocompletePage, RepoError> {
        self.users.autocomplete(&input).await
    }

    pub async fn get_addresses_by_user_guid(
        &self,
        input: kokkak_domain::UserAddressInput,
    ) -> Result<kokkak_domain::UserAddressPage, RepoError> {
        self.users.get_addresses_by_user_guid(&input).await
    }

    pub async fn list_users(
        &self,
        after: Option<String>,
        limit: u32,
        actor: Uuid,
    ) -> Result<UserListPage, RepoError> {
        let rows = self.users.list_with_permissions(actor).await?;
        Ok(apply_cursor_pagination(rows, after.as_deref(), limit))
    }
}

fn apply_cursor_pagination(
    rows: Vec<UserListRow>,
    after: Option<&str>,
    limit: u32,
) -> UserListPage {
    let start = match after {
        Some(cursor) => rows
            .iter()
            .position(|r| r.email.as_str() > cursor)
            .unwrap_or(rows.len()),
        None => 0,
    };
    let end = (start + limit as usize).min(rows.len());
    let items = rows[start..end].to_vec();
    let next_cursor = if end < rows.len() {
        items.last().map(|r| r.email.clone())
    } else {
        None
    };
    UserListPage { items, next_cursor }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct MockUserRepository {
        by_id: std::sync::Mutex<std::collections::HashMap<uuid::Uuid, User>>,
        by_username: std::sync::Mutex<std::collections::HashMap<String, Uuid>>,

        list_rows: std::sync::Mutex<Vec<kokkak_domain::UserListRow>>,
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
        async fn list_with_permissions(
            &self,
            _caller_guid: Uuid,
        ) -> Result<Vec<kokkak_domain::UserListRow>, kokkak_domain::RepoError> {
            Ok(self.list_rows.lock().unwrap().clone())
        }
        async fn find_username_guid_by_user_guid(
            &self,
            _user_guid: Uuid,
        ) -> Result<Option<String>, kokkak_domain::RepoError> {
            Ok(None)
        }
        async fn admin_insert_full(
            &self,
            _req: &kokkak_domain::AdminInsertUserRequest,
        ) -> Result<kokkak_domain::AdminInsertUserResult, kokkak_domain::AdminInsertUserError>
        {
            Err(kokkak_domain::AdminInsertUserError::new(
                "internal",
                "admin_insert_full not implemented in user mock",
            ))
        }
    }

    use chrono::Utc;
    use kokkak_domain::{Role, User, UserStatus};

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
            permissions: Vec::new(),
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

    fn sample_list_row(email: &str) -> kokkak_domain::UserListRow {
        kokkak_domain::UserListRow {
            user_guid: Uuid::new_v4().to_string(),
            full_name: format!("Test {email}"),
            email: email.to_string(),
            role_codes: vec!["customer".into()],
            role_names: vec!["Customer".into()],
            has_permission: true,
            has_override: false,
            user_status: UserStatus::Active,
            user_username_status: 1,
        }
    }

    #[tokio::test]
    async fn list_users_returns_empty_page_when_repo_empty() {
        let repo: Arc<dyn UserRepository> = Arc::new(MockUserRepository::default());
        let svc = UserService::new(repo);
        let actor = Uuid::new_v4();
        let page = svc.list_users(None, 25, actor).await.unwrap();
        assert!(page.items.is_empty());
        assert!(page.next_cursor.is_none());
    }

    #[tokio::test]
    async fn list_users_applies_cursor_pagination() {
        let mock = MockUserRepository {
            list_rows: std::sync::Mutex::new(vec![
                sample_list_row("alice@example.com"),
                sample_list_row("bob@example.com"),
                sample_list_row("carol@example.com"),
            ]),
            ..Default::default()
        };
        let repo: Arc<dyn UserRepository> = Arc::new(mock);
        let svc = UserService::new(repo);
        let actor = Uuid::new_v4();

        let page1 = svc.list_users(None, 2, actor).await.unwrap();
        assert_eq!(page1.items.len(), 2);
        assert_eq!(page1.items[0].email, "alice@example.com");
        assert_eq!(page1.items[1].email, "bob@example.com");
        assert_eq!(page1.next_cursor.as_deref(), Some("bob@example.com"));

        let page2 = svc
            .list_users(page1.next_cursor.clone(), 2, actor)
            .await
            .unwrap();
        assert_eq!(page2.items.len(), 1);
        assert_eq!(page2.items[0].email, "carol@example.com");
        assert!(page2.next_cursor.is_none());
    }
}
