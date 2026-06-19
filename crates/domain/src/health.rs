//! Health-check ports (traits) and a small registry that runs them in parallel.
//!
//! This module follows the hexagonal pattern from AGENTS.md § 6:
//! the [`HealthCheck`] trait is a **port**. Concrete adapters
//! (SQL Server, Redis, NATS, Mongo) live in `infra` and implement
//! this trait.
//!
//! M0 ships only the trait + registry — no concrete checks are
//! wired yet. M1+ adds the real adapters in `crates/infra/src/health/`.

use async_trait::async_trait;
use thiserror::Error;

/// Why a single health check failed.
#[derive(Debug, Error)]
pub enum HealthError {
    /// Adapters wrap any IO / network / auth failure into this variant.
    #[error("{0}")]
    Failed(String),
}

/// A single dependency probe.
///
/// Implementors are adapters (e.g. `SqlServerHealthCheck`) that live in
/// `infra`. They must be cheap (a `SELECT 1`, a Redis `PING`, a NATS
/// `flush`, etc.) and bounded by a short timeout.
#[async_trait]
pub trait HealthCheck: Send + Sync {
    /// Stable identifier surfaced in `/readyz` output and logs
    /// (e.g. `"sqlserver"`, `"redis"`, `"nats"`, `"mongo"`).
    fn name(&self) -> &str;

    /// Run the probe. `Ok(())` = up. `Err` = down (with a short reason).
    async fn check(&self) -> Result<(), HealthError>;
}

/// One row of the readiness report.
#[derive(Debug, Clone)]
pub struct CheckOutcome {
    /// Check identifier (from [`HealthCheck::name`]).
    pub name: String,
    /// `true` if the probe returned `Ok`.
    pub ok: bool,
    /// Human-readable failure reason (only set when `ok == false`).
    pub error: Option<String>,
}

impl CheckOutcome {
    fn up(name: String) -> Self {
        Self {
            name,
            ok: true,
            error: None,
        }
    }

    fn down(name: String, error: String) -> Self {
        Self {
            name,
            ok: false,
            error: Some(error),
        }
    }
}

/// Aggregated result of [`HealthRegistry::run_all`].
#[derive(Debug, Clone, Default)]
pub struct ReadyReport {
    /// One entry per registered check, in registration order.
    pub checks: Vec<CheckOutcome>,
}

impl ReadyReport {
    /// `true` iff every check passed (or the registry was empty).
    pub fn is_ready(&self) -> bool {
        self.checks.iter().all(|c| c.ok)
    }
}

/// Collection of [`HealthCheck`]s run together by `/readyz`.
///
/// Cheap to clone (the checks are behind `Arc`).
#[derive(Clone, Default)]
pub struct HealthRegistry {
    checks: Vec<std::sync::Arc<dyn HealthCheck>>,
}

impl HealthRegistry {
    /// Empty registry — every `/readyz` call returns 200 with no checks.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add one check. Order is preserved in `/readyz` output.
    pub fn register(&mut self, check: std::sync::Arc<dyn HealthCheck>) {
        self.checks.push(check);
    }

    /// Builder-style variant of [`Self::register`].
    #[must_use]
    pub fn with_check(mut self, check: std::sync::Arc<dyn HealthCheck>) -> Self {
        self.register(check);
        self
    }

    /// Number of registered checks.
    pub fn len(&self) -> usize {
        self.checks.len()
    }

    /// `true` when no checks have been registered.
    pub fn is_empty(&self) -> bool {
        self.checks.is_empty()
    }

    /// Run every registered check **in parallel** and collect outcomes.
    ///
    /// Checks are independent, so concurrency is the right call —
    /// the slowest check dominates total latency, not their sum.
    pub async fn run_all(&self) -> ReadyReport {
        use futures::future;

        let results = future::join_all(self.checks.iter().map(|check| {
            // Clone the name outside the async block so the borrow
            // does not span the `.await`.
            let name = check.name().to_string();
            async move {
                match check.check().await {
                    Ok(()) => CheckOutcome::up(name),
                    Err(err) => CheckOutcome::down(name, err.to_string()),
                }
            }
        }))
        .await;

        ReadyReport { checks: results }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct AlwaysOk;
    #[async_trait]
    impl HealthCheck for AlwaysOk {
        fn name(&self) -> &str {
            "always_ok"
        }
        async fn check(&self) -> Result<(), HealthError> {
            Ok(())
        }
    }

    struct AlwaysFail;
    #[async_trait]
    impl HealthCheck for AlwaysFail {
        fn name(&self) -> &str {
            "always_fail"
        }
        async fn check(&self) -> Result<(), HealthError> {
            Err(HealthError::Failed("simulated outage".into()))
        }
    }

    #[test]
    fn new_registry_is_empty() {
        let reg = HealthRegistry::new();
        assert!(reg.is_empty());
        assert_eq!(reg.len(), 0);
    }

    #[test]
    fn default_registry_is_empty() {
        let reg = HealthRegistry::default();
        assert!(reg.is_empty());
    }

    #[test]
    fn register_increments_len() {
        let mut reg = HealthRegistry::new();
        reg.register(std::sync::Arc::new(AlwaysOk));
        reg.register(std::sync::Arc::new(AlwaysFail));
        assert_eq!(reg.len(), 2);
        assert!(!reg.is_empty());
    }

    #[test]
    fn with_check_returns_updated_registry() {
        let reg = HealthRegistry::new().with_check(std::sync::Arc::new(AlwaysOk));
        assert_eq!(reg.len(), 1);
    }

    #[tokio::test]
    async fn run_all_with_no_checks_returns_empty_report() {
        let reg = HealthRegistry::new();
        let report = reg.run_all().await;
        assert!(report.checks.is_empty());
        // Empty registry = vacuously ready.
        assert!(report.is_ready());
    }

    #[tokio::test]
    async fn run_all_with_passing_check_reports_up() {
        let reg = HealthRegistry::new().with_check(std::sync::Arc::new(AlwaysOk));
        let report = reg.run_all().await;
        assert_eq!(report.checks.len(), 1);
        assert!(report.checks[0].ok);
        assert_eq!(report.checks[0].name, "always_ok");
        assert!(report.checks[0].error.is_none());
        assert!(report.is_ready());
    }

    #[tokio::test]
    async fn run_all_with_failing_check_reports_down() {
        let reg = HealthRegistry::new().with_check(std::sync::Arc::new(AlwaysFail));
        let report = reg.run_all().await;
        assert_eq!(report.checks.len(), 1);
        assert!(!report.checks[0].ok);
        assert_eq!(report.checks[0].name, "always_fail");
        assert_eq!(report.checks[0].error.as_deref(), Some("simulated outage"));
        assert!(!report.is_ready());
    }

    #[tokio::test]
    async fn run_all_reports_each_check_independently() {
        // Mixed: one up, one down -> overall not ready.
        let reg = HealthRegistry::new()
            .with_check(std::sync::Arc::new(AlwaysOk))
            .with_check(std::sync::Arc::new(AlwaysFail));
        let report = reg.run_all().await;
        assert_eq!(report.checks.len(), 2);
        assert!(report.checks[0].ok);
        assert!(!report.checks[1].ok);
        assert!(!report.is_ready());
    }
}
