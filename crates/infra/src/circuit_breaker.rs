use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use kokkak_domain::circuit_breaker::{CircuitBreakerConfig, CircuitSnapshot, CircuitState};

#[derive(Clone)]
pub struct CircuitBreaker {
    inner: Arc<CircuitBreakerInner>,
}

struct CircuitBreakerInner {
    config: CircuitBreakerConfig,
    state: tokio::sync::RwLock<CircuitState>,
    failure_count: AtomicU32,
    success_count: AtomicU32,
    last_failure_epoch: AtomicU64,
    half_open_calls: AtomicU32,
    _name: String,
}

fn now_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

impl CircuitBreaker {
    pub fn new(name: impl Into<String>, config: CircuitBreakerConfig) -> Self {
        Self {
            inner: Arc::new(CircuitBreakerInner {
                config,
                state: tokio::sync::RwLock::new(CircuitState::Closed),
                failure_count: AtomicU32::new(0),
                success_count: AtomicU32::new(0),
                last_failure_epoch: AtomicU64::new(0),
                half_open_calls: AtomicU32::new(0),
                _name: name.into(),
            }),
        }
    }

    pub async fn state(&self) -> CircuitState {
        let s = self.inner.state.read().await;
        if *s == CircuitState::Open {
            let last = self.inner.last_failure_epoch.load(Ordering::Relaxed);
            let now = now_epoch_secs();
            if now.saturating_sub(last) >= self.inner.config.open_duration_secs {
                drop(s);
                let mut w = self.inner.state.write().await;
                if *w == CircuitState::Open {
                    *w = CircuitState::HalfOpen;
                    self.inner.half_open_calls.store(0, Ordering::Relaxed);
                    self.inner.success_count.store(0, Ordering::Relaxed);
                    tracing::info!(circuit = %self.inner._name, "circuit half-open");
                }
                return *w;
            }
        }
        *s
    }

    pub async fn allow_request(&self) -> bool {
        match self.state().await {
            CircuitState::Closed => true,
            CircuitState::Open => false,
            CircuitState::HalfOpen => {
                let calls = self.inner.half_open_calls.fetch_add(1, Ordering::SeqCst);
                calls < self.inner.config.half_open_max_calls
            }
        }
    }

    pub async fn record_success(&self) {
        match self.state().await {
            CircuitState::HalfOpen { .. } => {
                let count = self.inner.success_count.fetch_add(1, Ordering::SeqCst) + 1;
                if count >= self.inner.config.half_open_max_calls {
                    let mut w = self.inner.state.write().await;
                    *w = CircuitState::Closed;
                    self.inner.failure_count.store(0, Ordering::Relaxed);
                    self.inner.half_open_calls.store(0, Ordering::Relaxed);
                    tracing::info!(circuit = %self.inner._name, "circuit closed");
                }
            }
            CircuitState::Closed => {
                self.inner.failure_count.store(0, Ordering::Relaxed);
            }
            _ => {}
        }
    }

    pub async fn record_failure(&self) {
        let current_state = self.state().await;
        match current_state {
            CircuitState::HalfOpen { .. } => {
                self.trip().await;
            }
            CircuitState::Closed => {
                let count = self.inner.failure_count.fetch_add(1, Ordering::SeqCst) + 1;
                if count >= self.inner.config.failure_threshold {
                    self.trip().await;
                }
            }
            _ => {}
        }
    }

    async fn trip(&self) {
        let mut w = self.inner.state.write().await;
        *w = CircuitState::Open;
        self.inner
            .last_failure_epoch
            .store(now_epoch_secs(), Ordering::Relaxed);
        self.inner.half_open_calls.store(0, Ordering::Relaxed);
        tracing::warn!(
            circuit = %self.inner._name,
            failure_count = self.inner.failure_count.load(Ordering::Relaxed),
            "circuit opened"
        );
    }

    pub async fn snapshot(&self) -> CircuitSnapshot {
        CircuitSnapshot {
            state: self.state().await,
            failure_count: self.inner.failure_count.load(Ordering::Relaxed),
            success_count: self.inner.success_count.load(Ordering::Relaxed),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fast_config() -> CircuitBreakerConfig {
        CircuitBreakerConfig {
            failure_threshold: 3,
            open_duration_secs: 0,
            half_open_max_calls: 1,
        }
    }

    #[tokio::test]
    async fn starts_closed() {
        let cb = CircuitBreaker::new("test", CircuitBreakerConfig::default());
        assert_eq!(cb.state().await, CircuitState::Closed);
    }

    #[tokio::test]
    async fn trips_after_threshold() {
        let cb = CircuitBreaker::new("test", fast_config());
        for _ in 0..3 {
            cb.record_failure().await;
        }
        assert_eq!(cb.state().await, CircuitState::Open);
    }

    #[tokio::test]
    async fn half_open_after_duration() {
        let cb = CircuitBreaker::new("test", fast_config());
        for _ in 0..3 {
            cb.record_failure().await;
        }
        assert_eq!(cb.state().await, CircuitState::Open);
        assert_eq!(cb.state().await, CircuitState::HalfOpen);
    }

    #[tokio::test]
    async fn half_open_success_closes() {
        let cb = CircuitBreaker::new("test", fast_config());
        for _ in 0..3 {
            cb.record_failure().await;
        }
        assert!(cb.allow_request().await);
        cb.record_success().await;
        assert_eq!(cb.state().await, CircuitState::Closed);
    }

    #[tokio::test]
    async fn half_open_failure_reopens() {
        let cb = CircuitBreaker::new("test", fast_config());
        for _ in 0..3 {
            cb.record_failure().await;
        }
        assert!(cb.allow_request().await);
        cb.record_failure().await;
        assert_eq!(cb.state().await, CircuitState::Open);
    }

    #[tokio::test]
    async fn closed_resets_failure_count_on_success() {
        let cb = CircuitBreaker::new("test", fast_config());
        cb.record_failure().await;
        cb.record_failure().await;
        cb.record_success().await;
        let snap = cb.snapshot().await;
        assert_eq!(snap.failure_count, 0);
    }
}
