//! End-to-end integration test for the auth flow (M14.5 — MSSQL only).
//!
//! Runs against a real SQL Server reachable via
//! `KOKKAK_DATABASE__SQLSERVER_URL`. Skipped when the env var is empty
//! or `"disabled"` (no JSON-DB sim in M14.5).

use std::sync::Arc;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use kokkak_api::{build_router, AppState};
use kokkak_common::config::{AuthSettings, DatabaseSettings};
use kokkak_domain::HealthRegistry;
use kokkak_infra::auth::jwt::JwtService;
use kokkak_infra::cache::translation_cache::CachedTranslationRepository;
use kokkak_infra::db::migrate;
use kokkak_infra::db::mssql::build_pool;
use kokkak_infra::db::mssql_catalog::MssqlServiceRepository;
use kokkak_infra::db::mssql_chat::MssqlChatRepository;
use kokkak_infra::db::mssql_master::MssqlMasterDropdownRepository;
use kokkak_infra::db::mssql_order::MssqlOrderRepository;
use kokkak_infra::db::mssql_payment::MssqlPaymentRepository;
use kokkak_infra::db::mssql_permission_user::MssqlPermissionUserRepository;
use kokkak_infra::db::mssql_translation::MssqlTranslationRepository;
use kokkak_infra::db::mssql_user::MssqlUserRepository;
use kokkak_infra::db::mssql_user_role::MssqlUserRoleRepository;
use std::path::PathBuf;
use tower::ServiceExt;
use uuid::Uuid;

fn live_url() -> Option<String> {
    let raw = std::env::var("KOKKAK_DATABASE__SQLSERVER_URL").ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed == "disabled" {
        return None;
    }
    Some(trimmed.to_string())
}

async fn make_app() -> (axum::Router, Vec<PathBuf>) {
    let url = match live_url() {
        Some(u) => u,
        None => {
            // Return a sentinel — tests using this MUST gate on live_url() first.
            eprintln!("SKIPPED: integration_auth requires KOKKAK_DATABASE__SQLSERVER_URL");
            // Return an empty router and dummy paths; tests will short-circuit.
            return (axum::Router::new(), vec![]);
        }
    };
    let settings = DatabaseSettings {
        sqlserver_url: url,
        pool_size: 4,
        connect_timeout_secs: 5,
        migrations_dir: String::new(),
    };
    let pool = build_pool(&settings).await.expect("build_pool");

    // Run migrations once (idempotent).
    let mig_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("migrations");
    let _ = migrate::run(&pool, &mig_dir).await;

    let user_repo = Arc::new(MssqlUserRepository::new(pool.clone()));
    let service_repo = Arc::new(MssqlServiceRepository::new(pool.clone()));
    let order_repo = Arc::new(MssqlOrderRepository::new(pool.clone()));
    let chat_repo = Arc::new(MssqlChatRepository::new(pool.clone()));
    let payment_repo = Arc::new(MssqlPaymentRepository::new(pool.clone()));

    let jwt_settings = AuthSettings {
        jwt_secret: "e2e-test-secret".into(),
        issuer: "kokkak-e2e".into(),
        access_ttl_secs: 60,
        refresh_ttl_secs: 600,
    };
    let jwt = Arc::new(JwtService::new(&jwt_settings).unwrap());

    let translation: Arc<dyn kokkak_domain::TranslationRepository> = Arc::new(
        CachedTranslationRepository::new(MssqlTranslationRepository::new(pool.clone())),
    );

    let bundle = kokkak_api::repo_factory::RepoBundle {
        backend: kokkak_api::repo_factory::RepoBackend::Mssql,
        users: user_repo,
        services: service_repo,
        orders: order_repo,
        chat: chat_repo,
        payments: payment_repo,
        // M15-prep: shared with the admin permissions endpoint.
        // Tests for this repo live in the unit suite (mock impl);
        // this integration test exercises the rest of the route
        // table and just needs any impl that satisfies the trait.
        user_roles: Arc::new(MssqlUserRoleRepository::new(pool.clone())),
        // M17: dedicated permission-page repository (decoupled from
        // `users`). Auth tests don't exercise permission routes, so a
        // live MSSQL adapter against the same pool is sufficient.
        permission_users: Arc::new(MssqlPermissionUserRepository::new(pool.clone())),
        // M20: master-data dropdowns. Auth tests don't exercise
        // the new master routes, but the bundle requires the field
        // so the wiring matches production.
        master: Arc::new(MssqlMasterDropdownRepository::new(pool.clone())),
        translation,
        mssql_pool: None,
        topology: None,
    };
    let state: AppState = kokkak_api::build_app_state_with(
        bundle,
        jwt,
        HealthRegistry::new(),
        Arc::new(kokkak_common::config::Settings::default()),
    );
    (build_router(state), vec![])
}

#[tokio::test]
async fn register_then_login_then_me_round_trip() {
    let (app, _paths) = make_app().await;
    if live_url().is_none() {
        eprintln!("skipping (no MSSQL)");
        return;
    }
    let username = format!("user-{}@example.com", Uuid::new_v4());

    // 1) Register
    let reg_body = serde_json::json!({
        "username": &username,
        "password": "supersecret-123",
        "first_name": "Alice",
        "last_name": "Wonder",
        "role": "customer",
    });
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/auth/register")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&reg_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["success"], true);
    assert_eq!(v["data"]["user"]["username"], username);
    let _access_token = v["data"]["access_token"].as_str().unwrap().to_string();

    // 2) Login
    let login_body = serde_json::json!({
        "username": &username,
        "password": "supersecret-123",
        "scope": "mobile",
    });
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/auth/login")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&login_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let login_token = v["data"]["access_token"].as_str().unwrap().to_string();

    // 3) /users/me with login token
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/users/me")
                .header("authorization", format!("Bearer {login_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["data"]["username"], username);
    assert_eq!(v["data"]["roles"][0], "customer");

    // 4) /users/me without token → 401
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/users/me")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn register_duplicate_username_returns_409() {
    let (app, _paths) = make_app().await;
    if live_url().is_none() {
        return;
    }
    let username = format!("dup-{}@example.com", Uuid::new_v4());
    let body = serde_json::json!({
        "username": &username,
        "password": "supersecret-123",
        "first_name": "A",
        "last_name": "B",
    });
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/auth/register")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/auth/register")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn login_with_wrong_password_returns_401() {
    let (app, _paths) = make_app().await;
    if live_url().is_none() {
        return;
    }
    let username = format!("u-{}@example.com", Uuid::new_v4());
    let reg = serde_json::json!({
        "username": &username,
        "password": "supersecret-123",
        "first_name": "A",
        "last_name": "B",
    });
    let _ = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/auth/register")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&reg).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    let login = serde_json::json!({
        "username": &username,
        "password": "wrong-password",
    });
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/auth/login")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&login).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn healthz_returns_200() {
    if live_url().is_none() {
        eprintln!("skipping (no MSSQL)");
        return;
    }
    let (app, _) = make_app().await;
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/healthz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn readyz_returns_200_when_no_checks() {
    if live_url().is_none() {
        eprintln!("skipping (no MSSQL)");
        return;
    }
    let (app, _) = make_app().await;
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/readyz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn catalog_list_returns_empty_envelope_when_no_services() {
    let (app, _) = make_app().await;
    if live_url().is_none() {
        return;
    }
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/catalog/services")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["success"], true);
    assert!(v["data"].as_array().unwrap().is_empty());
}
