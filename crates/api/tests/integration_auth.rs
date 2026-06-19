//! End-to-end integration test for the auth flow.
//!
//! Stands up an in-process router with the JSON-DB sim, drives the
//! auth endpoints via `tower::ServiceExt::oneshot`, and asserts the
//! full register → login → /users/me flow returns the expected
//! JSON envelopes.

use std::sync::Arc;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use kokkak_api::{build_app_state_json, build_router, AppState};
use kokkak_domain::HealthRegistry;
use kokkak_infra::auth::jwt::JwtService;
use kokkak_infra::db::json_catalog::JsonServiceRepository;
use kokkak_infra::db::json_order::JsonOrderRepository;
use kokkak_infra::db::json_user::JsonUserRepository;
use std::path::PathBuf;
use tower::ServiceExt;
use uuid::Uuid;

async fn make_app() -> (axum::Router, Vec<PathBuf>) {
    let tmp = std::env::temp_dir()
        .join("kokkak_api_e2e_test")
        .join(Uuid::new_v4().to_string());
    std::fs::create_dir_all(&tmp).unwrap();
    let paths = vec![
        tmp.join("users.json"),
        tmp.join("services.json"),
        tmp.join("orders.json"),
    ];
    for p in &paths {
        let _ = std::fs::remove_file(p);
    }
    let user_repo = JsonUserRepository::open(&paths[0]).await.unwrap();
    let service_repo = JsonServiceRepository::open(&paths[1]).await.unwrap();
    let order_repo = JsonOrderRepository::open(&paths[2]).await.unwrap();
    let user_repo_arc: Arc<dyn kokkak_domain::UserRepository> = Arc::new(user_repo);
    let service_repo_arc: Arc<dyn kokkak_domain::ServiceRepository> = Arc::new(service_repo);
    let order_repo_arc: Arc<dyn kokkak_domain::OrderRepository> = Arc::new(order_repo);
    let settings = kokkak_common::config::AuthSettings {
        jwt_secret: "e2e-test-secret".into(),
        issuer: "kokkak-e2e".into(),
        access_ttl_secs: 60,
        refresh_ttl_secs: 600,
    };
    let jwt = Arc::new(JwtService::new(&settings).unwrap());
    let translation: Arc<dyn kokkak_domain::TranslationRepository> = Arc::new(
        kokkak_infra::cache::translation_cache::CachedTranslationRepository::new(
            kokkak_infra::db::json_translation::JsonTranslationRepository::in_memory(),
        ),
    );
    let state: AppState = build_app_state_json(
        user_repo_arc,
        service_repo_arc,
        order_repo_arc,
        jwt,
        HealthRegistry::new(),
        translation,
    );
    let app = build_router(state);
    (app, paths)
}

#[tokio::test]
async fn register_then_login_then_me_round_trip() {
    let (app, paths) = make_app().await;
    let email = format!("user-{}@example.com", Uuid::new_v4());

    // 1) Register
    let reg_body = serde_json::json!({
        "username": &email,
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
    assert_eq!(v["data"]["user"]["username"], email);
    let access_token = v["data"]["access_token"].as_str().unwrap().to_string();
    assert!(!access_token.is_empty());

    // 2) Login
    let login_body = serde_json::json!({
        "username": &email,
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
    assert!(!login_token.is_empty());

    // 3) /users/me with the login token
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
    assert_eq!(v["data"]["username"], email);
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

    // Cleanup.
    for p in paths {
        let _ = std::fs::remove_file(&p);
    }
}

#[tokio::test]
async fn register_duplicate_email_returns_409() {
    let (app, paths) = make_app().await;
    let email = format!("dup-{}@example.com", Uuid::new_v4());
    let body = serde_json::json!({
        "username": &email,
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
    for p in paths {
        let _ = std::fs::remove_file(&p);
    }
}

#[tokio::test]
async fn login_with_wrong_password_returns_401() {
    let (app, paths) = make_app().await;
    let email = format!("u-{}@example.com", Uuid::new_v4());
    // Register first.
    let reg = serde_json::json!({
        "username": &email,
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
    // Try wrong password.
    let login = serde_json::json!({
        "username": &email,
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
    for p in paths {
        let _ = std::fs::remove_file(&p);
    }
}

#[tokio::test]
async fn healthz_returns_200() {
    let (app, paths) = make_app().await;
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
    for p in paths {
        let _ = std::fs::remove_file(&p);
    }
}

#[tokio::test]
async fn readyz_returns_200_when_no_checks() {
    let (app, paths) = make_app().await;
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
    for p in paths {
        let _ = std::fs::remove_file(&p);
    }
}

#[tokio::test]
async fn catalog_list_returns_empty_envelope_when_no_services() {
    let (app, paths) = make_app().await;
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
    for p in paths {
        let _ = std::fs::remove_file(&p);
    }
}
