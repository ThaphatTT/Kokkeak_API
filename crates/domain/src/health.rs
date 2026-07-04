

use async_trait::async_trait;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum HealthError {

    #[error("{0}")]
    Failed(String),
}

#[async_trait]
pub trait HealthCheck: Send + Sync {

    fn name(&self) -> &str;

    async fn check(&self) -> Result<(), HealthError>;
}

#[derive(Debug, Clone)]
pub struct CheckOutcome {

    pub name: String,

    pub ok: bool,

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

#[derive(Debug, Clone, Default)]
pub struct ReadyReport {

    pub checks: Vec<CheckOutcome>,
}

impl ReadyReport {

    pub fn is_ready(&self) -> bool {
        self.checks.iter().all(|c| c.ok)
    }
}

#[derive(Clone, Default)]
pub struct HealthRegistry {
    checks: Vec<std::sync::Arc<dyn HealthCheck>>,
}

impl HealthRegistry {

    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, check: std::sync::Arc<dyn HealthCheck>) {
        self.checks.push(check);
    }

    #[must_use]
    pub fn with_check(mut self, check: std::sync::Arc<dyn HealthCheck>) -> Self {
        self.register(check);
        self
    }

    pub fn len(&self) -> usize {
        self.checks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.checks.is_empty()
    }

    pub async fn run_all(&self) -> ReadyReport {
        use futures::future;

        let results = future::join_all(self.checks.iter().map(|check| {

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
