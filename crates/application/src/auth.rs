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

use crate::audit::AuditEvent;
use crate::rate_limit::RateLimitDecision;

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
    /// Login name (NEW_DB schema; matches `[user_username].user_username`).
    pub username: String,
    /// Plain-text password supplied by the client. Hashed before storage.
    pub password: String,
    /// Token scope (`"mobile"` / `"web"` / `"admin"`).
    pub scope: String,
    /// Client IP. Sourced from `ConnectInfo<SocketAddr>` /
    /// `X-Forwarded-For` at the HTTP layer and threaded through here
    /// so the auth use case can drive per-(username, IP) rate
    /// limiting and audit logging. `None` for tests / non-HTTP
    /// callers; the auth path then keys on username alone.
    pub ip: Option<std::net::IpAddr>,
}

/// Result of register / login / refresh.
#[derive(Debug, Clone)]
pub struct AuthOutcome {
    /// Public-safe view of the authenticated user.
    pub user: PublicUser,
    /// Access + refresh token pair to return to the client.
    pub tokens: TokenPair,
}

/// Auth use case bundle.
pub struct AuthService {
    users: Arc<dyn UserRepository>,
    hasher: Arc<dyn PasswordHasherPort>,
    jwt: Arc<dyn JwtIssuerPort>,
    /// Audit sink. Receives one event per login / refresh / register
    /// outcome. See [`crate::audit::AuditLogger`].
    audit: Arc<dyn crate::audit::AuditLogger>,
    /// Login rate limiter. Drives per-(username, IP) lockout. See
    /// [`crate::rate_limit::LoginRateLimiter`].
    login_rl: Arc<dyn crate::rate_limit::LoginRateLimiter>,
}

impl AuthService {
    /// Construct the service bundle. All five ports are required at
    /// startup (composition root wires the concrete adapters).
    ///
    /// For tests / non-HTTP callers that don't have an IP, pass
    /// [`crate::rate_limit::AllowAllLoginRateLimiter`]. For dev /
    /// single-instance production, the in-memory limiter is enough;
    /// for multi-pod deployments, swap in a Redis-backed impl.
    pub fn new(
        users: Arc<dyn UserRepository>,
        hasher: Arc<dyn PasswordHasherPort>,
        jwt: Arc<dyn JwtIssuerPort>,
        audit: Arc<dyn crate::audit::AuditLogger>,
        login_rl: Arc<dyn crate::rate_limit::LoginRateLimiter>,
    ) -> Self {
        Self {
            users,
            hasher,
            jwt,
            audit,
            login_rl,
        }
    }

    /// Register a new account.
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
            // Fresh registration has no permission overrides yet;
            // role-based permissions land on the next login once the
            // SP can resolve them.
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
        let tokens = self.issue_pair(user.id, &user.roles, "mobile")?;
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

    /// Login by username + password.
    ///
    /// SECURITY — three layers of username-enumeration defense:
    ///
    /// 1. **Generic error**: every failure path (user-not-found,
    ///    wrong password, suspended / deleted / pending account)
    ///    collapses into [`AuthError::InvalidCredentials`]. The HTTP
    ///    layer renders the same body and status for all of them so
    ///    the client cannot distinguish "no such user" from "wrong
    ///    password". Required by OWASP Authentication Cheat Sheet
    ///    § Username Enumeration and NIST SP 800-63B § 5.2.2.
    ///
    /// 2. **Constant-time response**: even when the user is not
    ///    found, we still call [`PasswordHasherPort::verify`] against
    ///    a pre-computed dummy hash. Without this, a missing user
    ///    responds in ~1 ms while a wrong password against a real
    ///    hash takes ~50–200 ms (argon2id default cost), letting an
    ///    attacker enumerate valid usernames by latency alone.
    ///
    /// 3. **Per-(username, IP) rate limit**: after N consecutive
    ///    failures within the window, future calls return
    ///    [`AuthError::RateLimited`] without touching the hasher at
    ///    all. Defends against credential stuffing and password
    ///    spraying — one attacker IP trying many usernames (or many
    ///    IPs trying one username) hits the lockout.
    ///
    /// The specific reason is logged internally at WARN level AND
    /// emitted as a structured [`crate::audit::AuditEvent`] so the
    /// ops / SIEM pipeline can spot credential stuffing and account
    /// abuse without the client ever seeing the distinction.
    pub async fn login(&self, input: LoginInput) -> Result<AuthOutcome, AuthError> {
        let username = input.username.trim().to_lowercase();
        let ip = input.ip;

        // (3) Rate-limit gate — runs BEFORE the expensive argon2
        // verify so a locked-out brute-force attack doesn't burn
        // CPU. The decision is also audit-logged.
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

        // ponytail: pick the hash to verify BEFORE branching on the
        // user-found outcome. The dummy hash is pre-computed at
        // adapter construction so this branch is a string-slice
        // pick — no allocation, no extra argon2 work, no timing leak.
        let hash_to_check = user
            .as_ref()
            .map(|u| u.password_hash.as_str())
            .unwrap_or_else(|| self.hasher.dummy_hash());

        let verified = self.hasher.verify(&input.password, hash_to_check).is_ok();

        // Combine the two facts (user exists + password verified).
        // The dummy hash has a random salt per process so it cannot
        // match any real password — a successful verify therefore
        // implies `user.is_some()`.
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

        // unwrap is safe: failure_reason was None so user was Some
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

        // Success — reset the per-(username, IP) counter so the
        // legitimate user isn't penalised for typing their password
        // correctly after a few typos.
        if let Some(ip_addr) = ip {
            self.login_rl.reset(&username, ip_addr);
        }

        let tokens = self.issue_pair(user.id, &user.roles, &input.scope)?;
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

    /// Exchange a refresh token for a new access + refresh pair.
    ///
    /// No constant-time guard here — the refresh path does not call
    /// the password hasher (the JWT signature + the user-lookup is
    /// the only work). The status check still logs the specific
    /// reason so the ops pipeline can spot "token still valid but
    /// account got suspended after issue".
    pub async fn refresh(
        &self,
        refresh_token: &str,
        scope: &str,
    ) -> Result<AuthOutcome, AuthError> {
        let claims = self.jwt.verify(refresh_token)?;
        if claims.kind != TokenKind::Refresh {
            self.audit.log(
                AuditEvent::new("auth.refresh.failure")
                    .with_reason("invalid_token")
                    .with_context("detail", "not a refresh token"),
            );
            return Err(AuthError::InvalidToken("not a refresh token".into()));
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
        let tokens = self.issue_pair(user.id, &user.roles, scope)?;
        tracing::debug!(
            event = "auth.refresh.success",
            user_id = %user.id,
            "refresh succeeded",
        );
        self.audit.log(
            AuditEvent::new("auth.refresh.success")
                .with_username(&user.username)
                .with_user_id(user.id)
                .with_context("scope", scope.to_string()),
        );
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

/// Internal classification of *why* a login failed. Logged at WARN
/// level for the security team; never surfaced to the client (the
/// HTTP layer always returns a generic [`AuthError::InvalidCredentials`]).
#[derive(Debug, Clone, Copy)]
enum LoginFailureReason {
    /// The username does not exist in `user_username`.
    UserNotFound,
    /// The username exists but the password did not verify.
    WrongPassword,
    /// The account is suspended — contact support to restore.
    AccountSuspended,
    /// The account is soft-deleted (cannot log in).
    AccountDeleted,
    /// The account is pending email verification.
    AccountPending,
}

impl LoginFailureReason {
    /// Stable wire-shaped string used in `tracing` field values. These
    /// are stable contracts that the SIEM / dashboard layer may
    /// pivot on, so do not rename without coordinating with the ops team.
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
            // Active + non-authenticating shouldn't reach here
            // (`can_authenticate()` filters it), but if it does we
            // attribute it to wrong_password so we don't leak
            // account state.
            UserStatus::Active => Self::WrongPassword,
        }
    }
}

/// Emit a single `auth.login.failure` event with the structured reason.
/// SECURITY: never includes the password (or even its hash). The
/// username is logged as-is — if your threat model treats usernames
/// as PII, SHA256 it before logging. Output goes to `tracing` (JSON
/// in production via the JSON subscriber) and is picked up by the
/// log aggregator / SIEM for brute-force detection.
fn log_login_failure(username: &str, reason: LoginFailureReason) {
    tracing::warn!(
        event = "auth.login.failure",
        username = %username,
        reason = reason.as_str(),
        "login failed",
    );
}

/// Emit a single `auth.refresh.failure` event for a token whose
/// subject points at an account that can no longer authenticate.
fn log_refresh_failure(user_id: Uuid, reason: LoginFailureReason) {
    tracing::warn!(
        event = "auth.refresh.failure",
        user_id = %user_id,
        reason = reason.as_str(),
        "refresh failed: account cannot authenticate",
    );
}

/// Build a single login-failure audit event with the structured
/// fields the SIEM pivot on. Keeps the call sites compact.
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

/// Port for password hashing (decouples the use case from `argon2`).
pub trait PasswordHasherPort: Send + Sync {
    /// Hash a plain-text password (returns a PHC-format string).
    fn hash(&self, password: &str) -> Result<String, AuthError>;
    /// Verify a plain-text password against a stored PHC-format hash.
    fn verify(&self, password: &str, hash: &str) -> Result<(), AuthError>;
    /// Throwaway PHC string that no real password can match. Used by
    /// [`AuthService::login`] to keep the response time constant when
    /// the username does not exist (defense against username
    /// enumeration via timing — OWASP Authentication Cheat Sheet,
    /// NIST SP 800-63B § 5.2.2). The plaintext and salt are picked
    /// at construction time so the verify call costs the same as a
    /// real verify; the random salt ensures no real password can
    /// ever match the stored hash.
    fn dummy_hash(&self) -> &str;
}

/// Port for JWT issuing / verifying.
pub trait JwtIssuerPort: Send + Sync {
    /// Mint a short-lived access token for the given user / roles / scope.
    fn issue_access(&self, user_id: Uuid, roles: &[Role], scope: &str)
        -> Result<String, AuthError>;
    /// Mint a long-lived refresh token for the given user / roles / scope.
    fn issue_refresh(
        &self,
        user_id: Uuid,
        roles: &[Role],
        scope: &str,
    ) -> Result<String, AuthError>;
    /// Verify a token's signature + expiry and return its claims.
    fn verify(&self, token: &str) -> Result<Claims, AuthError>;
    /// Access token TTL in seconds (used by the cookie `Max-Age`).
    fn access_ttl_secs(&self) -> i64;
    /// Refresh token TTL in seconds.
    fn refresh_ttl_secs(&self) -> i64;
}

// ---- Adapters live in the api crate (composition root) ----

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
        // M17 cleanup: only `list_with_permissions` remains (used by
        // the admin user-list screen). `find_user_permissions_by_username`
        // moved to the dedicated `PermissionUserRepository` port; the
        // auth/login mock no longer needs to implement it.
        async fn list_with_permissions(
            &self,
        ) -> Result<Vec<kokkak_domain::UserListRow>, kokkak_domain::RepoError> {
            Ok(Vec::new())
        }
    }

    use kokkak_infra::auth::jwt::JwtService;
    use kokkak_infra::auth::password::PasswordHasherImpl;
    // skip MssqlUserRepository;

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
        fn dummy_hash(&self) -> &str {
            // Delegate to the production hasher — the real argon2id
            // hash is pre-computed at construction and lives inside
            // the wrapped `PasswordHasherImpl`.
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

    async fn make_service() -> AuthService {
        make_service_with_repo().await.0
    }

    /// Build a service alongside its concrete mock repo so the test
    /// can mutate user state (e.g. flip status to Suspended) before
    /// driving login. The mock lives behind `Arc<dyn UserRepository>`
    /// inside the service — mutating the inner state is visible to
    /// every subsequent lookup because both sides hold the same Arc.
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
            .refresh(&registered.tokens.access_token, "mobile")
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

    // ---- Username-enumeration defense (T-LoginEnum) ----

    #[tokio::test]
    async fn login_unknown_user_returns_generic_invalid_credentials() {
        // SECURITY: no matter what the failure cause, the client
        // must see the same error body. The specific reason is
        // logged internally for the SIEM pipeline.
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
        // A suspended user with the right password must STILL see
        // the same generic error. The internal log carries the
        // specific reason.
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
        // SECURITY: even when the user is not found, we must call
        // `verify` (against the dummy hash) so the response time
        // doesn't leak "user-not-found" via latency. We verify
        // this by using a counting hasher.
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

        // Login against a non-existent user — verify MUST still run.
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

    // ---- Audit log (T-AuditLogin) ----

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

    // ---- Rate limiting (T-LoginRateLimit) ----

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
        // 429 surfaces through AuthError::RateLimited(secs).
        match err {
            AuthError::RateLimited(secs) => assert!(secs >= 1, "retry_after must be ≥ 1s"),
            other => panic!("expected RateLimited, got {other:?}"),
        }
        // And the audit log carries the rate_limited event.
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
        // The rate limiter is keyed on (username, IP). When no IP is
        // available (tests / non-HTTP callers / behind a proxy that
        // strips the header), we skip the gate rather than block on
        // something we can't key — the HTTP middleware still applies
        // its per-IP cap.
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
        // ip = None: must NOT hit the rate limiter (it's keyed on
        // IP), so login proceeds normally even though the limiter
        // would otherwise block everyone.
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
        // Wire a counting limiter so we can observe that reset() was
        // called on success.
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
        /// Test-only: flip a stored user's status in place. Goes
        /// through the public `update` path of the trait so we
        /// exercise the real `User.password_hash` clone.
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

    /// Counting hasher: wraps `PasswordHasherImpl` and tallies every
    /// `verify` call. Used by the constant-time login test to assert
    /// that the verify path runs even when the user is missing.
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

    /// Counting rate limiter: like `AllowAllLoginRateLimiter` but
    /// records every `reset()` call so tests can assert that
    /// successful login calls `reset()` exactly once.
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
