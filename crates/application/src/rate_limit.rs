

use std::net::IpAddr;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RateLimitDecision {

    Allow,

    Locked {

        retry_after: Duration,
    },
}

impl RateLimitDecision {

    pub fn is_allowed(&self) -> bool {
        matches!(self, Self::Allow)
    }

    pub fn retry_after_secs(&self) -> u64 {
        match self {
            Self::Allow => 0,
            Self::Locked { retry_after } => retry_after.as_secs().max(1),
        }
    }
}

pub trait LoginRateLimiter: Send + Sync {

    fn check(&self, username: &str, ip: IpAddr) -> RateLimitDecision;

    fn record_failure(&self, username: &str, ip: IpAddr);

    fn reset(&self, username: &str, ip: IpAddr);
}

pub struct AllowAllLoginRateLimiter;

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

pub struct AlwaysLockedRateLimiter {

    pub failures: std::sync::Mutex<Vec<(String, IpAddr)>>,
}

impl AlwaysLockedRateLimiter {

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

        let d = RateLimitDecision::Locked {
            retry_after: Duration::from_millis(500),
        };
        assert_eq!(d.retry_after_secs(), 1);
    }
}
