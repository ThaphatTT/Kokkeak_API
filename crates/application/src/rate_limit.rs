//! Login rate-limit port (`LoginRateLimiter`).
//!
//! The HTTP-layer rate limit (`tower_governor`, `rate_limit_redis`
//! middleware) is per-IP and applies to every route. This port is
//! **per-(username, IP)** and applies **only to login** — it
//! specifically defends against credential stuffing and password
//! spraying where one attacker IP tries many usernames, or one
//! attacker tries the same username from many IPs.
//!
//! The token format (success / failure) is intentionally separated:
//! - `check()` is the gate at the start of login.
//! - `record_failure()` is called from the failure paths.
//! - `reset()` is called on successful login so the legitimate user
//!   doesn't see lockout after typing their password correctly.
//!
//! ponytail: keeping three separate methods (rather than a single
//! `record_outcome(success: bool)`) lets the caller apply the
//! rate-limit check BEFORE running the (expensive, constant-time)
//! argon2 verify, so a locked-out brute-force attack doesn't burn
//! CPU on hash verification.

use std::net::IpAddr;
use std::time::Duration;

/// Outcome of a `check()` call. `Ok(())` means login may proceed;
/// `Err(retry_after)` means the (username, IP) pair is locked and
/// the caller should surface `429 Too Many Requests` with the
/// supplied retry-after hint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RateLimitDecision {
    /// Allow the login attempt to proceed.
    Allow,
    /// Locked out — wait at least `Duration` before retrying.
    Locked {
        /// How long the client should wait before the next attempt.
        retry_after: Duration,
    },
}

impl RateLimitDecision {
    /// `true` iff the caller may proceed with the login attempt.
    pub fn is_allowed(&self) -> bool {
        matches!(self, Self::Allow)
    }

    /// Seconds-until-retry for clients. `0` when allowed.
    pub fn retry_after_secs(&self) -> u64 {
        match self {
            Self::Allow => 0,
            Self::Locked { retry_after } => retry_after.as_secs().max(1),
        }
    }
}

/// Per-(username, IP) login rate limiter. Decides when to return
/// `RateLimitDecision::Locked` for a given (username, IP) pair
/// based on a sliding window of recent failures.
pub trait LoginRateLimiter: Send + Sync {
    /// Check whether the (username, IP) pair is currently locked.
    /// MUST NOT mutate state.
    fn check(&self, username: &str, ip: IpAddr) -> RateLimitDecision;

    /// Record a failed login attempt for this (username, IP). The
    /// limiter decides when this push tips the counter over the
    /// threshold (i.e. when `check()` starts returning `Locked`).
    fn record_failure(&self, username: &str, ip: IpAddr);

    /// Clear the record for this (username, IP) — call on successful
    /// login so the legitimate user is not penalised for typing
    /// their password correctly.
    fn reset(&self, username: &str, ip: IpAddr);
}

/// Test-only `LoginRateLimiter` that never locks anyone out. Used
/// by default in unit tests; production code wires a real impl
/// (in-memory for single-instance, Redis-backed for HA).
pub struct AllowAllLoginRateLimiter;

/// Production fallback when no rate limiter is wired (e.g. the
/// `LoginRateLimiter` port can't be constructed at startup).
/// Like `AllowAllLoginRateLimiter` — never locks anyone out —
/// but with the explicit "this is a degraded mode" semantic.
pub struct NoopLoginRateLimiter;

impl LoginRateLimiter for AllowAllLoginRateLimiter {
    fn check(&self, _username: &str, _ip: IpAddr) -> RateLimitDecision {
        RateLimitDecision::Allow
    }
    fn record_failure(&self, _username: &str, _ip: IpAddr) {}
    fn reset(&self, _username: &str, _ip: IpAddr) {}
}

impl LoginRateLimiter for NoopLoginRateLimiter {
    fn check(&self, _username: &str, _ip: IpAddr) -> RateLimitDecision {
        RateLimitDecision::Allow
    }
    fn record_failure(&self, _username: &str, _ip: IpAddr) {}
    fn reset(&self, _username: &str, _ip: IpAddr) {}
}

/// Test-only `LoginRateLimiter` that records every failure and
/// always returns `Locked` after the first one. Lets tests assert
/// that the auth path emits the right events when locked out.
pub struct AlwaysLockedRateLimiter {
    /// Every `(username, ip)` pair that hit `record_failure` since
    /// the limiter was constructed. Tests use this to assert
    /// that the auth path recorded the right failures.
    pub failures: std::sync::Mutex<Vec<(String, IpAddr)>>,
}

impl AlwaysLockedRateLimiter {
    /// Build a fresh limiter with an empty failure log.
    pub fn new() -> Self {
        Self {
            failures: std::sync::Mutex::new(Vec::new()),
        }
    }
}

impl Default for AlwaysLockedRateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

impl LoginRateLimiter for AlwaysLockedRateLimiter {
    fn check(&self, _username: &str, _ip: IpAddr) -> RateLimitDecision {
        RateLimitDecision::Locked {
            retry_after: Duration::from_secs(60),
        }
    }
    fn record_failure(&self, username: &str, ip: IpAddr) {
        self.failures.lock().unwrap().push((username.into(), ip));
    }
    fn reset(&self, _username: &str, _ip: IpAddr) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allow_all_limiter_never_locks() {
        let l = AllowAllLoginRateLimiter;
        for _ in 0..1000 {
            assert!(l.check("alice", "127.0.0.1".parse().unwrap()).is_allowed());
            l.record_failure("alice", "127.0.0.1".parse().unwrap());
        }
    }

    #[test]
    fn always_locked_blocks_first_attempt() {
        let l = AlwaysLockedRateLimiter::new();
        let decision = l.check("alice", "127.0.0.1".parse().unwrap());
        assert!(!decision.is_allowed());
        assert_eq!(decision.retry_after_secs(), 60);
    }

    #[test]
    fn retry_after_secs_clamps_to_one() {
        // Locked retry of <1s still tells the client to wait at
        // least 1s so we don't busy-loop.
        let d = RateLimitDecision::Locked {
            retry_after: Duration::from_millis(500),
        };
        assert_eq!(d.retry_after_secs(), 1);
    }
}
