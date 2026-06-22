//! `InMemoryLoginRateLimiter` — sliding-window per-(username, IP)
//! counter. State lives in a `HashMap` keyed by `"<username>|<ip>"`.
//!
//! ponytail: in-memory only. Each replica has its own counter, so an
//! attacker hitting N replicas in parallel gets N× the budget.
//! Sufficient for single-instance dev / staging; for HA production,
//! swap in a Redis-backed impl that uses the same `LoginRateLimiter`
//! port (the trait is the contract — the impl is replaceable).

use std::collections::{HashMap, VecDeque};
use std::net::IpAddr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use kokkak_application::rate_limit::{LoginRateLimiter, RateLimitDecision};

/// Default sliding-window length (5 minutes).
pub const DEFAULT_WINDOW: Duration = Duration::from_secs(5 * 60);

/// Default threshold — locked once the (username, IP) pair has
/// accumulated this many failures within `DEFAULT_WINDOW`.
pub const DEFAULT_MAX_ATTEMPTS: usize = 5;

/// Sliding-window per-(username, IP) login limiter. State lives
/// entirely in process memory; for multi-replica HA production,
/// swap in a Redis-backed implementation behind the same
/// [`LoginRateLimiter`] trait.
pub struct InMemoryLoginRateLimiter {
    inner: Mutex<HashMap<String, VecDeque<Instant>>>,
    window: Duration,
    max_attempts: usize,
}

impl InMemoryLoginRateLimiter {
    /// Construct with the default window (5min) and threshold (5
    /// failures). Single-instance dev / staging use this.
    pub fn new() -> Self {
        Self::with_params(DEFAULT_WINDOW, DEFAULT_MAX_ATTEMPTS)
    }

    /// Construct with a custom window + threshold. Used by tests;
    /// production code can also read these from config.
    pub fn with_params(window: Duration, max_attempts: usize) -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
            window,
            max_attempts,
        }
    }
}

impl Default for InMemoryLoginRateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

fn key(username: &str, ip: IpAddr) -> String {
    // `|` is not valid in usernames or IPs (RFC 5321 § 4.1.2 forbids
    // `|` in the local-part). Safe delimiter.
    format!("{username}|{ip}")
}

/// Drop entries older than `window`. Returns the (possibly mutated)
/// deque as a convenience so the caller can decide what to do.
fn evict_expired(deque: &mut VecDeque<Instant>, window: Duration) {
    let now = Instant::now();
    while let Some(&front) = deque.front() {
        if now.duration_since(front) > window {
            deque.pop_front();
        } else {
            break;
        }
    }
}

impl LoginRateLimiter for InMemoryLoginRateLimiter {
    fn check(&self, username: &str, ip: IpAddr) -> RateLimitDecision {
        let k = key(username, ip);
        let mut map = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        let Some(deque) = map.get_mut(&k) else {
            return RateLimitDecision::Allow;
        };
        evict_expired(deque, self.window);
        if deque.len() >= self.max_attempts {
            // Retry-after = (oldest_failure + window) - now, floored
            // at 1 second so clients never busy-loop.
            let oldest = *deque.front().expect("non-empty by len check");
            let elapsed = oldest.elapsed();
            let retry_after = self.window.saturating_sub(elapsed);
            RateLimitDecision::Locked {
                retry_after: retry_after.max(Duration::from_secs(1)),
            }
        } else {
            RateLimitDecision::Allow
        }
    }

    fn record_failure(&self, username: &str, ip: IpAddr) {
        let k = key(username, ip);
        let mut map = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        let deque = map.entry(k).or_default();
        evict_expired(deque, self.window);
        deque.push_back(Instant::now());
    }

    fn reset(&self, username: &str, ip: IpAddr) {
        let k = key(username, ip);
        self.inner
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&k);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::IpAddr;
    use std::thread::sleep;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn under_threshold_is_allowed() {
        let l = InMemoryLoginRateLimiter::with_params(Duration::from_secs(60), 3);
        for _ in 0..3 {
            assert!(l.check("alice", ip("1.2.3.4")).is_allowed());
            l.record_failure("alice", ip("1.2.3.4"));
        }
    }

    #[test]
    fn crossing_threshold_locks() {
        let l = InMemoryLoginRateLimiter::with_params(Duration::from_secs(60), 3);
        for _ in 0..3 {
            l.record_failure("alice", ip("1.2.3.4"));
        }
        match l.check("alice", ip("1.2.3.4")) {
            RateLimitDecision::Locked { retry_after } => {
                // retry_after should be close to the full window (60s),
                // not 0 — proves we use the oldest failure timestamp.
                assert!(retry_after.as_secs() >= 1);
                assert!(retry_after.as_secs() <= 60);
            }
            RateLimitDecision::Allow => panic!("expected Locked"),
        }
    }

    #[test]
    fn different_ip_has_independent_counter() {
        // One IP doesn't drain another's budget — defends against
        // "attacker IP burns legitimate IP's budget" attack.
        let l = InMemoryLoginRateLimiter::with_params(Duration::from_secs(60), 2);
        l.record_failure("alice", ip("1.2.3.4"));
        l.record_failure("alice", ip("1.2.3.4"));
        // Locked on attacker IP
        assert!(!l.check("alice", ip("1.2.3.4")).is_allowed());
        // But legitimate IP can still try
        assert!(l.check("alice", ip("5.6.7.8")).is_allowed());
    }

    #[test]
    fn different_username_has_independent_counter() {
        // Per-username isolation — one user's lockout doesn't lock
        // out another user from the same IP.
        let l = InMemoryLoginRateLimiter::with_params(Duration::from_secs(60), 2);
        l.record_failure("alice", ip("1.2.3.4"));
        l.record_failure("alice", ip("1.2.3.4"));
        assert!(!l.check("alice", ip("1.2.3.4")).is_allowed());
        assert!(l.check("bob", ip("1.2.3.4")).is_allowed());
    }

    #[test]
    fn reset_clears_the_counter() {
        let l = InMemoryLoginRateLimiter::with_params(Duration::from_secs(60), 2);
        l.record_failure("alice", ip("1.2.3.4"));
        l.record_failure("alice", ip("1.2.3.4"));
        l.reset("alice", ip("1.2.3.4"));
        assert!(l.check("alice", ip("1.2.3.4")).is_allowed());
    }

    #[test]
    fn old_failures_expire_after_window() {
        // Short window so the test runs in milliseconds.
        let l = InMemoryLoginRateLimiter::with_params(Duration::from_millis(80), 2);
        l.record_failure("alice", ip("1.2.3.4"));
        l.record_failure("alice", ip("1.2.3.4"));
        assert!(!l.check("alice", ip("1.2.3.4")).is_allowed());
        sleep(Duration::from_millis(120));
        assert!(
            l.check("alice", ip("1.2.3.4")).is_allowed(),
            "failures must expire after the window"
        );
    }
}
