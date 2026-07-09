use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

use crate::user::{Permission, Role};

#[derive(Debug, Clone, Error)]
pub enum AuthError {
    #[error("invalid credentials")]
    InvalidCredentials,

    #[error("token expired")]
    TokenExpired,

    #[error("invalid token: {0}")]
    InvalidToken(String),

    #[error("forbidden: {0}")]
    Forbidden(String),

    #[error("username already in use")]
    UsernameTaken,

    #[error("validation: {0}")]
    Validation(String),

    #[error("auth backend error: {0}")]
    Backend(String),

    #[error("rate limited (retry after {0}s)")]
    RateLimited(u64),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub sub: Uuid,

    pub iss: String,

    pub iat: i64,

    pub exp: i64,

    pub kind: TokenKind,

    pub roles: Vec<Role>,

    pub scope: String,

    pub jti: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TokenKind {
    Access,

    Refresh,
}

impl TokenKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Access => "access",
            Self::Refresh => "refresh",
        }
    }
}

#[derive(Debug, Clone)]
pub struct AuthSession {
    pub user_id: Uuid,

    pub roles: Vec<Role>,

    pub expires_at: DateTime<Utc>,

    pub scope: String,

    pub jti: String,
}

impl AuthSession {
    pub fn has_role(&self, role: Role) -> bool {
        self.roles.contains(&role)
    }

    pub fn is_admin(&self) -> bool {
        self.roles
            .iter()
            .any(|r| matches!(r, Role::Admin | Role::SuperAdmin))
    }
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct PublicUser {
    pub id: Uuid,

    pub username: String,

    pub first_name: String,

    pub last_name: String,

    pub roles: Vec<Role>,

    pub permissions: Vec<Permission>,

    pub status: crate::user::UserStatus,

    pub created_at: DateTime<Utc>,
}

impl From<&crate::user::User> for PublicUser {
    fn from(u: &crate::user::User) -> Self {
        Self {
            id: u.id,
            username: u.username.clone(),
            first_name: u.first_name.clone(),
            last_name: u.last_name.clone(),
            roles: u.roles.clone(),
            permissions: u.permissions.clone(),
            status: u.status,
            created_at: u.created_at,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct TokenPair {
    pub access_token: String,

    pub refresh_token: String,

    pub access_ttl_secs: i64,

    pub refresh_ttl_secs: i64,

    pub token_type: &'static str,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SessionInfo {
    pub jti: String,
    pub user_id: Uuid,
    pub scope: String,
    pub device: String,
    pub ip: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateSession {
    pub jti: String,
    pub user_id: Uuid,
    pub scope: String,
    pub device: String,
    pub ip: String,
    pub ttl_secs: i64,
}

#[async_trait]
pub trait SessionStore: Send + Sync {
    async fn create(&self, session: &CreateSession) -> Result<(), AuthError>;
    async fn get(&self, user_id: Uuid, jti: &str) -> Result<Option<SessionInfo>, AuthError>;
    async fn revoke(&self, user_id: Uuid, jti: &str) -> Result<(), AuthError>;
    async fn revoke_all(&self, user_id: Uuid) -> Result<u64, AuthError>;
    async fn list(&self, user_id: Uuid) -> Result<Vec<SessionInfo>, AuthError>;
}

pub struct NoopSessionStore;

#[async_trait]
impl SessionStore for NoopSessionStore {
    async fn create(&self, _session: &CreateSession) -> Result<(), AuthError> {
        Ok(())
    }
    async fn get(&self, _user_id: Uuid, _jti: &str) -> Result<Option<SessionInfo>, AuthError> {
        Ok(None)
    }
    async fn revoke(&self, _user_id: Uuid, _jti: &str) -> Result<(), AuthError> {
        Ok(())
    }
    async fn revoke_all(&self, _user_id: Uuid) -> Result<u64, AuthError> {
        Ok(0)
    }
    async fn list(&self, _user_id: Uuid) -> Result<Vec<SessionInfo>, AuthError> {
        Ok(vec![])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_kind_as_str_is_snake_case() {
        assert_eq!(TokenKind::Access.as_str(), "access");
        assert_eq!(TokenKind::Refresh.as_str(), "refresh");
    }

    #[test]
    fn claims_serde_round_trip() {
        let now = chrono::Utc::now().timestamp();
        let c = Claims {
            sub: Uuid::new_v4(),
            iss: "kokkak-api".into(),
            iat: now,
            exp: now + 900,
            kind: TokenKind::Access,
            roles: vec![Role::Customer, Role::Admin],
            scope: "web".into(),
            jti: Uuid::new_v4().to_string(),
        };
        let s = serde_json::to_string(&c).unwrap();
        let v: serde_json::Value = serde_json::from_str(&s).unwrap();
        assert_eq!(v["kind"], "access");
        assert_eq!(v["scope"], "web");
        assert_eq!(v["roles"][0], "customer");
        assert_eq!(v["roles"][1], "admin");
    }

    #[test]
    fn auth_session_role_check() {
        let s = AuthSession {
            user_id: Uuid::new_v4(),
            roles: vec![Role::Admin],
            expires_at: Utc::now(),
            scope: "admin".into(),
            jti: "test-jti".into(),
        };
        assert!(s.has_role(Role::Admin));
        assert!(s.is_admin());
        assert!(!s.has_role(Role::Customer));
    }

    #[test]
    fn public_user_omits_password_hash() {
        let now = Utc::now();
        let u = crate::user::User {
            id: Uuid::new_v4(),
            first_name: "A".into(),
            last_name: "B".into(),
            username: "ab".into(),
            password_hash: "$argon2id$SECRET".into(),
            roles: vec![Role::Customer],
            permissions: Vec::new(),
            status: crate::user::UserStatus::Active,
            created_at: now,
            updated_at: now,
        };
        let pubu = PublicUser::from(&u);
        let s = serde_json::to_string(&pubu).unwrap();
        assert!(!s.contains("password"));
        assert!(!s.contains("SECRET"));
        assert!(s.contains("username"));
        assert!(s.contains("first_name"));
        assert!(s.contains("last_name"));
    }

    #[test]
    fn public_user_exposes_permissions_as_codes() {
        let now = Utc::now();
        let u = crate::user::User {
            id: Uuid::new_v4(),
            first_name: "A".into(),
            last_name: "B".into(),
            username: "ab".into(),
            password_hash: "x".into(),
            roles: vec![Role::Admin],
            permissions: vec![Permission::PageJobsView, Permission::JobsCreate],
            status: crate::user::UserStatus::Active,
            created_at: now,
            updated_at: now,
        };
        let pubu = PublicUser::from(&u);
        let v: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&pubu).unwrap()).unwrap();
        assert_eq!(
            v["permissions"],
            serde_json::json!(["JOBS_VIEW", "JOBS_CREATE"])
        );
    }
}
