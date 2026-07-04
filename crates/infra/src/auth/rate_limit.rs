

use std::collections::{HashMap, VecDeque};
use std::net::IpAddr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use kokkak_application::rate_limit::{LoginRateLimiter, RateLimitDecision};

pub const DEFAULT_WINDOW: Duration = Duration::from_secs(5 * 60);

pub const DEFAULT_MAX_ATTEMPTS: usize = 5;

pub struct InMemoryLoginRateLimiter {
    inner: Mutex<HashMap<String, VecDeque<Instant>>>,
    window: Duration,
    max_attempts: usize,
}

impl InMemoryLoginRateLimiter {

    pub fn new() -> Self {
        Self::with_params(DEFAULT_WINDOW, DEFAULT_MAX_ATTEMPTS)
    }

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

    format!("{username}|{ip}")
}

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

                assert!(retry_after.as_secs() >= 1);
                assert!(retry_after.as_secs() <= 60);
            }
            RateLimitDecision::Allow => panic!("expected Locked"),
        }
    }

    #[test]
    fn different_ip_has_independent_counter() {

        let l = InMemoryLoginRateLimiter::with_params(Duration::from_secs(60), 2);
        l.record_failure("alice", ip("1.2.3.4"));
        l.record_failure("alice", ip("1.2.3.4"));

        assert!(!l.check("alice", ip("1.2.3.4")).is_allowed());

        assert!(l.check("alice", ip("5.6.7.8")).is_allowed());
    }

    #[test]
    fn different_username_has_independent_counter() {

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
