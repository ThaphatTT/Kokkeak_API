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

/// T03: serve Prometheus text-format metrics.
async fn metrics_handler() -> impl IntoResponse {
    let body = telemetry::render_metrics();
    Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, "text/plain; version=0.0.4; charset=utf-8")
        .body(Body::from(body))
        .expect("failed to build metrics response")
}

fn main() {
    // ---- T-09: install the rustls crypto provider BEFORE anything
    //   else. `rustls` 0.23 ships without a default provider — a release
    //   build that reaches `ServerConfig::builder()` without
    //   `install_default()` panics with "Could not automatically
    //   determine the process-level CryptoProvider". The check is
    //   `set_once` so calling it twice is harmless (the second call
    //   returns `Err` which we ignore). Dev builds already get a
    //   provider via `tiberius` / `rust-s3` — this `install_default`
    //   makes the release build work the same way. ----
    let _ = rustls::crypto::ring::default_provider().install_default();

    // ---- T02: load .env (if present) into process env BEFORE
    //   Settings::load() — figment's Env provider only reads from
    //   std::env, not from disk.
    //
    //   Resolution order:
    //   1. `KOKKAK_ENV_FILE` env var (explicit path; lets a runner
    //      script like `scripts/prod-run.ps1` point at the file
    //      regardless of the current working directory).
    //   2. `KOKKAK_ENVIRONMENT=production` → `.env.production`
    //   3. Otherwise → `.env.dev` (development default).
    //
    //   Production deploys that inject env vars via docker / k8s /
    //   systemd usually ship no .env file — `from_filename` is a
    //   no-op in that case. ----
    let env_file = std::env::var("KOKKAK_ENV_FILE")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| match std::env::var("KOKKAK_ENVIRONMENT").as_deref() {
            Ok("production") => ".env.production".to_string(),
            _ => ".env.dev".to_string(),
        });
    let _ = dotenvy::from_filename(&env_file);

    // ---- T02: load & validate configuration ----
    let settings = Settings::load().unwrap_or_else(|err| {
        eprintln!("[kokkak-api] invalid configuration: {err}");
        eprintln!("[kokkak-api] see .env.example for required variables");
        std::process::exit(1);
    });

    // ---- T-19: build the tokio runtime with `settings.server.workers`
    //   as the worker count. `#[tokio::main]` defaults to num_cpus,
    //   which is fine on a dev box but wasteful on a 2-CPU pod and
    //   insufficient on a 32-core production node. Constructing the
    //   runtime by hand lets us honour the operator's choice.
    //
    //   Settings validation already rejects `workers == 0` (see
    //   `Settings::validate()` in `common/src/config.rs`), so the
    //   `.max(1)` here is defensive, not load-bearing.
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
    // ---- M11: initialize the i18n catalog (th / en / lo). The
    //   default locale is the catalog fallback; per-request
    //   locale is set by the locale_middleware.
    i18n::init_i18n("en");
    // ---- T03: init tracing (JSON or pretty) + Prometheus metrics ----
    telemetry::init_tracing(settings.log.format);
    let _metrics_handle = Arc::new(telemetry::init_metrics());
    // ---- T-24: opt-in OTLP bridge. No-op unless the `otel`
    //   feature is enabled and OTEL_EXPORTER_OTLP_ENDPOINT is set.
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

    // ---- M9 / T-16: build the object-storage adapter (S3 in
    //   prod, local FS in the Strangler transition, in-memory
    //   fallback for unit tests). Failure to build exits the
    //   process — there's no useful behaviour for an API with
    //   no working storage when handlers ask for it.
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

    // ---- T-23: resolve the public base URL. Both are derived from
    //   env so the same code path runs in dev (LocalStorage +
    //   public_base_url from `.env.dev`) and prod (S3/Local +
    //   public_base_url from `.env.production`). Empty
    //   `public_base_url` is allowed in dev (every `*_img_url`
    //   becomes `null`); the validator rejects empty + persistent
    //   storage in production.
    let public_base_url: Arc<str> = Arc::from(settings.server.public_base_url.as_str());
    // T-23-b: HMAC sign material. Empty in dev (URLs carry no
    // `?exp=&sig=` query, the `/files/*` handler rejects all
    // requests). Production requires 32-byte secret + ttl in
    // 60..=3600 range (validator enforces).
    let signed_url_secret: Arc<str> = Arc::from(settings.storage.signed_url_secret.as_str());
    let signed_url_ttl_secs: u32 = settings.storage.signed_url_ttl_secs;
    tracing::info!(
        signed_url_ttl_secs = signed_url_ttl_secs,
        signed_url_secret_len = signed_url_secret.len(),
        "signed-url knobs loaded"
    );

    // ---- Build app state ----
    let settings_arc = Arc::new(settings.clone());
    let state = build_app_state_with(
        bundle,
        jwt,
        registry,
        settings_arc.clone(),
        storage,
        public_base_url.clone(),
        signed_url_secret.clone(),
        signed_url_ttl_secs,
    );

    // ---- Routes ----
    let mut app = build_router(state.clone()).route("/metrics", get(metrics_handler));
    // T-23-b: `/files/*` is now HMAC-signed — the API proxies
    // every image fetches regardless of the storage adapter
    // (Local OR S3) so the URL contract is uniform. Replacing the
    // earlier open ServeDir mount (which let anyone who knew a
    // path read a KYC document). The unsigned/anonymous request
    // path returns 403 — Info logging on reject, no PII leaked.
    let files_route = axum::Router::new()
        .route("/files/*path", get(kokkak_api::files::files_handler))
        .with_state(state.clone());
    app = app.merge(files_route);
    tracing::info!(
        mount = "/files/*",
        "signed-URL route mounted (HMAC-SHA256, time-limited)"
    );
    #[allow(unused_imports)]
    {
        // The previous `ServeDir` mount (`tower-http::services`)
        // is intentionally removed — T-23-b routes every
        // /files/* request through the signer instead. The
        // `tower-http` `"fs"` feature stays on (used elsewhere).
    }

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

    // T-07 + R-02: per-IP rate limit.
    // - backend=memory (default): per-instance `tower_governor` GCRA.
    //   Fine for single-pod dev/test; multiplies by pod count under HPA.
    // - backend=redis: shared counter via Lua INCR + EXPIRE. Required
    //   before running more than one replica.
    // Sits BETWEEN cors and compression: short-circuits before
    // we pay the compression CPU cost on requests that will be
    // dropped anyway.
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
                // GC the limiter's key storage every 5 minutes so a busy
                // day doesn't balloon memory with stale per-IP entries.
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
                // R-02: build a dedicated `deadpool-redis` pool for
                // the rate limiter. The 1-second window matches the
                // `requests_per_second` knob semantics — each
                // `burst_size` hits are allowed per wall-clock second,
                // then the window rolls over. A future finer-grained
                // window (e.g. 100ms sliding) can be added by
                // changing the constant + plumbing a setting knob.
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

    // ---- T-14: HTTP idempotency middleware.
    //   Construct the in-memory store when the feature is enabled.
    //   Layer order: idempotency sits between trace (outer) and
    //   timeout/compression/cors (inner) so the cache hit short-
    //   circuits before the more expensive layers but after the
    //   request is logged.
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

    // ---- T-16: request safety layers.
    //   Body limit and concurrency cap sit OUTERMOST in the chain
    //   (these are the last `.layer()` calls) so oversized bodies
    //   and excess load are shed before any other layer spends
    //   work on them. Order between the two doesn't matter
    //   semantically — both reject; the body limit is marginally
    //   cheaper (a header / first-chunk peek vs a per-request
    //   permit acquisition), so it sits one rung further out.
    //
    //   ponytail: we use `tower_http::RequestBodyLimitLayer` for
    //   the body cap (returns 413) and a tiny custom
    //   `ConcurrencyCap` middleware (returns 503) instead of
    //   `tower::load_shed::LoadShedLayer` — the latter returns
    //   `BoxError` which axum's `Router::layer` can't accept.
    //   The custom middleware uses `tokio::sync::Semaphore`
    //   directly so the error type stays `Infallible`-compatible.
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

    // ---- Expose `Arc<Settings>` as a request-scoped Extension so
    //   extractors (e.g. `ClientIp`) can read feature flags without
    //   changing their generic `S` constraint. Cheap to clone on
    //   every request — Arc increments are ~5 ns. ----
    let app = app.layer(Extension(settings_arc.clone()));
    tracing::info!("settings Extension wired (extractors can read feature flags)");

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

        // T-12: cert file watcher (auto-reload for LE 90-day
        // rotation). Only wired when `tls.auto_reload = true` —
        // the default is off because the restart causes a brief
        // connection blip. Operators opt in when they need
        // zero-touch cert rotation.
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
                        // Keep the watcher alive for the task lifetime.
                        let _watcher = watcher;
                        // Drain the initial false value so `changed()`
                        // awaits the next true signal.
                        let _ = rx.borrow_and_update();
                        if rx.changed().await.is_err() {
                            return;
                        }
                        // Debounce: file watchers can fire multiple
                        // events for a single rotation (write to temp
                        // file → rename → modify). 500 ms covers the
                        // typical LE renewal pattern.
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
                    // Watcher init failure should not crash a
                    // long-running server — log loudly and continue.
                    // The certs are loaded; the service just won't
                    // auto-reload. Operator can restart manually.
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
        // R-02: `into_make_service_with_connect_info` so the
        // Redis-backed rate-limit middleware can extract the
        // client IP via `ConnectInfo<SocketAddr>`. The memory
        // backend (tower_governor) reads the IP from the
        // connection directly via its own extractor, so the
        // extra plumbing costs nothing on that path.
        .serve(app.into_make_service_with_connect_info::<std::net::SocketAddr>())
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

        // R-02: see the TLS branch above — same `ConnectInfo`
        // wiring, same reason.
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

/// R-02: build a dedicated `deadpool-redis` pool for the rate
/// limiter.
///
/// We do NOT reuse `kokkak_infra::cache::redis::RedisCache`'s
/// pool because (a) the cache pool may be sized for big-value
/// traffic, (b) the rate limiter has very different access
/// patterns (1 RTT per request, no pipelining), and (c) keeping
/// the limiter pool independent means a runaway cache cannot
/// exhaust the limiter's connections.
///
/// ponytail: the URL comes from `settings.redis.url` — the same
/// `KOKKAK_REDIS__URL` knob. If the operator wants a separate
/// Redis instance just for rate limiting, that knob is the
/// single seam to extend.
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
