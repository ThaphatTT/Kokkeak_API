use std::sync::Arc;

use chrono::Utc;
use kokkak_domain::{
    AuthError, Claims, CreateSession, PublicUser, Role, SessionInfo, SessionStore, TokenKind,
    TokenPair, User, UserRepository, UserStatus,
};
use uuid::Uuid;

use crate::audit::AuditEvent;
use crate::rate_limit::RateLimitDecision;

#[derive(Debug, Clone)]
pub struct RegisterInput {
    pub username: String,

    pub password: String,

    pub first_name: String,

    pub last_name: String,

    pub role: Role,
}

#[derive(Debug, Clone)]
pub struct LoginInput {
    pub username: String,

    pub password: String,

    pub scope: String,

    pub ip: Option<std::net::IpAddr>,
}

#[derive(Debug, Clone)]
pub struct AuthOutcome {
    pub user: PublicUser,

    pub tokens: TokenPair,
}

pub struct AuthService {
    users: Arc<dyn UserRepository>,
    hasher: Arc<dyn PasswordHasherPort>,
    jwt: Arc<dyn JwtIssuerPort>,
    sessions: Arc<dyn SessionStore>,
    audit: Arc<dyn crate::audit::AuditLogger>,
    login_rl: Arc<dyn crate::rate_limit::LoginRateLimiter>,
}

impl AuthService {
    pub fn new(
        users: Arc<dyn UserRepository>,
        hasher: Arc<dyn PasswordHasherPort>,
        jwt: Arc<dyn JwtIssuerPort>,
        sessions: Arc<dyn SessionStore>,
        audit: Arc<dyn crate::audit::AuditLogger>,
        login_rl: Arc<dyn crate::rate_limit::LoginRateLimiter>,
    ) -> Self {
        Self {
            users,
            hasher,
            jwt,
            sessions,
            audit,
            login_rl,
        }
    }

    pub async fn register(&self, input: RegisterInput) -> Result<AuthOutcome, AuthError> {
        let username = input.username.trim().to_lowercase();
        if username.is_empty() {
            self.audit.log(
                AuditEvent::new("auth.register.failure")
                    .with_username(&username)
                    .with_reason("validation"),
            );
            return Err(AuthError::Validation("username must not be empty".into()));
        }
        if input.password.len() < 8 {
            self.audit.log(
                AuditEvent::new("auth.register.failure")
                    .with_username(&username)
                    .with_reason("validation"),
            );
            return Err(AuthError::Validation(
                "password must be at least 8 characters".into(),
            ));
        }
        if input.first_name.trim().is_empty() {
            self.audit.log(
                AuditEvent::new("auth.register.failure")
                    .with_username(&username)
                    .with_reason("validation"),
            );
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

            permissions: Vec::new(),
            status: UserStatus::Active,
            created_at: now,
            updated_at: now,
        };
        match self.users.insert(&user).await {
            Ok(()) => {}
            Err(kokkak_domain::RepoError::Conflict(_)) => {
                self.audit.log(
                    AuditEvent::new("auth.register.failure")
                        .with_username(&user.username)
                        .with_reason("username_taken"),
                );
                return Err(AuthError::UsernameTaken);
            }
            Err(other) => {
                self.audit.log(
                    AuditEvent::new("auth.register.failure")
                        .with_username(&user.username)
                        .with_reason("backend_error"),
                );
                return Err(AuthError::Backend(other.to_string()));
            }
        }
        let (tokens, refresh_jti) = self.issue_pair(user.id, &user.roles, "mobile")?;
        let _ = self
            .sessions
            .create(&CreateSession {
                jti: refresh_jti,
                user_id: user.id,
                scope: "mobile".into(),
                device: "register".into(),
                ip: String::new(),
                ttl_secs: self.jwt.refresh_ttl_secs(),
            })
            .await;
        self.audit.log(
            AuditEvent::new("auth.register.success")
                .with_username(&user.username)
                .with_user_id(user.id)
                .with_context("role", format!("{:?}", user.roles)),
        );
        Ok(AuthOutcome {
            user: PublicUser::from(&user),
            tokens,
        })
    }

    pub async fn login(&self, input: LoginInput) -> Result<AuthOutcome, AuthError> {
        let username = input.username.trim().to_lowercase();
        let ip = input.ip;

        if let Some(ip_addr) = ip {
            let decision = self.login_rl.check(&username, ip_addr);
            if let RateLimitDecision::Locked { retry_after } = decision {
                let secs = retry_after.as_secs().max(1);
                self.audit.log(
                    AuditEvent::new("auth.login.rate_limited")
                        .with_username(&username)
                        .with_ip(ip_addr)
                        .with_reason("rate_limited")
                        .with_context("retry_after_secs", secs.to_string()),
                );
                tracing::warn!(
                    event = "auth.login.rate_limited",
                    username = %username,
                    ip = %ip_addr,
                    retry_after_secs = secs,
                    "login blocked by per-(username, IP) rate limiter",
                );
                return Err(AuthError::RateLimited(secs));
            }
        }

        let user = self
            .users
            .find_by_username(&username)
            .await
            .map_err(|e| AuthError::Backend(e.to_string()))?;

        let hash_to_check = user
            .as_ref()
            .map(|u| u.password_hash.as_str())
            .unwrap_or_else(|| self.hasher.dummy_hash());

        let verified = self.hasher.verify(&input.password, hash_to_check).is_ok();

        let failure_reason: Option<LoginFailureReason> = match (user.as_ref(), verified) {
            (Some(_), false) => Some(LoginFailureReason::WrongPassword),
            (None, _) => Some(LoginFailureReason::UserNotFound),
            _ => None,
        };

        if let Some(reason) = failure_reason {
            log_login_failure(&username, reason);
            self.audit.log(build_login_failure_event(
                "auth.login.failure",
                &username,
                ip,
                reason.as_str(),
            ));
            if let Some(ip_addr) = ip {
                self.login_rl.record_failure(&username, ip_addr);
            }
            return Err(AuthError::InvalidCredentials);
        }

        let user = user.expect("user must be Some when failure_reason is None");

        if !user.can_authenticate() {
            let reason = LoginFailureReason::from_status(user.status);
            log_login_failure(&username, reason);
            self.audit.log(build_login_failure_event(
                "auth.login.failure",
                &username,
                ip,
                reason.as_str(),
            ));
            if let Some(ip_addr) = ip {
                self.login_rl.record_failure(&username, ip_addr);
            }
            return Err(AuthError::InvalidCredentials);
        }

        if let Some(ip_addr) = ip {
            self.login_rl.reset(&username, ip_addr);
        }

        let (tokens, refresh_jti) = self.issue_pair(user.id, &user.roles, &input.scope)?;
        let _ = self
            .sessions
            .create(&CreateSession {
                jti: refresh_jti,
                user_id: user.id,
                scope: input.scope.clone(),
                device: "login".into(),
                ip: ip.map(|i| i.to_string()).unwrap_or_default(),
                ttl_secs: self.jwt.refresh_ttl_secs(),
            })
            .await;
        tracing::debug!(
            event = "auth.login.success",
            user_id = %user.id,
            "login succeeded",
        );
        self.audit.log(
            AuditEvent::new("auth.login.success")
                .with_username(&username)
                .with_user_id(user.id)
                .with_ip_opt(ip)
                .with_context("scope", input.scope.clone()),
        );
        Ok(AuthOutcome {
            user: PublicUser::from(&user),
            tokens,
        })
    }

    pub async fn refresh(&self, refresh_token: &str) -> Result<AuthOutcome, AuthError> {
        let claims = self.jwt.verify(refresh_token)?;
        if claims.kind != TokenKind::Refresh {
            self.audit.log(
                AuditEvent::new("auth.refresh.failure")
                    .with_reason("invalid_token")
                    .with_context("detail", "not a refresh token"),
            );
            return Err(AuthError::InvalidToken("not a refresh token".into()));
        }
        let scope = claims.scope.clone();
        let old_jti = claims.jti.clone();
        let existing = self.sessions.get(claims.sub, &old_jti).await?;
        if existing.is_none() {
            self.audit.log(
                AuditEvent::new("auth.refresh.failure")
                    .with_reason("revoked")
                    .with_context("detail", "session not found in store"),
            );
            return Err(AuthError::InvalidToken("session revoked".into()));
        }
        let user = self
            .users
            .find_by_id(claims.sub)
            .await
            .map_err(|e| AuthError::Backend(e.to_string()))?
            .ok_or(AuthError::InvalidCredentials)?;
        if !user.can_authenticate() {
            let reason = LoginFailureReason::from_status(user.status);
            log_refresh_failure(user.id, reason);
            self.audit.log(
                AuditEvent::new("auth.refresh.failure")
                    .with_username(&user.username)
                    .with_user_id(user.id)
                    .with_reason(reason.as_str()),
            );
            return Err(AuthError::InvalidCredentials);
        }
        let _ = self.sessions.revoke(claims.sub, &old_jti).await;
        let (tokens, refresh_jti) = self.issue_pair(user.id, &user.roles, &scope)?;
        let _ = self
            .sessions
            .create(&CreateSession {
                jti: refresh_jti,
                user_id: user.id,
                scope: scope.clone(),
                device: existing
                    .as_ref()
                    .map(|s| s.device.clone())
                    .unwrap_or_default(),
                ip: existing.as_ref().map(|s| s.ip.clone()).unwrap_or_default(),
                ttl_secs: self.jwt.refresh_ttl_secs(),
            })
            .await;
        tracing::debug!(
            event = "auth.refresh.success",
            user_id = %user.id,
            "refresh succeeded",
        );
        self.audit.log(
            AuditEvent::new("auth.refresh.success")
                .with_username(&user.username)
                .with_user_id(user.id)
                .with_context("scope", scope.clone()),
        );
        Ok(AuthOutcome {
            user: PublicUser::from(&user),
            tokens,
        })
    }

    pub async fn logout(&self, user_id: Uuid, jti: &str) -> Result<(), AuthError> {
        self.sessions.revoke(user_id, jti).await
    }

    pub async fn revoke_all(&self, user_id: Uuid) -> Result<u64, AuthError> {
        self.sessions.revoke_all(user_id).await
    }

    pub async fn list_sessions(&self, user_id: Uuid) -> Result<Vec<SessionInfo>, AuthError> {
        self.sessions.list(user_id).await
    }

    fn issue_pair(
        &self,
        user_id: Uuid,
        roles: &[Role],
        scope: &str,
    ) -> Result<(TokenPair, String), AuthError> {
        let (access, _access_jti) = self.jwt.issue_access(user_id, roles, scope)?;
        let (refresh, refresh_jti) = self.jwt.issue_refresh(user_id, roles, scope)?;
        Ok((
            TokenPair {
                access_token: access,
                refresh_token: refresh,
                access_ttl_secs: self.jwt.access_ttl_secs(),
                refresh_ttl_secs: self.jwt.refresh_ttl_secs(),
                token_type: "Bearer",
            },
            refresh_jti,
        ))
    }
}

#[derive(Debug, Clone, Copy)]
enum LoginFailureReason {
    UserNotFound,

    WrongPassword,

    AccountSuspended,

    AccountDeleted,

    AccountPending,
}

impl LoginFailureReason {
    fn as_str(&self) -> &'static str {
        match self {
            Self::UserNotFound => "user_not_found",
            Self::WrongPassword => "wrong_password",
            Self::AccountSuspended => "account_suspended",
            Self::AccountDeleted => "account_deleted",
            Self::AccountPending => "account_pending",
        }
    }

    fn from_status(status: UserStatus) -> Self {
        match status {
            UserStatus::Suspended => Self::AccountSuspended,
            UserStatus::Deleted => Self::AccountDeleted,
            UserStatus::Pending => Self::AccountPending,

            UserStatus::Active => Self::WrongPassword,
        }
    }
}

fn log_login_failure(username: &str, reason: LoginFailureReason) {
    tracing::warn!(
        event = "auth.login.failure",
        username = %username,
        reason = reason.as_str(),
        "login failed",
    );
}

fn log_refresh_failure(user_id: Uuid, reason: LoginFailureReason) {
    tracing::warn!(
        event = "auth.refresh.failure",
        user_id = %user_id,
        reason = reason.as_str(),
        "refresh failed: account cannot authenticate",
    );
}

fn build_login_failure_event(
    event: &'static str,
    username: &str,
    ip: Option<std::net::IpAddr>,
    reason: &'static str,
) -> AuditEvent {
    let mut e = AuditEvent::new(event)
        .with_username(username)
        .with_reason(reason);
    if let Some(addr) = ip {
        e = e.with_ip(addr);
    }
    e
}

pub trait PasswordHasherPort: Send + Sync {
    fn hash(&self, password: &str) -> Result<String, AuthError>;

    fn verify(&self, password: &str, hash: &str) -> Result<(), AuthError>;

    fn dummy_hash(&self) -> &str;
}

pub trait JwtIssuerPort: Send + Sync {
    fn issue_access(
        &self,
        user_id: Uuid,
        roles: &[Role],
        scope: &str,
    ) -> Result<(String, String), AuthError>;

    fn issue_refresh(
        &self,
        user_id: Uuid,
        roles: &[Role],
        scope: &str,
    ) -> Result<(String, String), AuthError>;

    fn verify(&self, token: &str) -> Result<Claims, AuthError>;

    fn access_ttl_secs(&self) -> i64;

    fn refresh_ttl_secs(&self) -> i64;
}

#[cfg(test)]
mod tests {
    use super::*;

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

        async fn list_with_permissions(
            &self,
            _caller_guid: Uuid,
        ) -> Result<Vec<kokkak_domain::UserListRow>, kokkak_domain::RepoError> {
            Ok(Vec::new())
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
                "admin_insert_full not implemented in auth mock",
            ))
        }
    }

    use kokkak_infra::auth::jwt::JwtService;
    use kokkak_infra::auth::password::PasswordHasherImpl;

    struct TestHasher(PasswordHasherImpl);
    impl PasswordHasherPort for TestHasher {
        fn hash(&self, password: &str) -> Result<String, AuthError> {
            self.0.hash(password)
        }
        fn verify(&self, password: &str, hash: &str) -> Result<(), AuthError> {
            self.0.verify(password, hash)
        }
        fn dummy_hash(&self) -> &str {
            self.0.dummy_hash()
        }
    }

    struct TestJwt(JwtService);
    impl JwtIssuerPort for TestJwt {
        fn issue_access(
            &self,
            user_id: Uuid,
            roles: &[Role],
            scope: &str,
        ) -> Result<(String, String), AuthError> {
            self.0.issue_access(user_id, roles, scope)
        }
        fn issue_refresh(
            &self,
            user_id: Uuid,
            roles: &[Role],
            scope: &str,
        ) -> Result<(String, String), AuthError> {
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

    async fn make_service() -> AuthService {
        make_service_with_repo().await.0
    }

    async fn make_service_with_repo() -> (AuthService, Arc<MockUserRepository>) {
        let repo = Arc::new(MockUserRepository::default());
        let settings = kokkak_common::config::AuthSettings {
            jwt_secret: "test-secret-please-change-me".into(),
            issuer: "kokkak-test".into(),
            access_ttl_secs: 60,
            refresh_ttl_secs: 600,
        };
        let jwt = JwtService::new(&settings).unwrap();
        let svc = AuthService::new(
            repo.clone() as Arc<dyn UserRepository>,
            Arc::new(TestHasher(PasswordHasherImpl::new())),
            Arc::new(TestJwt(jwt)),
            Arc::new(kokkak_domain::NoopSessionStore),
            Arc::new(crate::audit::TestAuditLogger::default()),
            Arc::new(crate::rate_limit::AllowAllLoginRateLimiter),
        );
        (svc, repo)
    }

    #[tokio::test]
    async fn register_creates_user_and_returns_tokens() {
        let svc = make_service().await;
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
    }

    #[tokio::test]
    async fn register_duplicate_username_returns_username_taken() {
        let svc = make_service().await;
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
    }

    #[tokio::test]
    async fn login_with_wrong_password_fails() {
        let svc = make_service().await;
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
                ip: None,
            })
            .await
            .unwrap_err();
        assert!(matches!(err, AuthError::InvalidCredentials));
    }

    #[tokio::test]
    async fn login_with_correct_password_returns_tokens() {
        let svc = make_service().await;
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
                ip: None,
            })
            .await
            .unwrap();
        assert_eq!(out.user.username, "alice");
    }

    #[tokio::test]
    async fn refresh_exchanges_refresh_token() {
        let svc = make_service().await;
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
        let out = svc.refresh(&registered.tokens.refresh_token).await.unwrap();

        assert!(!out.tokens.access_token.is_empty());
        assert!(!out.tokens.refresh_token.is_empty());
        assert_eq!(out.user.username, "alice");
    }

    #[tokio::test]
    async fn refresh_with_access_token_fails() {
        let svc = make_service().await;
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
            .refresh(&registered.tokens.access_token)
            .await
            .unwrap_err();
        assert!(matches!(err, AuthError::InvalidToken(_)));
    }

    #[tokio::test]
    async fn register_rejects_short_password() {
        let svc = make_service().await;
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
    }

    #[tokio::test]
    async fn register_rejects_empty_first_name() {
        let svc = make_service().await;
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
    }

    #[tokio::test]
    async fn login_unknown_user_returns_generic_invalid_credentials() {
        let svc = make_service().await;
        let err = svc
            .login(LoginInput {
                username: "ghost-user-does-not-exist".into(),
                password: "any-password".into(),
                scope: "mobile".into(),
                ip: None,
            })
            .await
            .unwrap_err();
        assert!(
            matches!(err, AuthError::InvalidCredentials),
            "user-not-found must collapse into the same error as wrong-password"
        );
    }

    #[tokio::test]
    async fn login_suspended_user_returns_generic_invalid_credentials() {
        let (svc, repo) = make_service_with_repo().await;
        svc.register(RegisterInput {
            username: "alice".into(),
            password: "supersecret-123".into(),
            first_name: "Alice".into(),
            last_name: "Wonder".into(),
            role: Role::Customer,
        })
        .await
        .unwrap();
        repo.update_status("alice", UserStatus::Suspended);

        let err = svc
            .login(LoginInput {
                username: "alice".into(),
                password: "supersecret-123".into(),
                scope: "mobile".into(),
                ip: None,
            })
            .await
            .unwrap_err();
        assert!(matches!(err, AuthError::InvalidCredentials));
    }

    #[tokio::test]
    async fn login_deleted_user_returns_generic_invalid_credentials() {
        let (svc, repo) = make_service_with_repo().await;
        svc.register(RegisterInput {
            username: "alice".into(),
            password: "supersecret-123".into(),
            first_name: "Alice".into(),
            last_name: "Wonder".into(),
            role: Role::Customer,
        })
        .await
        .unwrap();
        repo.update_status("alice", UserStatus::Deleted);

        let err = svc
            .login(LoginInput {
                username: "alice".into(),
                password: "supersecret-123".into(),
                scope: "mobile".into(),
                ip: None,
            })
            .await
            .unwrap_err();
        assert!(matches!(err, AuthError::InvalidCredentials));
    }

    #[tokio::test]
    async fn login_pending_user_returns_generic_invalid_credentials() {
        let (svc, repo) = make_service_with_repo().await;
        svc.register(RegisterInput {
            username: "alice".into(),
            password: "supersecret-123".into(),
            first_name: "Alice".into(),
            last_name: "Wonder".into(),
            role: Role::Customer,
        })
        .await
        .unwrap();
        repo.update_status("alice", UserStatus::Pending);

        let err = svc
            .login(LoginInput {
                username: "alice".into(),
                password: "supersecret-123".into(),
                scope: "mobile".into(),
                ip: None,
            })
            .await
            .unwrap_err();
        assert!(matches!(err, AuthError::InvalidCredentials));
    }

    #[tokio::test]
    async fn login_always_calls_verify_for_constant_time() {
        let verify_count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let count_for_hasher = verify_count.clone();
        let hasher: Arc<dyn PasswordHasherPort> = Arc::new(CountingHasher {
            inner: PasswordHasherImpl::new(),
            count: count_for_hasher,
        });
        let jwt = TestJwt(
            JwtService::new(&kokkak_common::config::AuthSettings {
                jwt_secret: "test-secret-please-change-me".into(),
                issuer: "kokkak-test".into(),
                access_ttl_secs: 60,
                refresh_ttl_secs: 600,
            })
            .unwrap(),
        );
        let repo: Arc<dyn UserRepository> = Arc::new(MockUserRepository::default());
        let svc = AuthService::new(
            repo,
            hasher,
            Arc::new(jwt),
            Arc::new(crate::audit::TestAuditLogger::default()),
            Arc::new(crate::rate_limit::AllowAllLoginRateLimiter),
        );

        let _ = svc
            .login(LoginInput {
                username: "ghost".into(),
                password: "anything".into(),
                scope: "mobile".into(),
                ip: None,
            })
            .await;
        assert!(
            verify_count.load(std::sync::atomic::Ordering::SeqCst) >= 1,
            "verify must be called even when the user is not found — \
                     this is the constant-time login guard"
        );
    }

    #[tokio::test]
    async fn login_success_emits_auth_login_success_audit_event() {
        let audit = Arc::new(crate::audit::TestAuditLogger::default());
        let repo: Arc<dyn UserRepository> = Arc::new(MockUserRepository::default());
        let settings = kokkak_common::config::AuthSettings {
            jwt_secret: "test-secret-please-change-me".into(),
            issuer: "kokkak-test".into(),
            access_ttl_secs: 60,
            refresh_ttl_secs: 600,
        };
        let jwt = JwtService::new(&settings).unwrap();
        let svc = AuthService::new(
            repo,
            Arc::new(TestHasher(PasswordHasherImpl::new())),
            Arc::new(TestJwt(jwt)),
            audit.clone(),
            Arc::new(crate::rate_limit::AllowAllLoginRateLimiter),
        );
        svc.register(RegisterInput {
            username: "alice".into(),
            password: "supersecret-123".into(),
            first_name: "Alice".into(),
            last_name: "Wonder".into(),
            role: Role::Customer,
        })
        .await
        .unwrap();
        let ip = "203.0.113.5".parse::<std::net::IpAddr>().unwrap();
        svc.login(LoginInput {
            username: "alice".into(),
            password: "supersecret-123".into(),
            scope: "mobile".into(),
            ip: Some(ip),
        })
        .await
        .unwrap();
        let events = audit.events.lock().unwrap();
        let success = events
            .iter()
            .find(|e| e.event == "auth.login.success")
            .expect("audit must record auth.login.success");
        assert_eq!(success.username.as_deref(), Some("alice"));
        assert_eq!(success.ip, Some(ip));
        assert_eq!(
            success.context.get("scope").map(String::as_str),
            Some("mobile")
        );
    }

    #[tokio::test]
    async fn login_failure_emits_auth_login_failure_with_specific_reason() {
        let audit = Arc::new(crate::audit::TestAuditLogger::default());
        let repo: Arc<dyn UserRepository> = Arc::new(MockUserRepository::default());
        let settings = kokkak_common::config::AuthSettings {
            jwt_secret: "test-secret-please-change-me".into(),
            issuer: "kokkak-test".into(),
            access_ttl_secs: 60,
            refresh_ttl_secs: 600,
        };
        let jwt = JwtService::new(&settings).unwrap();
        let svc = AuthService::new(
            repo,
            Arc::new(TestHasher(PasswordHasherImpl::new())),
            Arc::new(TestJwt(jwt)),
            audit.clone(),
            Arc::new(crate::rate_limit::AllowAllLoginRateLimiter),
        );
        let _ = svc
            .login(LoginInput {
                username: "ghost".into(),
                password: "anything".into(),
                scope: "mobile".into(),
                ip: None,
            })
            .await;
        let events = audit.events.lock().unwrap();
        let fail = events
            .iter()
            .find(|e| e.event == "auth.login.failure")
            .expect("audit must record auth.login.failure");
        assert_eq!(fail.reason, Some("user_not_found"));
        assert_eq!(fail.username.as_deref(), Some("ghost"));
    }

    #[tokio::test]
    async fn rate_limited_login_returns_rate_limited_error_and_emits_audit() {
        let audit = Arc::new(crate::audit::TestAuditLogger::default());
        let repo: Arc<dyn UserRepository> = Arc::new(MockUserRepository::default());
        let settings = kokkak_common::config::AuthSettings {
            jwt_secret: "test-secret-please-change-me".into(),
            issuer: "kokkak-test".into(),
            access_ttl_secs: 60,
            refresh_ttl_secs: 600,
        };
        let jwt = JwtService::new(&settings).unwrap();
        let svc = AuthService::new(
            repo,
            Arc::new(TestHasher(PasswordHasherImpl::new())),
            Arc::new(TestJwt(jwt)),
            audit.clone(),
            Arc::new(crate::rate_limit::AlwaysLockedRateLimiter::new()),
        );

        let ip = "203.0.113.99".parse::<std::net::IpAddr>().unwrap();
        let err = svc
            .login(LoginInput {
                username: "alice".into(),
                password: "anything".into(),
                scope: "mobile".into(),
                ip: Some(ip),
            })
            .await
            .unwrap_err();

        match err {
            AuthError::RateLimited(secs) => assert!(secs >= 1, "retry_after must be ≥ 1s"),
            other => panic!("expected RateLimited, got {other:?}"),
        }

        let events = audit.events.lock().unwrap();
        let evt = events
            .iter()
            .find(|e| e.event == "auth.login.rate_limited")
            .expect("audit must record auth.login.rate_limited");
        assert_eq!(evt.reason, Some("rate_limited"));
        assert_eq!(evt.username.as_deref(), Some("alice"));
        assert_eq!(evt.ip, Some(ip));
        assert!(evt.context.contains_key("retry_after_secs"));
    }

    #[tokio::test]
    async fn login_without_ip_skips_rate_limit_check() {
        let audit = Arc::new(crate::audit::TestAuditLogger::default());
        let repo: Arc<dyn UserRepository> = Arc::new(MockUserRepository::default());
        let settings = kokkak_common::config::AuthSettings {
            jwt_secret: "test-secret-please-change-me".into(),
            issuer: "kokkak-test".into(),
            access_ttl_secs: 60,
            refresh_ttl_secs: 600,
        };
        let jwt = JwtService::new(&settings).unwrap();
        let svc = AuthService::new(
            repo,
            Arc::new(TestHasher(PasswordHasherImpl::new())),
            Arc::new(TestJwt(jwt)),
            audit.clone(),
            Arc::new(crate::rate_limit::AlwaysLockedRateLimiter::new()),
        );
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
                ip: None,
            })
            .await;
        assert!(out.is_ok(), "ip=None should bypass the limiter");
    }

    #[tokio::test]
    async fn successful_login_resets_rate_limit_counter() {
        let audit = Arc::new(crate::audit::TestAuditLogger::default());
        let repo: Arc<dyn UserRepository> = Arc::new(MockUserRepository::default());
        let settings = kokkak_common::config::AuthSettings {
            jwt_secret: "test-secret-please-change-me".into(),
            issuer: "kokkak-test".into(),
            access_ttl_secs: 60,
            refresh_ttl_secs: 600,
        };
        let jwt = JwtService::new(&settings).unwrap();
        let reset_calls = Arc::new(std::sync::Mutex::new(
            Vec::<(String, std::net::IpAddr)>::new(),
        ));
        let rc = reset_calls.clone();
        let limiter = CountingRateLimiter {
            reset_log: rc,
            ..CountingRateLimiter::allow_all()
        };
        let svc = AuthService::new(
            repo,
            Arc::new(TestHasher(PasswordHasherImpl::new())),
            Arc::new(TestJwt(jwt)),
            audit.clone(),
            Arc::new(limiter),
        );
        svc.register(RegisterInput {
            username: "alice".into(),
            password: "supersecret-123".into(),
            first_name: "Alice".into(),
            last_name: "Wonder".into(),
            role: Role::Customer,
        })
        .await
        .unwrap();
        let ip = "203.0.113.42".parse::<std::net::IpAddr>().unwrap();
        svc.login(LoginInput {
            username: "alice".into(),
            password: "supersecret-123".into(),
            scope: "mobile".into(),
            ip: Some(ip),
        })
        .await
        .unwrap();
        let resets = reset_calls.lock().unwrap();
        assert_eq!(
            resets.len(),
            1,
            "reset must be called exactly once on success"
        );
        assert_eq!(resets[0].0, "alice");
        assert_eq!(resets[0].1, ip);
    }

    impl MockUserRepository {
        fn update_status(&self, username: &str, status: UserStatus) {
            let key = username.trim().to_lowercase();
            let id = {
                let by_un = self.by_username.lock().unwrap();
                match by_un.get(&key) {
                    Some(id) => *id,
                    None => return,
                }
            };
            let mut by_id = self.by_id.lock().unwrap();
            if let Some(user) = by_id.get_mut(&id) {
                user.status = status;
            }
        }
    }

    struct CountingHasher {
        inner: PasswordHasherImpl,
        count: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    }
    impl PasswordHasherPort for CountingHasher {
        fn hash(&self, password: &str) -> Result<String, AuthError> {
            self.inner.hash(password)
        }
        fn verify(&self, password: &str, hash: &str) -> Result<(), AuthError> {
            self.count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            self.inner.verify(password, hash)
        }
        fn dummy_hash(&self) -> &str {
            self.inner.dummy_hash()
        }
    }

    struct CountingRateLimiter {
        inner: crate::rate_limit::AllowAllLoginRateLimiter,
        reset_log: std::sync::Arc<std::sync::Mutex<Vec<(String, std::net::IpAddr)>>>,
    }
    impl CountingRateLimiter {
        fn allow_all() -> Self {
            Self {
                inner: crate::rate_limit::AllowAllLoginRateLimiter,
                reset_log: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            }
        }
    }
    impl crate::rate_limit::LoginRateLimiter for CountingRateLimiter {
        fn check(
            &self,
            username: &str,
            ip: std::net::IpAddr,
        ) -> crate::rate_limit::RateLimitDecision {
            self.inner.check(username, ip)
        }
        fn record_failure(&self, username: &str, ip: std::net::IpAddr) {
            self.inner.record_failure(username, ip);
        }
        fn reset(&self, username: &str, ip: std::net::IpAddr) {
            self.reset_log.lock().unwrap().push((username.into(), ip));
        }
    }
}
