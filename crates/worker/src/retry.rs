use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone)]
pub struct RetryPolicy {
    pub max_retries: u32,
    pub base_delay: Duration,
    pub max_delay: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: 5,
            base_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(30),
        }
    }
}

impl RetryPolicy {
    pub fn delay_for(&self, attempt: u32) -> Duration {
        if attempt == 0 {
            return Duration::ZERO;
        }
        let exp = self.base_delay.saturating_mul(1u32 << attempt.min(20));
        let capped = exp.min(self.max_delay);
        let max_ms = capped.as_millis().max(1) as u64;
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .subsec_nanos() as u64;
        let jitter_ms = nanos % (max_ms + 1);
        Duration::from_millis(jitter_ms)
    }

    pub fn should_retry(&self, attempt: u32) -> bool {
        attempt < self.max_retries
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_attempt_has_zero_delay() {
        let policy = RetryPolicy::default();
        assert_eq!(policy.delay_for(0), Duration::ZERO);
    }

    #[test]
    fn delay_is_within_bounds() {
        let policy = RetryPolicy {
            max_retries: 5,
            base_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(30),
        };
        for attempt in 1..=5 {
            let delay = policy.delay_for(attempt);
            assert!(
                delay <= policy.max_delay,
                "attempt {attempt} delay {delay:?} > max"
            );
        }
    }

    #[test]
    fn delay_has_jitter() {
        let policy = RetryPolicy::default();
        let delays: Vec<Duration> = (0..50).map(|_| policy.delay_for(1)).collect();
        let unique: std::collections::HashSet<u64> =
            delays.iter().map(|d| d.as_millis() as u64).collect();
        assert!(unique.len() > 1, "jitter should produce varied delays");
    }

    #[test]
    fn should_retry_respects_max() {
        let policy = RetryPolicy {
            max_retries: 3,
            ..Default::default()
        };
        assert!(policy.should_retry(0));
        assert!(policy.should_retry(1));
        assert!(policy.should_retry(2));
        assert!(!policy.should_retry(3));
    }
}
