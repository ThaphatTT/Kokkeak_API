//! Kokkeak API entry point (binary).
//!
//! Composition root: builds the JSON-DB repositories, auth services,
//! app state, and the axum router. Wires health checks for
//! Redis / NATS / Mongo when those URLs are configured (T07–T09).
//!
//! ## Status (M9 complete)
//!
//! - T01-T05 (M0): healthz / readyz / metrics / trace / graceful shutdown
//! - M1: Redis / NATS / Mongo health + cache/queue ports
//! - M1.5: JSON-DB simulation layer, single-flight, settings for
//!   data_dir + auth
//! - M2: Auth & RBAC (register / login / refresh / logout / me)
//! - M3: Catalog (services) + Order (me / assigned) skeleton
//! - M4: NATS worker with idempotent handlers
//! - M5: real SQL Server repositories (tiberius)
//! - M6: matching + dispatch
//! - M7: i18n (th / en / lo)
//! - M8: chat (REST + WebSocket) + Redis pub/sub backplane + S3
//! - M9: payment + commission + payout + admin RBAC

use std::sync::Arc;

use axum::{
    body::Body,
    http::{header::CONTENT_TYPE, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use kokkak_common::{config::Settings, telemetry};
use kokkak_domain::HealthRegistry;
use kokkak_infra::auth::jwt::JwtService;
use kokkak_infra::cache::redis::RedisCache;
use kokkak_infra::db::json_catalog::JsonServiceRepository;
use kokkak_infra::db::json_chat::JsonChatRepository;
use kokkak_infra::db::json_order::JsonOrderRepository;
use kokkak_infra::db::json_payment::JsonPaymentRepository;
use kokkak_infra::db::json_user::JsonUserRepository;
use kokkak_infra::db::mongo::MongoClient;
use kokkak_infra::queue::nats::NatsQueue;

use kokkak_api::build_app_state;
use kokkak_api::build_router;

/// T03: serve Prometheus text-format metrics.
async fn metrics_handler() -> impl IntoResponse {
    let body = telemetry::render_metrics();
    Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, "text/plain; version=0.0.4; charset=utf-8")
        .body(Body::from(body))
        .expect("failed to build metrics response")
}

#[tokio::main]
async fn main() {
    // ---- T02: load & validate configuration ----
    let settings = Settings::load().unwrap_or_else(|err| {
        eprintln!("[kokkak-api] invalid configuration: {err}");
        eprintln!("[kokkak-api] see .env.example for required variables");
        std::process::exit(1);
    });

    // ---- T03: init tracing (JSON or pretty) + Prometheus metrics ----
    telemetry::init_tracing(settings.log.format);
    let _metrics_handle = Arc::new(telemetry::init_metrics());

    tracing::info!(
        addr = %settings.server.addr,
        workers = settings.server.workers,
        log_format = ?settings.log.format,
        sqlserver_configured = settings.database.is_configured(),
        redis_configured = settings.redis.is_configured(),
        nats_configured = settings.nats.is_configured(),
        mongo_configured = settings.mongo.is_configured(),
        data_dir = %settings.data_dir.path,
        auth_configured = settings.auth.is_configured(),
        "kokkak-api starting"
    );

    // ---- M1.5: ensure data dir exists ----
    let data_dir = std::path::PathBuf::from(&settings.data_dir.path);
    if let Err(e) = tokio::fs::create_dir_all(&data_dir).await {
        eprintln!(
            "[kokkak-api] failed to create data dir {}: {e}",
            data_dir.display()
        );
        std::process::exit(1);
    }
    if settings.data_dir.reset_on_startup {
        if let Err(e) = tokio::fs::remove_dir_all(&data_dir).await {
            tracing::warn!(error = %e, "failed to reset data dir");
        }
        let _ = tokio::fs::create_dir_all(&data_dir).await;
    }

    // ---- M2: build auth + JWT services ----
    let jwt = Arc::new(JwtService::new(&settings.auth).unwrap_or_else(|e| {
        eprintln!("[kokkak-api] invalid auth settings: {e}");
        eprintln!("[kokkak-api] set KOKKAK_AUTH__JWT_SECRET in .env");
        std::process::exit(1);
    }));

    // ---- M1.5: build JSON-DB repositories ----
    let user_repo = JsonUserRepository::open(data_dir.join("users.json"))
        .await
        .unwrap_or_else(|e| {
            eprintln!("[kokkak-api] failed to open user repo: {e}");
            std::process::exit(1);
        });
    let service_repo = JsonServiceRepository::open(data_dir.join("services.json"))
        .await
        .unwrap_or_else(|e| {
            eprintln!("[kokkak-api] failed to open service repo: {e}");
            std::process::exit(1);
        });
    let order_repo = JsonOrderRepository::open(data_dir.join("orders.json"))
        .await
        .unwrap_or_else(|e| {
            eprintln!("[kokkak-api] failed to open order repo: {e}");
            std::process::exit(1);
        });
    // M8: chat uses a JSON file (or, when Mongo is configured
    // and a `chat` sub-module is plugged in, the MongoDB
    // adapter). For M8 we always use the JSON sim.
    let chat_repo = JsonChatRepository::open(data_dir.join("chat.json"))
        .await
        .unwrap_or_else(|e| {
            eprintln!("[kokkak-api] failed to open chat repo: {e}");
            std::process::exit(1);
        });
    // M9: payment / commission / payout JSON sim.
    let payment_repo = JsonPaymentRepository::open(data_dir.join("payments.json"))
        .await
        .unwrap_or_else(|e| {
            eprintln!("[kokkak-api] failed to open payment repo: {e}");
            std::process::exit(1);
        });

    // ---- M2: build use cases ----
    let user_repo_arc: Arc<dyn kokkak_domain::UserRepository> = Arc::new(user_repo);
    let service_repo_arc: Arc<dyn kokkak_domain::ServiceRepository> = Arc::new(service_repo);
    let order_repo_arc: Arc<dyn kokkak_domain::OrderRepository> = Arc::new(order_repo);
    let chat_repo_arc: Arc<dyn kokkak_domain::ChatRepository> = Arc::new(chat_repo);
    let payment_repo_arc: Arc<dyn kokkak_domain::PaymentRepository> = Arc::new(payment_repo);

    // ---- T05 + M1: build readiness registry ----
    let registry = build_health_registry(&settings).await;

    // ---- Build app state ----
    let state = build_app_state(
        user_repo_arc,
        service_repo_arc,
        order_repo_arc,
        chat_repo_arc,
        payment_repo_arc,
        jwt,
        registry,
    );

    // ---- Routes ----
    let app = build_router(state)
        .route("/metrics", get(metrics_handler))
        .layer(axum::middleware::from_fn(
            kokkak_api::middleware::trace::trace_request,
        ));

    // ---- Bind + serve with graceful shutdown ----
    let listener = tokio::net::TcpListener::bind(&settings.server.addr)
        .await
        .unwrap_or_else(|err| {
            eprintln!(
                "[kokkak-api] failed to bind {}: {err}",
                settings.server.addr
            );
            std::process::exit(1);
        });

    tracing::info!(addr = %settings.server.addr, "kokkak-api listening");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .expect("server error");

    tracing::info!("kokkak-api exited cleanly");
}

async fn build_health_registry(settings: &Settings) -> HealthRegistry {
    let mut registry = HealthRegistry::new();

    if settings.redis.is_configured() {
        match RedisCache::new(&settings.redis) {
            Ok(cache) => {
                registry.register(Arc::new(
                    kokkak_infra::health::redis::RedisHealthCheck::new(Arc::new(cache)),
                ));
            }
            Err(err) => {
                tracing::warn!(error = %err, "redis configured but pool build failed");
            }
        }
    } else {
        tracing::info!("redis not configured — /readyz will skip it");
    }

    if settings.nats.is_configured() {
        match NatsQueue::connect(&settings.nats).await {
            Ok(queue) => {
                registry.register(Arc::new(kokkak_infra::health::nats::NatsHealthCheck::new(
                    Arc::new(queue),
                )));
            }
            Err(err) => {
                tracing::warn!(error = %err, "nats configured but connect failed");
            }
        }
    } else {
        tracing::info!("nats not configured — /readyz will skip it");
    }

    if settings.mongo.is_configured() {
        match MongoClient::connect(&settings.mongo).await {
            Ok(client) => {
                registry.register(Arc::new(
                    kokkak_infra::health::mongo::MongoHealthCheck::new(Arc::new(client)),
                ));
            }
            Err(err) => {
                tracing::warn!(error = %err, "mongo configured but connect failed");
            }
        }
    } else {
        tracing::info!("mongo not configured — /readyz will skip it");
    }

    if settings.database.is_configured() {
        tracing::warn!(
            "KOKKAK_DATABASE__SQLSERVER_URL is set but the tiberius client is deferred to M1.5+ — \
             /readyz will NOT report SQL Server. See crates/infra/src/db/mssql.rs."
        );
    }

    registry
}

/// Resolves on the first of: SIGINT (Ctrl-C), SIGTERM (Unix only).
async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(err) = tokio::signal::ctrl_c().await {
            tracing::error!(error = %err, "failed to install Ctrl-C handler");
        }
        tracing::info!("SIGINT received, starting graceful shutdown");
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut sig) => {
                sig.recv().await;
                tracing::info!("SIGTERM received, starting graceful shutdown");
            }
            Err(err) => {
                tracing::error!(error = %err, "failed to install SIGTERM handler");
                std::future::pending::<()>().await;
            }
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {}
        _ = terminate => {}
    }
}
