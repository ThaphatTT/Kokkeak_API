

use argon2::password_hash::{
    rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString,
};
use argon2::Argon2;

use kokkak_domain::AuthError;

pub const MIN_PASSWORD_LEN: usize = 8;

#[derive(Clone)]
pub struct PasswordHasherImpl {

    dummy_hash: String,
}

const DUMMY_PASSWORD: &str = "__kokkak_constant_time_login_only__";

impl PasswordHasherImpl {

    pub fn new() -> Self {
        let argon2 = Argon2::default();
        let salt = SaltString::generate(&mut OsRng);
        let dummy_hash = argon2
            .hash_password(DUMMY_PASSWORD.as_bytes(), &salt)
            .map(|h| h.to_string())
            .expect("argon2 hash of dummy plaintext must succeed; this is a startup invariant");
        Self { dummy_hash }
    }

    pub fn hash(&self, password: &str) -> Result<String, AuthError> {
        if password.len() < MIN_PASSWORD_LEN {
            return Err(AuthError::Validation(format!(
                "password must be at least {MIN_PASSWORD_LEN} characters"
            )));
        }
        let salt = SaltString::generate(&mut OsRng);
        let argon2 = Argon2::default();
        argon2
            .hash_password(password.as_bytes(), &salt)
            .map(|h| h.to_string())
            .map_err(|e| AuthError::Backend(format!("argon2: {e}")))
    }

    pub fn verify(&self, password: &str, hash: &str) -> Result<(), AuthError> {
        let parsed = PasswordHash::new(hash)
            .map_err(|e| AuthError::Backend(format!("invalid hash: {e}")))?;
        Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .map_err(|_| AuthError::InvalidCredentials)
    }

    pub fn dummy_hash(&self) -> &str {
        &self.dummy_hash
    }
}

impl Default for PasswordHasherImpl {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_then_verify_round_trip() {
        let h = PasswordHasherImpl::new();
        let phc = h.hash("correct-horse-battery").unwrap();
        assert!(phc.starts_with("$argon2"));
        h.verify("correct-horse-battery", &phc).unwrap();
    }

    #[test]
    fn wrong_password_fails() {
        let h = PasswordHasherImpl::new();
        let phc = h.hash("correct-horse-battery").unwrap();
        let err = h.verify("wrong-password", &phc).unwrap_err();
        assert!(matches!(err, AuthError::InvalidCredentials));
    }

    #[test]
    fn short_password_rejected() {
        let h = PasswordHasherImpl::new();
        let err = h.hash("short").unwrap_err();
        assert!(matches!(err, AuthError::Validation(_)));
    }

    #[test]
    fn malformed_hash_returns_backend_error() {
        let h = PasswordHasherImpl::new();
        let err = h.verify("anything", "not-a-phc").unwrap_err();
        assert!(matches!(err, AuthError::Backend(_)));
    }

    #[test]
    fn two_hashes_of_same_password_differ() {

        let h = PasswordHasherImpl::new();
        let a = h.hash("a-good-password-123").unwrap();
        let b = h.hash("a-good-password-123").unwrap();
        assert_ne!(a, b);
        h.verify("a-good-password-123", &a).unwrap();
        h.verify("a-good-password-123", &b).unwrap();
    }
}
