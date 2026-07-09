use std::sync::Arc;
use std::time::Duration;

use axum::{
    body::Body,
    extract::Extension,
    http::{header::CONTENT_TYPE, HeaderValue, Method, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
};
use deadpool_redis::{Config, Pool, Runtime};
use kokkak_common::{
    config::{RateLimitBackend, Settings},
    i18n, telemetry,
};
use kokkak_domain::HealthRegistry;
use kokkak_infra::auth::jwt::JwtService;
use kokkak_infra::cache::redis::RedisCache;
use kokkak_infra::db::mongo::MongoClient;
use kokkak_infra::queue::nats::NatsQueue;
use tower_governor::key_extractor::PeerIpKeyExtractor;
use tower_governor::{governor::GovernorConfigBuilder, GovernorLayer};
use tower_http::compression::CompressionLayer;
use tower_http::cors::CorsLayer;
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::timeout::TimeoutLayer;

use kokkak_api::build_app_state_with;
use kokkak_api::build_repos;
use kokkak_api::build_router;
use kokkak_api::middleware::rate_limit_redis::{rate_limit_redis_middleware, RedisRateLimit};
use kokkak_api::middleware::safety::ConcurrencyCap;
use kokkak_api::redirect::redirect_router;
use kokkak_api::tls::{build_rustls_config, hsts_layer};
use kokkak_infra::storage::build_from_settings as build_storage;

async fn metrics_handler() -> impl IntoResponse {
    let body = telemetry::render_metrics();
    Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, "text/plain; version=0.0.4; charset=utf-8")
        .body(Body::from(body))
        .expect("failed to build metrics response")
}

fn main() {
    let _ = rustls::crypto::ring::default_provider().install_default();

    let env_file = std::env::var("KOKKAK_ENV_FILE")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| match std::env::var("KOKKAK_ENVIRONMENT").as_deref() {
            Ok("production") => ".env.production".to_string(),
            _ => ".env.dev".to_string(),
        });
    let _ = dotenvy::from_filename(&env_file);

    let settings = Settings::load().unwrap_or_else(|err| {
        eprintln!("[kokkak-api] invalid configuration: {err}");
        eprintln!("[kokkak-api] see .env.example for required variables");
        std::process::exit(1);
    });

    let worker_threads = settings.server.workers.max(1);
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(worker_threads)
        .enable_all()
        .thread_name("kokkak-api")
        .build()
        .expect("failed to build tokio runtime");

    runtime.block_on(run(settings));
}

async fn run(settings: Settings) {
    i18n::init_i18n("en");

    telemetry::init_tracing(settings.log.format);
    let _metrics_handle = Arc::new(telemetry::init_metrics());

    if telemetry::init_otel("kokkak-api", None) {
        tracing::info!("OTel exporter wired (traces + metrics OTLP)");
    }

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

    let jwt = Arc::new(JwtService::new(&settings.auth).unwrap_or_else(|e| {
        eprintln!("[kokkak-api] invalid auth settings: {e}");
        eprintln!("[kokkak-api] set KOKKAK_AUTH__JWT_SECRET in .env");
        std::process::exit(1);
    }));

    let bundle = build_repos(&data_dir, &settings).await.unwrap_or_else(|e| {
        eprintln!("[kokkak-api] failed to build repo bundle: {e}");
        std::process::exit(1);
    });
    tracing::info!(
        backend = bundle.backend.as_str(),
        "kokkak-api: repository bundle ready"
    );

    let _ = (bundle.backend, bundle.mssql_pool.is_some());

    let mut registry = build_health_registry(&settings).await;

    if let Some(topo) = &bundle.topology {
        let topo_arc = Arc::new(topo.clone());
        registry.register(Arc::new(
            kokkak_infra::health::sqlserver::MultiDbHealthCheck::new(topo_arc),
        ));
        tracing::info!(
            roles = ?topo.live_roles(),
            "sqlserver multi-DB health check registered"
        );
    }

    let (storage, storage_kind) = match build_storage(&settings.storage).await {
        Ok(pair) => pair,
        Err(e) => {
            eprintln!("[kokkak-api] failed to build storage adapter: {e}");
            std::process::exit(1);
        }
    };
    tracing::info!(
        adapter = storage_kind.as_str(),
        "object-storage adapter ready"
    );

    let public_base_url: Arc<str> = Arc::from(settings.server.public_base_url.as_str());

    let signed_url_secret: Arc<str> = Arc::from(settings.storage.signed_url_secret.as_str());
    let signed_url_ttl_secs: u32 = settings.storage.signed_url_ttl_secs;
    tracing::info!(
        signed_url_ttl_secs = signed_url_ttl_secs,
        signed_url_secret_len = signed_url_secret.len(),
        "signed-url knobs loaded"
    );

    let settings_arc = Arc::new(settings.clone());

    let session_redis_pool = if settings.redis.is_configured() {
        let cfg = deadpool_redis::Config::from_url(&settings.redis.url);
        match cfg.create_pool(Some(deadpool_redis::Runtime::Tokio1)) {
            Ok(pool) => {
                tracing::info!("session store: Redis-backed");
                Some(pool)
            }
            Err(e) => {
                tracing::warn!(error = %e, "session store: Redis pool build failed — falling back to no-op");
                None
            }
        }
    } else {
        tracing::info!("session store: no-op (KOKKAK_REDIS__URL not set)");
        None
    };

    let state = build_app_state_with(
        bundle,
        jwt,
        registry,
        settings_arc.clone(),
        storage,
        public_base_url.clone(),
        signed_url_secret.clone(),
        signed_url_ttl_secs,
        session_redis_pool,
    );

    let mut app = build_router(state.clone()).route("/metrics", get(metrics_handler));

    let files_route = axum::Router::new()
        .route("/files/*path", get(kokkak_api::files::files_handler))
        .with_state(state.clone());
    app = app.merge(files_route);
    tracing::info!(
        mount = "/files/*",
        "signed-URL route mounted (HMAC-SHA256, time-limited)"
    );
    #[allow(unused_imports)]
    {}

    let cors = build_cors_layer(&settings.middleware.cors_allow_origins);
    let app = match cors {
        Some(layer) => {
            tracing::info!(
                origins = ?settings.middleware.cors_allow_origins,
                "CORS layer wired"
            );
            app.layer(layer)
        }
        None => {
            tracing::info!("CORS allowlist empty — cross-origin requests denied");
            app
        }
    };

    let app = if settings.middleware.rate_limit.enabled {
        let burst = settings.middleware.rate_limit.burst_size;
        match settings.middleware.rate_limit.backend {
            RateLimitBackend::Memory => {
                let rate = settings.middleware.rate_limit.requests_per_second;
                tracing::info!(
                    rps = rate,
                    burst = burst,
                    "rate limit enabled (per-IP, GCRA, in-memory)"
                );
                let governor_conf = std::sync::Arc::new(
                    GovernorConfigBuilder::default()
                        .per_second(u64::from(rate))
                        .burst_size(burst)
                        .key_extractor(PeerIpKeyExtractor)
                        .finish()
                        .expect("rate-limit config must build (knobs validated upstream)"),
                );

                let limiter = governor_conf.limiter().clone();
                tokio::spawn(async move {
                    loop {
                        tokio::time::sleep(std::time::Duration::from_secs(300)).await;
                        let before = limiter.len();
                        limiter.retain_recent();
                        let after = limiter.len();
                        if before != after {
                            tracing::debug!(before, after, "rate limiter GC swept stale entries");
                        }
                    }
                });
                app.layer(GovernorLayer {
                    config: governor_conf,
                })
            }
            RateLimitBackend::Redis => {
                const REDIS_WINDOW_SECS: u64 = 1;
                let pool = build_rate_limit_pool(&settings).unwrap_or_else(|err| {
                    eprintln!("[kokkak-api] rate limit redis pool build failed: {err}");
                    std::process::exit(1);
                });
                let limiter = RedisRateLimit::new(pool, u64::from(burst), REDIS_WINDOW_SECS);
                tracing::info!(
                    burst,
                    window_secs = REDIS_WINDOW_SECS,
                    "rate limit enabled (per-IP, fixed window, Redis-backed)"
                );
                app.layer(axum::middleware::from_fn_with_state(
                    limiter,
                    rate_limit_redis_middleware,
                ))
            }
        }
    } else {
        tracing::info!("rate limit disabled");
        app
    };

    let app = if settings.middleware.compression_enabled {
        tracing::info!("response compression enabled (gzip/deflate/br)");
        app.layer(CompressionLayer::new())
    } else {
        tracing::info!("response compression disabled");
        app
    };

    if settings.middleware.request_timeout_secs > 0 {
        tracing::info!(
            secs = settings.middleware.request_timeout_secs,
            "request timeout wired"
        );
    } else {
        tracing::warn!(
            "request timeout DISABLED — slow handlers will tie up tokio workers indefinitely"
        );
    }

    #[allow(deprecated)]
    let app = if settings.middleware.request_timeout_secs > 0 {
        app.layer(TimeoutLayer::new(Duration::from_secs(
            settings.middleware.request_timeout_secs,
        )))
    } else {
        app
    };

    let app = app.layer(axum::middleware::from_fn(
        kokkak_api::middleware::trace::trace_request,
    ));

    let app = if settings.middleware.idempotency.enabled {
        let max_entries = settings.middleware.idempotency.max_entries;
        let ttl = Duration::from_secs(settings.middleware.idempotency.ttl_secs);
        let store: std::sync::Arc<dyn kokkak_domain::IdempotencyStore> = std::sync::Arc::new(
            kokkak_infra::idempotency::InMemoryIdempotencyStore::new(max_entries),
        );
        tracing::info!(
            max_entries,
            ttl_secs = ttl.as_secs(),
            "idempotency cache enabled (in-memory store)"
        );
        let store_for_layer = store.clone();
        app.layer(axum::middleware::from_fn(move |req, next| {
            let store = store_for_layer.clone();
            let ttl = ttl;
            async move { kokkak_api::middleware::idempotency::handle(req, next, store, ttl).await }
        }))
    } else {
        tracing::info!("idempotency cache disabled");
        app
    };

    let body_limit_bytes = settings.middleware.request_body_limit_bytes;
    let app = app.layer(RequestBodyLimitLayer::new(body_limit_bytes));
    tracing::info!(body_limit_bytes, "request body limit wired");

    let max_concurrency = settings.middleware.max_concurrency;
    let cap = ConcurrencyCap::new(max_concurrency);
    let app = app.layer(axum::middleware::from_fn_with_state(
        cap,
        kokkak_api::middleware::safety::concurrency_cap,
    ));
    tracing::info!(
        max_concurrency,
        "concurrency cap wired (sheds 503 when at capacity)"
    );

    let app = app.layer(Extension(settings_arc.clone()));
    tracing::info!("settings Extension wired (extractors can read feature flags)");

    if settings.tls.enabled {
        let cert_path = std::path::PathBuf::from(settings.tls.cert_path_or_empty());
        let key_path = std::path::PathBuf::from(settings.tls.key_path_or_empty());
        let tls_config = build_rustls_config(&cert_path, &key_path).unwrap_or_else(|err| {
            eprintln!("[kokkak-api] failed to build TLS config: {err:#}");
            std::process::exit(1);
        });

        let app = if let Some(layer) = hsts_layer(settings.tls.hsts_max_age_secs) {
            tracing::info!(
                max_age_secs = settings.tls.hsts_max_age_secs,
                "HSTS enabled"
            );
            app.layer(layer)
        } else {
            tracing::info!("HSTS disabled (max-age = 0)");
            app
        };

        if settings.tls.redirect_from_port > 0 {
            let redirect_addr = format!("0.0.0.0:{}", settings.tls.redirect_from_port);
            tokio::spawn(async move {
                match tokio::net::TcpListener::bind(&redirect_addr).await {
                    Ok(listener) => {
                        tracing::info!(addr = %redirect_addr, "HTTPS redirect listener up");
                        if let Err(err) = axum::serve(listener, redirect_router()).await {
                            tracing::error!(error = %err, "HTTPS redirect listener exited");
                        }
                    }
                    Err(err) => {
                        tracing::warn!(
                            addr = %redirect_addr,
                            error = %err,
                            "failed to bind HTTPS redirect listener; HTTPS-only deployment continues"
                        );
                    }
                }
            });
        }

        tracing::info!(
            addr = %settings.server.addr,
            cert = %cert_path.display(),
            "kokkak-api listening (HTTPS)"
        );

        let tls_handle = axum_server::Handle::new();
        tokio::spawn({
            let tls_handle = tls_handle.clone();
            async move {
                shutdown_signal().await;
                tls_handle.shutdown();
            }
        });

        if settings.tls.auto_reload {
            match kokkak_api::cert_watcher::watch_cert_files(&cert_path, &key_path) {
                Ok((watcher, mut rx)) => {
                    let initial_cert_fp = kokkak_api::cert_watcher::cert_fingerprint(&cert_path)
                        .unwrap_or_else(|_| "<unreadable>".into());
                    let initial_key_fp = kokkak_api::cert_watcher::cert_fingerprint(&key_path)
                        .unwrap_or_else(|_| "<unreadable>".into());
                    tracing::info!(
                        cert = %initial_cert_fp,
                        key = %initial_key_fp,
                        "cert auto-reload watcher active (LE 90-day rotation)"
                    );
                    let tls_handle_for_reload = tls_handle.clone();
                    tokio::spawn(async move {
                        let _watcher = watcher;

                        let _ = rx.borrow_and_update();
                        if rx.changed().await.is_err() {
                            return;
                        }

                        tokio::time::sleep(Duration::from_millis(500)).await;
                        let new_cert_fp = kokkak_api::cert_watcher::cert_fingerprint(&cert_path)
                            .unwrap_or_else(|_| "<unreadable>".into());
                        let new_key_fp = kokkak_api::cert_watcher::cert_fingerprint(&key_path)
                            .unwrap_or_else(|_| "<unreadable>".into());
                        tracing::info!(
                            old_cert = %initial_cert_fp,
                            new_cert = %new_cert_fp,
                            old_key = %initial_key_fp,
                            new_key = %new_key_fp,
                            "cert or key file changed — graceful shutdown for orchestrator restart"
                        );
                        tls_handle_for_reload.shutdown();
                    });
                }
                Err(err) => {
                    tracing::error!(
                        error = %err,
                        "cert watcher init failed; auto-reload disabled (service still serves with current cert)"
                    );
                }
            }
        }

        axum_server::bind_rustls(
            settings
                .server
                .addr
                .parse::<std::net::SocketAddr>()
                .unwrap_or_else(|err| {
                    eprintln!("[kokkak-api] invalid server.addr: {err}");
                    std::process::exit(1);
                }),
            tls_config,
        )
        .handle(tls_handle)
        .serve(app.into_make_service_with_connect_info::<std::net::SocketAddr>())
        .await
        .expect("TLS server error");
    } else {
        let listener = tokio::net::TcpListener::bind(&settings.server.addr)
            .await
            .unwrap_or_else(|err| {
                eprintln!(
                    "[kokkak-api] failed to bind {}: {err}",
                    settings.server.addr
                );
                std::process::exit(1);
            });

        tracing::info!(addr = %settings.server.addr, "kokkak-api listening (HTTP)");

        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .with_graceful_shutdown(shutdown_signal())
        .await
        .expect("server error");
    }

    tracing::info!("kokkak-api exited cleanly");
}

fn build_rate_limit_pool(settings: &Settings) -> Result<Pool, String> {
    if !settings.redis.is_configured() {
        return Err("KOKKAK_REDIS__URL is not set".to_string());
    }
    let cfg = Config::from_url(&settings.redis.url);
    cfg.create_pool(Some(Runtime::Tokio1))
        .map_err(|e| e.to_string())
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
        tracing::debug!(
            "sqlserver_url is set; /readyz will report sqlserver once the topology is built"
        );
    }

    registry
}

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

fn build_cors_layer(allow_origins: &[String]) -> Option<CorsLayer> {
    if allow_origins.is_empty() {
        return None;
    }
    let origins: Vec<HeaderValue> = allow_origins
        .iter()
        .filter_map(|o| match HeaderValue::from_str(o) {
            Ok(v) => Some(v),
            Err(err) => {
                eprintln!("[kokkak-api] invalid CORS origin {}: {err} — skipping", o);
                None
            }
        })
        .collect();
    if origins.is_empty() {
        return None;
    }
    Some(
        CorsLayer::new()
            .allow_origin(origins)
            .allow_methods([
                Method::GET,
                Method::POST,
                Method::PUT,
                Method::PATCH,
                Method::DELETE,
                Method::OPTIONS,
            ])
            .allow_headers([
                axum::http::header::CONTENT_TYPE,
                axum::http::header::AUTHORIZATION,
                axum::http::HeaderName::from_static("x-request-id"),
                axum::http::HeaderName::from_static("idempotency-key"),
            ])
            .allow_credentials(true),
    )
}
