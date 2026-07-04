

use std::sync::Arc;

use async_trait::async_trait;
use kokkak_domain::{HealthCheck, HealthError};

use crate::queue::nats::NatsQueue;

pub struct NatsHealthCheck {
    queue: Arc<NatsQueue>,
}

impl NatsHealthCheck {

    pub fn new(queue: Arc<NatsQueue>) -> Self {
        Self { queue }
    }
}

#[async_trait]
impl HealthCheck for NatsHealthCheck {
    fn name(&self) -> &str {
        "nats"
    }

    async fn check(&self) -> Result<(), HealthError> {
        crate::queue::nats::ping(&self.queue)
            .await
            .map_err(|e| HealthError::Failed(e.to_string()))
    }
}
