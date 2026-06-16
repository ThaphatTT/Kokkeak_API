//! Auth use cases (M2).
//!
//! Encapsulates register / login / refresh. Takes `Arc<dyn UserRepository>`
//! and a `PasswordHasher` + `JwtService` (from `infra`). Stays
//! transport-agnostic — the API layer maps HTTP requests to these
//! inputs.

use std::sync::Arc;

use chrono::Utc;
use kokkak_domain::{
    AuthError, Claims, PublicUser, Role, TokenKind, TokenPair, User, UserRepository, UserStatus,
};
use uuid::Uuid;

/// Register input (สมัครสมาชิกใหม่).
#[derive(Debug, Clone)]
pub struct RegisterInput {
    /// Email (will be lowercased).
    pub email: String,
    /// Plain-text password (use case hashes it; never logged).
    pub password: String,
    /// Display name.
    pub display_name: String,
    /// Role to grant. Only customer/technician allowed in self-registration.
    pub role: Role,
    /// Locale preference (`"th"` / `"en"` / `"lo"`).
    pub locale: String,
}

/// Login input.
#[derive(Debug, Clone)]
pub struct LoginInput {
    pub email: String,
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
        let email = input.email.trim().to_lowercase();
        if email.is_empty() || !email.contains('@') {
            return Err(AuthError::Validation("invalid email".into()));
        }
        if input.password.len() < 8 {
            return Err(AuthError::Validation(
                "password must be at least 8 characters".into(),
            ));
        }
        let now = Utc::now();
        let user = User {
            id: Uuid::new_v4(),
            email,
            display_name: input.display_name.trim().to_string(),
            password_hash: self.hasher.hash(&input.password)?,
            roles: vec![input.role],
            status: UserStatus::Active,
            locale: input.locale,
            created_at: now,
            updated_at: now,
        };
        self.users.insert(&user).await.map_err(|e| match e {
            kokkak_domain::RepoError::Conflict(_) => AuthError::EmailTaken,
            other => AuthError::Backend(other.to_string()),
        })?;
        let tokens = self.issue_pair(user.id, &user.roles, "mobile")?;
        Ok(AuthOutcome {
            user: PublicUser::from(&user),
            tokens,
        })
    }

    /// Login by email + password.
    pub async fn login(&self, input: LoginInput) -> Result<AuthOutcome, AuthError> {
        let email = input.email.trim().to_lowercase();
        let user = self
            .users
            .find_by_email(&email)
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
                email: "Alice@Example.com".into(),
                password: "supersecret-123".into(),
                display_name: "Alice".into(),
                role: Role::Customer,
                locale: "lo".into(),
            })
            .await
            .unwrap();
        assert_eq!(out.user.email, "alice@example.com");
        assert!(!out.tokens.access_token.is_empty());
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn register_duplicate_email_returns_email_taken() {
        let (svc, path) = make_service().await;
        svc.register(RegisterInput {
            email: "a@b.com".into(),
            password: "supersecret-123".into(),
            display_name: "A".into(),
            role: Role::Customer,
            locale: "lo".into(),
        })
        .await
        .unwrap();
        let err = svc
            .register(RegisterInput {
                email: "a@b.com".into(),
                password: "supersecret-123".into(),
                display_name: "A2".into(),
                role: Role::Customer,
                locale: "lo".into(),
            })
            .await
            .unwrap_err();
        assert!(matches!(err, AuthError::EmailTaken));
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn login_with_wrong_password_fails() {
        let (svc, path) = make_service().await;
        svc.register(RegisterInput {
            email: "a@b.com".into(),
            password: "supersecret-123".into(),
            display_name: "A".into(),
            role: Role::Customer,
            locale: "lo".into(),
        })
        .await
        .unwrap();
        let err = svc
            .login(LoginInput {
                email: "a@b.com".into(),
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
            email: "a@b.com".into(),
            password: "supersecret-123".into(),
            display_name: "A".into(),
            role: Role::Customer,
            locale: "lo".into(),
        })
        .await
        .unwrap();
        let out = svc
            .login(LoginInput {
                email: "a@b.com".into(),
                password: "supersecret-123".into(),
                scope: "mobile".into(),
            })
            .await
            .unwrap();
        assert_eq!(out.user.email, "a@b.com");
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn refresh_exchanges_refresh_token() {
        let (svc, path) = make_service().await;
        let registered = svc
            .register(RegisterInput {
                email: "a@b.com".into(),
                password: "supersecret-123".into(),
                display_name: "A".into(),
                role: Role::Customer,
                locale: "lo".into(),
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
        assert_eq!(out.user.email, "a@b.com");
        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn refresh_with_access_token_fails() {
        let (svc, path) = make_service().await;
        let registered = svc
            .register(RegisterInput {
                email: "a@b.com".into(),
                password: "supersecret-123".into(),
                display_name: "A".into(),
                role: Role::Customer,
                locale: "lo".into(),
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
}
