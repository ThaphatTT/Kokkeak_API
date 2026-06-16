//! Kokkeak Worker entry point (M4).
//!
//! Connects to NATS JetStream, subscribes to the subjects defined
//! in AGENTS.md § 10, and dispatches messages to the registered
//! handlers (all idempotent).
//!
//! Run with: `cargo run --bin kokkak-worker`
//!
//! Required env:
//! - KOKKAK_NATS__URL (`nats://host:4222`)
//! - KOKKAK_REDIS__URL (optional — enables Redis-backed idempotency)

use std::sync::Arc;

use kokkak_common::{config::Settings, telemetry};
use kokkak_domain::ChatRepository;
use kokkak_infra::cache::redis::RedisCache;
use kokkak_infra::db::json_chat::JsonChatRepository;
use tokio::sync::watch;
use tracing::info;

use kokkak_worker::handlers::{
    ChatPersistHandler, CommEmailHandler, HandlerContext, NotiPushHandler, OrderDispatchHandler,
    PointsRecalcHandler,
};
use kokkak_worker::{Idempotency, InMemoryIdempotency, RedisIdempotency, Worker, WorkerConfig};

#[tokio::main]
async fn main() {
    let settings = Settings::load().unwrap_or_else(|err| {
        eprintln!("[kokkak-worker] invalid configuration: {err}");
        std::process::exit(1);
    });

    telemetry::init_tracing(settings.log.format);
    let _ = telemetry::init_metrics();

    info!(
        nats = %settings.nats.url,
        redis = %settings.redis.url,
        log_format = ?settings.log.format,
        "kokkak-worker starting"
    );

    if !settings.nats.is_configured() {
        eprintln!("[kokkak-worker] KOKKAK_NATS__URL is required");
        std::process::exit(1);
    }

    // Build the idempotency cache.
    let idempotency: Arc<dyn Idempotency> = match RedisCache::new(&settings.redis) {
        Ok(cache) => {
            info!("using Redis-backed idempotency cache");
            Arc::new(RedisIdempotency::new(cache.pool()))
        }
        Err(_) => {
            info!("redis not configured — using in-memory idempotency cache");
            Arc::new(InMemoryIdempotency::new(10_000))
        }
    };

    // Build the handler context.
    let ctx = HandlerContext { idempotency };

    // M8: open the chat repository (JSON sim) and hand it to
    // the `chat.persist` handler.
    let data_dir = std::path::PathBuf::from(&settings.data_dir.path);
    if let Err(e) = tokio::fs::create_dir_all(&data_dir).await {
        eprintln!("[kokkak-worker] failed to create data dir: {e}");
        std::process::exit(1);
    }
    match JsonChatRepository::open(data_dir.join("chat.json")).await {
        Ok(repo) => {
            let repo: Arc<dyn ChatRepository> = Arc::new(repo);
            kokkak_worker::handlers::chat_persist::set_chat_repo(repo);
            info!("chat.persist handler wired to JSON-DB sim");
        }
        Err(e) => {
            eprintln!("[kokkak-worker] failed to open chat.json: {e}");
            std::process::exit(1);
        }
    }

    // Register every handler.
    let handlers: Vec<Arc<dyn kokkak_worker::handlers::Handler>> = vec![
        Arc::new(NotiPushHandler::new(ctx.clone())),
        Arc::new(CommEmailHandler::new(ctx.clone())),
        Arc::new(ChatPersistHandler::new(ctx.clone())),
        Arc::new(OrderDispatchHandler::new(ctx.clone())),
        Arc::new(PointsRecalcHandler::new(ctx)),
    ];

    // Connect NATS + assemble worker.
    let queue = Arc::new(
        kokkak_infra::queue::nats::NatsQueue::connect(&settings.nats)
            .await
            .unwrap_or_else(|e| {
                eprintln!("[kokkak-worker] nats connect failed: {e}");
                std::process::exit(1);
            }),
    );
    let worker = Worker::with_in_memory_idempotency(WorkerConfig::default(), queue, handlers);

    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    // Graceful shutdown on SIGINT / SIGTERM.
    tokio::spawn(async move {
        let ctrl_c = tokio::signal::ctrl_c();
        #[cfg(unix)]
        let terminate = async {
            let mut sig = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("install SIGTERM handler");
            sig.recv().await;
        };
        #[cfg(not(unix))]
        let terminate = std::future::pending::<()>();
        tokio::select! {
            _ = ctrl_c => info!("SIGINT received"),
            _ = terminate => info!("SIGTERM received"),
        }
        let _ = shutdown_tx.send(true);
    });

    if let Err(e) = worker.run(shutdown_rx).await {
        eprintln!("[kokkak-worker] fatal: {e}");
        std::process::exit(1);
    }

    info!("kokkak-worker exited cleanly");
}
