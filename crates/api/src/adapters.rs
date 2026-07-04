

use std::sync::Arc;

use kokkak_application::auth::{JwtIssuerPort, PasswordHasherPort};
use kokkak_domain::{AuthError, Claims, Role};
use kokkak_infra::auth::jwt::JwtService;
use kokkak_infra::auth::password::PasswordHasherImpl;
use uuid::Uuid;

pub struct PasswordHasherAdapter {
    inner: Arc<PasswordHasherImpl>,
}

impl PasswordHasherAdapter {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(PasswordHasherImpl::new()),
        }
    }
}

impl Default for PasswordHasherAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl PasswordHasherPort for PasswordHasherAdapter {
    fn hash(&self, password: &str) -> Result<String, AuthError> {
        self.inner.hash(password)
    }
    fn verify(&self, password: &str, hash: &str) -> Result<(), AuthError> {
        self.inner.verify(password, hash)
    }
    fn dummy_hash(&self) -> &str {
        self.inner.dummy_hash()
    }
}

pub struct JwtIssuerAdapter {
    inner: Arc<JwtService>,
}

impl JwtIssuerAdapter {
    pub fn new(svc: Arc<JwtService>) -> Self {
        Self { inner: svc }
    }
}

impl JwtIssuerPort for JwtIssuerAdapter {
    fn issue_access(
        &self,
        user_id: Uuid,
        roles: &[Role],
        scope: &str,
    ) -> Result<String, AuthError> {
        self.inner.issue_access(user_id, roles, scope)
    }
    fn issue_refresh(
        &self,
        user_id: Uuid,
        roles: &[Role],
        scope: &str,
    ) -> Result<String, AuthError> {
        self.inner.issue_refresh(user_id, roles, scope)
    }
    fn verify(&self, token: &str) -> Result<Claims, AuthError> {
        self.inner.verify(token)
    }
    fn access_ttl_secs(&self) -> i64 {
        self.inner.access_ttl_secs()
    }
    fn refresh_ttl_secs(&self) -> i64 {
        self.inner.refresh_ttl_secs()
    }
}
