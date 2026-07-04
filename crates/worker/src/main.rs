

use std::sync::Arc;

use kokkak_common::{config::Settings, telemetry};
use kokkak_domain::ChatRepository;
use kokkak_infra::cache::redis::RedisCache;
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

    if telemetry::init_otel("kokkak-worker", None) {
        info!("OTel exporter wired (traces + metrics OTLP)");
    }

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

    let ctx = HandlerContext { idempotency };

    let mssql_url = std::env::var("KOKKAK_DATABASE__SQLSERVER_URL").unwrap_or_default();
    if mssql_url.is_empty() || mssql_url == "disabled" {
        eprintln!(
            "[kokkak-worker] KOKKAK_DATABASE__SQLSERVER_URL not set — chat.persist cannot start"
        );
        std::process::exit(1);
    }
    let topo_settings = kokkak_common::config::DatabaseTopologySettings::from_settings(&settings);
    let topo = kokkak_infra::db::topology::DatabaseTopology::build(&topo_settings, false)
        .await
        .expect("worker MSSQL topology build");
    let primary_pool = topo.get(topo.primary_role().expect("primary")).clone();
    let chat_repo: Arc<dyn ChatRepository> = Arc::new(
        kokkak_infra::db::mssql_chat::MssqlChatRepository::new(primary_pool),
    );
    kokkak_worker::handlers::chat_persist::set_chat_repo(chat_repo);
    info!("chat.persist handler wired to MSSQL");

    let handlers: Vec<Arc<dyn kokkak_worker::handlers::Handler>> = vec![
        Arc::new(NotiPushHandler::new(ctx.clone())),
        Arc::new(CommEmailHandler::new(ctx.clone())),
        Arc::new(ChatPersistHandler::new(ctx.clone())),
        Arc::new(OrderDispatchHandler::new(ctx.clone())),
        Arc::new(PointsRecalcHandler::new(ctx)),
    ];

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
