use std::sync::Arc;
use std::time::Duration;

use async_nats::jetstream::consumer::{pull, AckPolicy};
use futures::StreamExt;
use kokkak_common::config::{LogFormat, NatsSettings};
use kokkak_domain::QueuePort;
use kokkak_infra::queue::nats::NatsQueue;
use thiserror::Error;
use tracing::{error, info, warn};

use crate::handlers::{Handler, HandlerError};
use crate::idempotency::{Idempotency, IdempotencyKey, InMemoryIdempotency};
use crate::retry::RetryPolicy;

#[derive(Debug, Error)]
pub enum WorkerError {
    #[error("nats connect failed: {0}")]
    Nats(String),

    #[error("invalid worker config: {0}")]
    Config(String),
}

const X_RETRY_COUNT_HEADER: &str = "x-retry-count";

#[derive(Debug, Clone)]
pub struct WorkerConfig {
    pub subjects: Vec<String>,

    pub stream_name: String,

    pub idempotency_ttl: Duration,

    pub pull_max_messages: usize,

    pub pull_expires_in: Duration,

    pub retry_policy: RetryPolicy,

    pub dlq_subject: String,
}

impl Default for WorkerConfig {
    fn default() -> Self {
        Self {
            subjects: vec![
                "noti.push".into(),
                "comm.email".into(),
                "comm.sms".into(),
                "chat.persist".into(),
                "order.dispatch".into(),
                "points.recalc".into(),
                "report.generate".into(),
                "media.process".into(),
                "audit.write".into(),
            ],
            stream_name: "kokkak.events".into(),
            idempotency_ttl: Duration::from_secs(24 * 3600),
            pull_max_messages: 100,
            pull_expires_in: Duration::from_secs(30),
            retry_policy: RetryPolicy::default(),
            dlq_subject: "kokkak.dlq".into(),
        }
    }
}

#[derive(Clone)]
pub struct Worker {
    config: WorkerConfig,
    handlers: Vec<Arc<dyn Handler>>,
    idempotency: Arc<dyn Idempotency>,
    queue: Arc<NatsQueue>,
}

impl Worker {
    pub fn new(
        config: WorkerConfig,
        queue: Arc<NatsQueue>,
        handlers: Vec<Arc<dyn Handler>>,
        idempotency: Arc<dyn Idempotency>,
    ) -> Self {
        Self {
            config,
            handlers,
            idempotency,
            queue,
        }
    }

    pub fn with_in_memory_idempotency(
        config: WorkerConfig,
        queue: Arc<NatsQueue>,
        handlers: Vec<Arc<dyn Handler>>,
    ) -> Self {
        let idempotency: Arc<dyn Idempotency> = Arc::new(InMemoryIdempotency::new(10_000));
        Self::new(config, queue, handlers, idempotency)
    }

    pub async fn ensure_topology(&self) -> Result<(), WorkerError> {
        let mut subjects: Vec<String> = self.config.subjects.clone();
        if !subjects.contains(&self.config.dlq_subject) {
            subjects.push(self.config.dlq_subject.clone());
        }
        let subject_refs: Vec<&str> = subjects.iter().map(|s| s.as_str()).collect();
        self.queue
            .ensure_stream(&self.config.stream_name, &subject_refs)
            .await
            .map_err(|e| WorkerError::Nats(e.to_string()))
    }

    pub async fn run(
        self,
        mut shutdown_rx: tokio::sync::watch::Receiver<bool>,
    ) -> Result<(), WorkerError> {
        self.ensure_topology().await?;
        info!(
            subjects = ?self.config.subjects,
            stream = %self.config.stream_name,
            "worker starting consumer loop"
        );

        let jet = self.queue.jetstream().clone();
        let cfg = self.config.clone();
        let idempotency = self.idempotency.clone();

        let subject_to_handler: Arc<std::collections::HashMap<String, Arc<dyn Handler>>> = {
            let mut m = std::collections::HashMap::new();
            for h in &self.handlers {
                m.insert(h.subject().to_string(), h.clone());
            }
            Arc::new(m)
        };

        let stream = jet
            .get_or_create_stream(async_nats::jetstream::stream::Config {
                name: cfg.stream_name.clone(),
                subjects: cfg.subjects.clone(),
                ..Default::default()
            })
            .await
            .map_err(|e| WorkerError::Nats(e.to_string()))?;

        let mut tasks = Vec::new();
        for subject in cfg.subjects.iter() {
            let handler = match subject_to_handler.get(subject) {
                Some(h) => h.clone(),
                None => continue,
            };
            let stream = stream.clone();
            let idempotency = idempotency.clone();
            let queue = self.queue.clone();
            let subject = subject.clone();
            let cfg = cfg.clone();
            let mut shutdown_rx = shutdown_rx.clone();
            tasks.push(tokio::spawn(async move {
                let durable_name = format!("kokkak-{}", subject.replace('.', "-"));
                let pull_cfg = pull::Config {
                    durable_name: Some(durable_name.clone()),
                    name: Some(durable_name),
                    filter_subject: subject.clone(),
                    ack_policy: AckPolicy::Explicit,
                    ..Default::default()
                };
                let pull = match stream.get_or_create_consumer(&subject, pull_cfg).await {
                    Ok(p) => p,
                    Err(e) => {
                        warn!(subject = %subject, error = %e, "create consumer failed; will skip");
                        return;
                    }
                };
                loop {
                    if *shutdown_rx.borrow() {
                        info!(subject = %subject, "consumer stopping");
                        return;
                    }
                    let fetch_result = tokio::select! {
                        _ = shutdown_rx.changed() => {
                            info!(subject = %subject, "consumer stopping (signal)");
                            return;
                        }
                        res = pull.fetch().max_messages(cfg.pull_max_messages).expires(cfg.pull_expires_in).messages() => {
                            res
                        }
                    };
                    let mut messages = match fetch_result {
                        Ok(m) => m,
                        Err(e) => {
                            warn!(subject = %subject, error = %e, "fetch failed");
                            tokio::time::sleep(Duration::from_secs(1)).await;
                            continue;
                        }
                    };
                    while let Some(msg_result) = messages.next().await {
                        let msg = match msg_result {
                            Ok(m) => m,
                            Err(e) => {
                                warn!(subject = %subject, error = %e, "stream error");
                                continue;
                            }
                        };

                        let message_id = msg
                            .message
                            .headers
                            .as_ref()
                            .and_then(|h| h.get(async_nats::header::NATS_MESSAGE_ID).map(|v| v.to_string()))
                            .unwrap_or_else(|| {
                                msg.message
                                    .headers
                                    .as_ref()
                                    .and_then(|h| h.get(async_nats::header::NATS_SEQUENCE).map(|v| v.to_string()))
                                    .unwrap_or_else(|| uuid::Uuid::new_v4().to_string())
                            });
                        let payload = msg.message.payload.clone();
                        let key = IdempotencyKey::new(subject.clone(), message_id.clone());
                        match idempotency.claim(&key, cfg.idempotency_ttl).await {
                            Ok(true) => {  }
                            Ok(false) => {
                                info!(subject = %subject, message_id = %message_id, "skip duplicate");
                                let _ = msg.ack().await;
                                continue;
                            }
                            Err(e) => {
                                warn!(subject = %subject, error = %e, "idempotency check failed; will process");
                            }
                        }
                        let retry_count = msg
                            .message
                            .headers
                            .as_ref()
                            .and_then(|h| h.get(X_RETRY_COUNT_HEADER))
                            .and_then(|v| v.as_str().parse::<u32>().ok())
                            .unwrap_or(0);

                        match handler.handle(&message_id, &payload).await {
                            Ok(()) => {
                                if let Err(e) = msg.ack().await {
                                    warn!(subject = %subject, error = %e, "ack failed");
                                }
                            }
                            Err(HandlerError::Failed(reason)) => {
                                if cfg.retry_policy.should_retry(retry_count) {
                                    let delay = cfg.retry_policy.delay_for(retry_count);
                                    warn!(
                                        subject = %subject,
                                        message_id = %message_id,
                                        attempt = retry_count + 1,
                                        max_retries = cfg.retry_policy.max_retries,
                                        delay_ms = delay.as_millis() as u64,
                                        %reason,
                                        "handler failed; will retry"
                                    );
                                    if let Err(e) = msg.ack_with(async_nats::jetstream::AckKind::Nak(Some(delay))).await {
                                        warn!(subject = %subject, error = %e, "nak failed");
                                    }
                                } else {
                                    error!(
                                        subject = %subject,
                                        message_id = %message_id,
                                        retries = retry_count,
                                        %reason,
                                        "handler failed after max retries; sending to DLQ"
                                    );
                                    let dlq_payload = serde_json::json!({
                                        "original_subject": subject,
                                        "message_id": message_id,
                                        "retry_count": retry_count,
                                        "last_error": reason,
                                        "payload": String::from_utf8_lossy(&payload),
                                    });
                                    if let Ok(dlq_bytes) = serde_json::to_vec(&dlq_payload) {
                                        if let Err(e) = queue.publish(&cfg.dlq_subject, &dlq_bytes).await {
                                            error!(error = %e, "DLQ publish failed");
                                        }
                                    }
                                    if let Err(e) = msg.ack_with(async_nats::jetstream::AckKind::Term).await {
                                        warn!(subject = %subject, error = %e, "term ack failed");
                                    }
                                }
                            }
                        }
                    }
                }
            }));
        }

        let _ = shutdown_rx.changed().await;
        info!("worker received shutdown signal, stopping consumers");
        for t in tasks {
            let _ = t.await;
        }
        Ok(())
    }
}

pub async fn from_settings(
    settings: &NatsSettings,
    log_format: LogFormat,
    handlers: Vec<Arc<dyn Handler>>,
) -> Result<Worker, WorkerError> {
    if !settings.is_configured() {
        return Err(WorkerError::Config(
            "nats not configured (set KOKKAK_NATS__URL)".into(),
        ));
    }
    let queue = NatsQueue::connect(settings)
        .await
        .map_err(|e| WorkerError::Nats(e.to_string()))?;
    let _ = log_format;
    Ok(Worker::with_in_memory_idempotency(
        WorkerConfig::default(),
        Arc::new(queue),
        handlers,
    ))
}
