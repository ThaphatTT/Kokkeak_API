//! Auth domain types (โดเมนยืนยันตัวตน — M2 + M14).
//!
//! Defines the typed errors and the **claims** the application / API
//! layers exchange. Concrete JWT issue / verify lives in `infra::auth::jwt`
//! so the domain stays free of `jsonwebtoken`.
//!
//! M14 renamed `EmailTaken` → `UsernameTaken` to match the NEW_DB
//! schema where login is `user_username_username` (not email).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

use crate::user::Role;

/// Typed auth errors (mapped to HTTP statuses by the API layer).
#[derive(Debug, Error)]
pub enum AuthError {
    /// 401 — credentials missing or invalid.
    #[error("invalid credentials")]
    InvalidCredentials,

    /// 401 — token expired.
    #[error("token expired")]
    TokenExpired,

    /// 401 — token signature / format invalid.
    #[error("invalid token: {0}")]
    InvalidToken(String),

    /// 403 — authenticated but not allowed.
    #[error("forbidden: {0}")]
    Forbidden(String),

    /// 409 — username already in use. Was `EmailTaken` in the legacy
    /// code; renamed to match NEW_DB which stores login in
    /// `user_username_username`.
    #[error("username already in use")]
    UsernameTaken,

    /// 422 — input validation failure.
    #[error("validation: {0}")]
    Validation(String),

    /// 500 — backend (argon2 / repo) failure.
    #[error("auth backend error: {0}")]
    Backend(String),
}

/// JWT claims (the body of every access / refresh token).
///
/// The `sub` is the user UUID, `roles` is duplicated into the token
/// to avoid a DB lookup on every request (revocation is handled by
/// the access-token TTL + the refresh-token blacklist in Redis).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    /// Subject (user UUID).
    pub sub: Uuid,
    /// Issuer.
    pub iss: String,
    /// Issued-at (unix seconds).
    pub iat: i64,
    /// Expiry (unix seconds).
    pub exp: i64,
    /// Token kind (`"access"` or `"refresh"`).
    pub kind: TokenKind,
    /// Roles embedded for fast RBAC checks.
    pub roles: Vec<Role>,
    /// Token type scope (e.g. `mobile`, `web`, `admin`).
    pub scope: String,
}

/// Distinguishes access tokens from refresh tokens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TokenKind {
    /// Short-lived API access token.
    Access,
    /// Long-lived refresh token (used to mint access tokens).
    Refresh,
}

impl TokenKind {
    /// Snake-case identifier.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Access => "access",
            Self::Refresh => "refresh",
        }
    }
}

/// The authenticated session that handlers receive via the
/// `AuthnUser` extractor.
#[derive(Debug, Clone)]
pub struct AuthSession {
    /// User id from the JWT.
    pub user_id: Uuid,
    /// Roles copied from the JWT.
    pub roles: Vec<Role>,
    /// Expiry of the underlying access token.
    pub expires_at: DateTime<Utc>,
    /// Token scope (`mobile` / `web` / `admin`).
    pub scope: String,
}

impl AuthSession {
    /// `true` iff the user has the given role.
    pub fn has_role(&self, role: Role) -> bool {
        self.roles.contains(&role)
    }

    /// `true` iff the user is admin-level.
    pub fn is_admin(&self) -> bool {
        self.roles
            .iter()
            .any(|r| matches!(r, Role::Admin | Role::SuperAdmin))
    }
}

/// Public-safe user view (no password hash).
///
/// M14 fields match the NEW_DB-aligned `User` aggregate:
/// - `username` (was `email`)
/// - `first_name` + `last_name` (was `display_name`)
/// - no `locale` (locale comes from JWT / Accept-Language per M11)
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct PublicUser {
    /// Stable user id (GUID v4).
    pub id: Uuid,
    /// Unique login name (NEW_DB schema; was `email` pre-M14).
    pub username: String,
    /// Given name (NEW_DB schema).
    pub first_name: String,
    /// Family name (NEW_DB schema).
    pub last_name: String,
    /// Roles assigned via `[user_user_role]` join (M14).
    pub roles: Vec<Role>,
    /// Account lifecycle status (0=Active, 1=Suspended, 2=Locked, 3=Disabled).
    pub status: crate::user::UserStatus,
    /// UTC timestamp the user record was first persisted.
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
            status: u.status,
            created_at: u.created_at,
        }
    }
}

/// Result of a successful login / refresh.
#[derive(Debug, Clone, Serialize)]
pub struct TokenPair {
    /// Short-lived access token.
    pub access_token: String,
    /// Long-lived refresh token.
    pub refresh_token: String,
    /// Access token TTL in seconds.
    pub access_ttl_secs: i64,
    /// Refresh token TTL in seconds.
    pub refresh_ttl_secs: i64,
    /// Token type (`"Bearer"`).
    pub token_type: &'static str,
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
}
