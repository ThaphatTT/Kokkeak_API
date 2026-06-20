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
use std::time::Duration;

use axum::{
    body::Body,
    http::{header::CONTENT_TYPE, HeaderValue, Method, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
};
use kokkak_common::{config::Settings, i18n, telemetry};
use kokkak_domain::HealthRegistry;
use kokkak_infra::auth::jwt::JwtService;
use kokkak_infra::cache::redis::RedisCache;
use kokkak_infra::db::mongo::MongoClient;
use kokkak_infra::queue::nats::NatsQueue;
use tower_http::compression::CompressionLayer;
use tower_http::cors::CorsLayer;
use tower_http::timeout::TimeoutLayer;

use kokkak_api::build_app_state_with;
use kokkak_api::build_repos;
use kokkak_api::build_router;
use kokkak_api::redirect::redirect_router;
use kokkak_api::tls::{build_rustls_config, hsts_layer};

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
    // ---- T02: load .env (if present) into process env BEFORE
    //   Settings::load() — figment's Env provider only reads from
    //   std::env, not from disk. `dotenv()` is a no-op when no
    //   .env exists (production deploys inject env vars via
    //   docker/k8s/systemd instead). ----
    let _ = dotenvy::dotenv();

    // ---- T02: load & validate configuration ----
    let settings = Settings::load().unwrap_or_else(|err| {
        eprintln!("[kokkak-api] invalid configuration: {err}");
        eprintln!("[kokkak-api] see .env.example for required variables");
        std::process::exit(1);
    });

    // ---- M11: initialize the i18n catalog (th / en / lo). The
    //   default locale is the catalog fallback; per-request
    //   locale is set by the locale_middleware.
    i18n::init_i18n("en");
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

    // ---- M10: build the repository bundle (MSSQL or JSON) ----
    let bundle = build_repos(&data_dir, &settings).await.unwrap_or_else(|e| {
        eprintln!("[kokkak-api] failed to build repo bundle: {e}");
        std::process::exit(1);
    });
    tracing::info!(
        backend = bundle.backend.as_str(),
        "kokkak-api: repository bundle ready"
    );
    // Pin the bundle to silence the unused-warning if the
    // Mssql pool is dropped (we keep it alive for the
    // process lifetime via the `RepoBundle`).
    let _ = (bundle.backend, bundle.mssql_pool.is_some());

    // ---- T05 + M1: build readiness registry ----
    let mut registry = build_health_registry(&settings).await;

    // ---- M12: register the multi-DB SQL Server health check
    //   when the factory actually built a topology. The check
    //   pings every live role, so /readyz shows the failing
    //   role on a multi-DB outage. ----
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

    // ---- Build app state ----
    let state = build_app_state_with(bundle, jwt, registry);

    // ---- Routes ----
    let app = build_router(state).route("/metrics", get(metrics_handler));

    // ---- T-06: wire the middleware stack.
    //   Layer order (outermost first) is the REVERSE of the
    //   apply order — `.layer(X)` wraps the service in X, so
    //   the LAST `.layer` call ends up CLOSEST to the handler.
    //
    //   Final request flow: trace → timeout → compression → cors
    //   → locale_middleware (inside router) → handler.
    //
    //   - trace_request is OUTERMOST so request-start logs and
    //     metrics fire before any short-circuit (CORS preflight,
    //     timeout, etc.).
    //   - cors is INNERMOST so preflight OPTIONS requests
    //     short-circuit at the CORS layer without paying for
    //     compression / timeout machinery on what is effectively
    //     a metadata exchange.
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
    // T-06: TimeoutLayer::new is deprecated in tower-http 0.6 in
    // favour of `with_status_code(408)`. The current API returns
    // HTTP 500 on timeout, which is acceptable for now — the
    // client-visible behaviour matters more than the exact code.
    // Upgrade path: swap to `with_status_code(StatusCode::REQUEST_TIMEOUT)`
    // once we commit to standardising 408 across all routes.
    #[allow(deprecated)]
    let app = if settings.middleware.request_timeout_secs > 0 {
        app.layer(TimeoutLayer::new(Duration::from_secs(
            settings.middleware.request_timeout_secs,
        )))
    } else {
        app
    };

    // trace_request stays OUTERMOST (existing behaviour).
    let app = app.layer(axum::middleware::from_fn(
        kokkak_api::middleware::trace::trace_request,
    ));

    // ---- Bind + serve with graceful shutdown ----
    if settings.tls.enabled {
        // T-09: HTTPS path. axum-server + rustls replaces the
        // plain tokio::net::TcpListener / axum::serve flow. The
        // redirect server (T-10) and production enforcement (T-11)
        // build on top of this branch.
        let cert_path = std::path::PathBuf::from(settings.tls.cert_path_or_empty());
        let key_path = std::path::PathBuf::from(settings.tls.key_path_or_empty());
        let tls_config = build_rustls_config(&cert_path, &key_path).unwrap_or_else(|err| {
            eprintln!("[kokkak-api] failed to build TLS config: {err:#}");
            std::process::exit(1);
        });

        // T-10: HSTS header. Applied LAST in the layer chain so
        // it runs FIRST on the response (layers form a LIFO
        // stack). `if_not_present` so handlers can opt-out per
        // response if they ever need to.
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

        // T-10: optional plain-HTTP → HTTPS redirect listener.
        // Lives in its own tokio task so the main HTTPS server
        // is unaffected. If the port is in use (e.g. another
        // service on :80) we log a warning rather than abort so
        // the HTTPS server still comes up — operators can then
        // decide whether to stop the conflicting service.
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

        // axum-server uses its own graceful-shutdown primitive: a
        // shared `Handle` that we signal from the same Ctrl-C / SIGTERM
        // listener as the plain-HTTP path. The handle is wired BEFORE
        // `.serve()` so in-flight requests drain before the listener
        // closes.
        let tls_handle = axum_server::Handle::new();
        tokio::spawn({
            let tls_handle = tls_handle.clone();
            async move {
                shutdown_signal().await;
                tls_handle.shutdown();
            }
        });
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
        .serve(app.into_make_service())
        .await
        .expect("TLS server error");
    } else {
        // Plain HTTP path (dev mode).
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

        axum::serve(listener, app)
            .with_graceful_shutdown(shutdown_signal())
            .await
            .expect("server error");
    }

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
        // M12: a SQL Server URL is set but the topology has not
        // been built yet. The actual `MultiDbHealthCheck` is
        // wired AFTER `build_repos` (see `main` below), so this
        // branch is only reached in dev when the URL is set but
        // the operator's expectation is JSON-DB (e.g. the env
        // var leaked from staging). The factory falls back to
        // JSON when the topology build fails; we just log a
        // hint here.
        tracing::debug!(
            "sqlserver_url is set; /readyz will report sqlserver once the topology is built"
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

/// T-06: build a [`CorsLayer`] from the configured allowlist.
///
/// Returns `None` if the allowlist is empty — the absence of a
/// CORS layer is the safest default (cross-origin requests are
/// rejected by the browser before they reach the server).
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
            ])
            .allow_credentials(true),
    )
}
