//! Auth use cases (M2 + M14).
//!
//! Encapsulates register / login / refresh. Takes `Arc<dyn UserRepository>`
//! and a `PasswordHasher` + `JwtService` (from `infra`). Stays
//! transport-agnostic — the API layer maps HTTP requests to these
//! inputs.
//!
//! M14 changes (NEW_DB.txt alignment):
//! - `email` field replaced by `username` (NEW_DB `user_username_username`)
//! - `display_name` replaced by `first_name` + `last_name`
//! - `locale` removed from the User aggregate (now lives in JWT /
//!   Accept-Language per M11)
//! - `EmailTaken` → `UsernameTaken`

use std::sync::Arc;

use chrono::Utc;
use kokkak_domain::{
    AuthError, Claims, PublicUser, Role, TokenKind, TokenPair, User, UserRepository, UserStatus,
};
use uuid::Uuid;

/// Register input (สมัครสมาชิกใหม่).
#[derive(Debug, Clone)]
pub struct RegisterInput {
    /// Login username (will be lowercased). In practice this can be
    /// an email, phone, or alphanumeric handle — the API doesn't
    /// enforce a particular shape (NEW_DB stores it as
    /// `user_username_username`).
    pub username: String,
    /// Plain-text password (use case hashes it; never logged).
    pub password: String,
    /// First name (`[user].user_first_name`).
    pub first_name: String,
    /// Last name (`[user].user_last_name`).
    pub last_name: String,
    /// Role to grant. Only customer/technician allowed in self-registration.
    pub role: Role,
}

/// Login input.
#[derive(Debug, Clone)]
pub struct LoginInput {
    pub username: String,
    pub password: String,
    /// Token scope (`"mobile"` / `"web"` / `"admin"`).
    pub scope: String,
}

/// Result of register / login / refresh.
#[derive(Debug, Clone)]
pub struct AuthOutcome {
    pub user: PublicUser,
    pub tokens: TokenPair,
}

/// Auth use case bundle.
pub struct AuthService {
    users: Arc<dyn UserRepository>,
    hasher: Arc<dyn PasswordHasherPort>,
    jwt: Arc<dyn JwtIssuerPort>,
}

impl AuthService {
    pub fn new(
        users: Arc<dyn UserRepository>,
        hasher: Arc<dyn PasswordHasherPort>,
        jwt: Arc<dyn JwtIssuerPort>,
    ) -> Self {
        Self { users, hasher, jwt }
    }

    /// Register a new account.
    pub async fn register(&self, input: RegisterInput) -> Result<AuthOutcome, AuthError> {
        let username = input.username.trim().to_lowercase();
        if username.is_empty() {
            return Err(AuthError::Validation("username must not be empty".into()));
        }
        if input.password.len() < 8 {
            return Err(AuthError::Validation(
                "password must be at least 8 characters".into(),
            ));
        }
        if input.first_name.trim().is_empty() {
            return Err(AuthError::Validation("first_name must not be empty".into()));
        }
        let now = Utc::now();
        let user = User {
            id: Uuid::new_v4(),
            first_name: input.first_name.trim().to_string(),
            last_name: input.last_name.trim().to_string(),
            username,
            password_hash: self.hasher.hash(&input.password)?,
            roles: vec![input.role],
            status: UserStatus::Active,
            created_at: now,
            updated_at: now,
        };
        self.users.insert(&user).await.map_err(|e| match e {
            kokkak_domain::RepoError::Conflict(_) => AuthError::UsernameTaken,
            other => AuthError::Backend(other.to_string()),
        })?;
        let tokens = self.issue_pair(user.id, &user.roles, "mobile")?;
        Ok(AuthOutcome {
            user: PublicUser::from(&user),
            tokens,
        })
    }

    /// Login by username + password.
    pub async fn login(&self, input: LoginInput) -> Result<AuthOutcome, AuthError> {
        let username = input.username.trim().to_lowercase();
        let user = self
            .users
            .find_by_username(&username)
            .await
            .map_err(|e| AuthError::Backend(e.to_string()))?
            .ok_or(AuthError::InvalidCredentials)?;
        if !user.can_authenticate() {
            return Err(AuthError::InvalidCredentials);
        }
        self.hasher.verify(&input.password, &user.password_hash)?;
        let tokens = self.issue_pair(user.id, &user.roles, &input.scope)?;
        Ok(AuthOutcome {
            user: PublicUser::from(&user),
            tokens,
        })
    }

    /// Exchange a refresh token for a new access + refresh pair.
    pub async fn refresh(
        &self,
        refresh_token: &str,
        scope: &str,
    ) -> Result<AuthOutcome, AuthError> {
        let claims = self.jwt.verify(refresh_token)?;
        if claims.kind != TokenKind::Refresh {
            return Err(AuthError::InvalidToken("not a refresh token".into()));
        }
        let user = self
            .users
            .find_by_id(claims.sub)
            .await
            .map_err(|e| AuthError::Backend(e.to_string()))?
            .ok_or(AuthError::InvalidCredentials)?;
        if !user.can_authenticate() {
            return Err(AuthError::InvalidCredentials);
        }
        let tokens = self.issue_pair(user.id, &user.roles, scope)?;
        Ok(AuthOutcome {
            user: PublicUser::from(&user),
            tokens,
        })
    }

    fn issue_pair(
        &self,
        user_id: Uuid,
        roles: &[Role],
        scope: &str,
    ) -> Result<TokenPair, AuthError> {
        let access = self.jwt.issue_access(user_id, roles, scope)?;
        let refresh = self.jwt.issue_refresh(user_id, roles, scope)?;
        Ok(TokenPair {
            access_token: access,
            refresh_token: refresh,
            access_ttl_secs: self.jwt.access_ttl_secs(),
            refresh_ttl_secs: self.jwt.refresh_ttl_secs(),
            token_type: "Bearer",
        })
    }
}

/// Port for password hashing (decouples the use case from `argon2`).
pub trait PasswordHasherPort: Send + Sync {
    fn hash(&self, password: &str) -> Result<String, AuthError>;
    fn verify(&self, password: &str, hash: &str) -> Result<(), AuthError>;
}

/// Port for JWT issuing / verifying.
pub trait JwtIssuerPort: Send + Sync {
    fn issue_access(&self, user_id: Uuid, roles: &[Role], scope: &str)
        -> Result<String, AuthError>;
    fn issue_refresh(
        &self,
        user_id: Uuid,
        roles: &[Role],
        scope: &str,
    ) -> Result<String, AuthError>;
    fn verify(&self, token: &str) -> Result<Claims, AuthError>;
    fn access_ttl_secs(&self) -> i64;
    fn refresh_ttl_secs(&self) -> i64;
}

// ---- Adapters live in the api crate (composition root) ----

#[cfg(test)]
mod tests {
    use super::*;
    use kokkak_infra::auth::jwt::JwtService;
    use kokkak_infra::auth::password::PasswordHasherImpl;
    use kokkak_infra::db::json_user::JsonUserRepository;
    use std::path::PathBuf;

    // Test-only adapter: bridges the concrete `PasswordHasherImpl` to
    // the `PasswordHasherPort` trait without depending on the
    // production adapter in the api crate.
    struct TestHasher(PasswordHasherImpl);
    impl PasswordHasherPort for TestHasher {
        fn hash(&self, password: &str) -> Result<String, AuthError> {
            self.0.hash(password)
        }
        fn verify(&self, password: &str, hash: &str) -> Result<(), AuthError> {
            self.0.verify(password, hash)
        }
    }

    struct TestJwt(JwtService);
    impl JwtIssuerPort for TestJwt {
        fn issue_access(
            &self,
            user_id: Uuid,
            roles: &[Role],
            scope: &str,
        ) -> Result<String, AuthError> {
            self.0.issue_access(user_id, roles, scope)
        }
        fn issue_refresh(
            &self,
            user_id: Uuid,
            roles: &[Role],
            scope: &str,
        ) -> Result<String, AuthError> {
            self.0.issue_refresh(user_id, roles, scope)
        }
        fn verify(&self, token: &str) -> Result<Claims, AuthError> {
            self.0.verify(token)
        }
        fn access_ttl_secs(&self) -> i64 {
            self.0.access_ttl_secs()
        }
        fn refresh_ttl_secs(&self) -> i64 {
            self.0.refresh_ttl_secs()
        }
    }

    async fn make_service() -> (AuthService, PathBuf) {
        let path: PathBuf = std::env::temp_dir()
            .join("kokkak_app_auth_test")
            .join(format!("auth-{}.json", Uuid::new_v4()));
        let _ = std::fs::create_dir_all(path.parent().unwrap());
        let _ = std::fs::remove_file(&path);
        let repo = JsonUserRepository::open(&path).await.unwrap();
        let settings = kokkak_common::config::AuthSettings {
            jwt_secret: "test-secret-please-change-me".into(),
            issuer: "kokkak-test".into(),
            access_ttl_secs: 60,
            refresh_ttl_secs: 600,
        };
        let jwt = JwtService::new(&settings).unwrap();
        let svc = AuthService::new(
            Arc::new(repo),
            Arc::new(TestHasher(PasswordHasherImpl::new())),
            Arc::new(TestJwt(jwt)),
        );
        (svc, path)
    }

    #[tokio::test]
    async fn register_creates_user_and_returns_tokens() {
        let (svc, path) = make_service().await;
        let out = svc
            .register(RegisterInput {
                username: "Alice@Example.com".into(),
                password: "supersecret-123".into(),
                first_name: "Alice".into(),
                last_name: "Wonder".into(),
                role: Role::Customer,
            })
            .await
            .unwrap();
        assert_eq!(out.user.username, "alice@example.com");
        assert_eq!(out.user.first_name, "Alice");
        assert_eq!(out.user.last_name, "Wonder");
        assert!(!out.tokens.access_token.is_empty());
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn register_duplicate_username_returns_username_taken() {
        let (svc, path) = make_service().await;
        svc.register(RegisterInput {
            username: "alice".into(),
            password: "supersecret-123".into(),
            first_name: "Alice".into(),
            last_name: "Wonder".into(),
            role: Role::Customer,
        })
        .await
        .unwrap();
        let err = svc
            .register(RegisterInput {
                username: "alice".into(),
                password: "supersecret-123".into(),
                first_name: "Alice2".into(),
                last_name: "Wonder2".into(),
                role: Role::Customer,
            })
            .await
            .unwrap_err();
        assert!(matches!(err, AuthError::UsernameTaken));
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn login_with_wrong_password_fails() {
        let (svc, path) = make_service().await;
        svc.register(RegisterInput {
            username: "alice".into(),
            password: "supersecret-123".into(),
            first_name: "Alice".into(),
            last_name: "Wonder".into(),
            role: Role::Customer,
        })
        .await
        .unwrap();
        let err = svc
            .login(LoginInput {
                username: "alice".into(),
                password: "wrong-password".into(),
                scope: "mobile".into(),
            })
            .await
            .unwrap_err();
        assert!(matches!(err, AuthError::InvalidCredentials));
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn login_with_correct_password_returns_tokens() {
        let (svc, path) = make_service().await;
        svc.register(RegisterInput {
            username: "alice".into(),
            password: "supersecret-123".into(),
            first_name: "Alice".into(),
            last_name: "Wonder".into(),
            role: Role::Customer,
        })
        .await
        .unwrap();
        let out = svc
            .login(LoginInput {
                username: "alice".into(),
                password: "supersecret-123".into(),
                scope: "mobile".into(),
            })
            .await
            .unwrap();
        assert_eq!(out.user.username, "alice");
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn refresh_exchanges_refresh_token() {
        let (svc, path) = make_service().await;
        let registered = svc
            .register(RegisterInput {
                username: "alice".into(),
                password: "supersecret-123".into(),
                first_name: "Alice".into(),
                last_name: "Wonder".into(),
                role: Role::Customer,
            })
            .await
            .unwrap();
        let out = svc
            .refresh(&registered.tokens.refresh_token, "mobile")
            .await
            .unwrap();
        // Returns a valid pair; tokens may equal the previous ones
        // when issued in the same second (JWT `iat` granularity).
        // Production adds a `jti` claim for true rotation.
        assert!(!out.tokens.access_token.is_empty());
        assert!(!out.tokens.refresh_token.is_empty());
        assert_eq!(out.user.username, "alice");
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn refresh_with_access_token_fails() {
        let (svc, path) = make_service().await;
        let registered = svc
            .register(RegisterInput {
                username: "alice".into(),
                password: "supersecret-123".into(),
                first_name: "Alice".into(),
                last_name: "Wonder".into(),
                role: Role::Customer,
            })
            .await
            .unwrap();
        let err = svc
            .refresh(&registered.tokens.access_token, "mobile")
            .await
            .unwrap_err();
        assert!(matches!(err, AuthError::InvalidToken(_)));
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn register_rejects_short_password() {
        let (svc, path) = make_service().await;
        let err = svc
            .register(RegisterInput {
                username: "alice".into(),
                password: "short".into(),
                first_name: "Alice".into(),
                last_name: "Wonder".into(),
                role: Role::Customer,
            })
            .await
            .unwrap_err();
        assert!(matches!(err, AuthError::Validation(_)));
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn register_rejects_empty_first_name() {
        let (svc, path) = make_service().await;
        let err = svc
            .register(RegisterInput {
                username: "alice".into(),
                password: "supersecret-123".into(),
                first_name: "  ".into(),
                last_name: "Wonder".into(),
                role: Role::Customer,
            })
            .await
            .unwrap_err();
        assert!(matches!(err, AuthError::Validation(_)));
        let _ = std::fs::remove_file(&path);
    }
}
