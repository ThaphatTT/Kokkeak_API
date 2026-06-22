//! Password hashing (M2).
//!
//! Uses argon2id (argon2 crate). The hash is stored as a PHC string
//! (`$argon2id$v=19$m=...,t=...,p=...$salt$hash`) so the cost
//! parameters are encoded alongside the hash and can be tuned
//! without rewriting the table.
//!
//! Per AGENTS.md § 12.1, plain-text passwords NEVER appear in the
//! codebase outside the brief moment between request read and the
//! `hash()` call. The handler validates first, then hashes inside
//! the use case.

use argon2::password_hash::{
    rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString,
};
use argon2::Argon2;

use kokkak_domain::AuthError;

/// Minimum password length (matches AGENTS.md § 11.5 intent).
pub const MIN_PASSWORD_LEN: usize = 8;

/// Argon2id hasher.
///
/// Carries a pre-computed throwaway hash (see [`Self::dummy_hash`]) used
/// by the login flow to keep response time constant when the
/// username is unknown (OWASP Authentication Cheat Sheet § Username
/// Enumeration, NIST SP 800-63B § 5.2.2).
#[derive(Clone)]
pub struct PasswordHasherImpl {
    /// PHC string of a throwaway plaintext. The salt is random per
    /// process instance so no real password can ever match this
    /// hash; the cost parameters match `Argon2::default()` so a
    /// `verify()` call against it takes the same wall-clock time
    /// as a verify against a real user's hash.
    dummy_hash: String,
}

/// Plaintext used to pre-compute [`PasswordHasherImpl::dummy_hash`].
/// Embedded as a fixed string so the value is reproducible across
/// processes (helps when checking logs / forensics). The salt is
/// still random per `new()` — only the plaintext is constant.
const DUMMY_PASSWORD: &str = "__kokkak_constant_time_login_only__";

impl PasswordHasherImpl {
    /// Construct an argon2id hasher and pre-compute the dummy hash
    /// used by the constant-time login guard. The dummy hash
    /// generation runs once at startup (~50–200 ms); it is NOT
    /// re-computed on every login call.
    pub fn new() -> Self {
        let argon2 = Argon2::default();
        let salt = SaltString::generate(&mut OsRng);
        let dummy_hash = argon2
            .hash_password(DUMMY_PASSWORD.as_bytes(), &salt)
            .map(|h| h.to_string())
            .expect("argon2 hash of dummy plaintext must succeed; this is a startup invariant");
        Self { dummy_hash }
    }

    /// Hash a plaintext password. Use this on **register / reset**
    /// flows only.
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

    /// Verify a plaintext password against a stored PHC string.
    /// Returns `Ok(())` when the password matches, `Err(InvalidCredentials)`
    /// when it does not (or the hash is malformed).
    pub fn verify(&self, password: &str, hash: &str) -> Result<(), AuthError> {
        let parsed = PasswordHash::new(hash)
            .map_err(|e| AuthError::Backend(format!("invalid hash: {e}")))?;
        Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .map_err(|_| AuthError::InvalidCredentials)
    }

    /// Throwaway PHC string for the constant-time login guard. The
    /// returned reference is borrowed from `self` and lives for the
    /// lifetime of the hasher (typically the whole process).
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
        // salt is random per hash → outputs differ.
        let h = PasswordHasherImpl::new();
        let a = h.hash("a-good-password-123").unwrap();
        let b = h.hash("a-good-password-123").unwrap();
        assert_ne!(a, b);
        h.verify("a-good-password-123", &a).unwrap();
        h.verify("a-good-password-123", &b).unwrap();
    }
}
