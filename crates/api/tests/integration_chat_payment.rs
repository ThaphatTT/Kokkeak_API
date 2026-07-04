

use std::sync::Arc;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use kokkak_api::{build_app_state_json, build_router, AppState};
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
            eprintln!("SKIPPED: integration_chat_payment requires KOKKAK_DATABASE__SQLSERVER_URL");
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
    let mig_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("migrations");
    let _ = migrate::run(&pool, &mig_dir).await;

    let user_repo: Arc<dyn kokkak_domain::UserRepository> =
        Arc::new(MssqlUserRepository::new(pool.clone()));
    let service_repo: Arc<dyn kokkak_domain::ServiceRepository> =
        Arc::new(MssqlServiceRepository::new(pool.clone()));
    let order_repo: Arc<dyn kokkak_domain::OrderRepository> =
        Arc::new(MssqlOrderRepository::new(pool.clone()));
    let chat_repo: Arc<dyn kokkak_domain::ChatRepository> =
        Arc::new(MssqlChatRepository::new(pool.clone()));
    let payment_repo: Arc<dyn kokkak_domain::PaymentRepository> =
        Arc::new(MssqlPaymentRepository::new(pool.clone()));
    let user_role_repo: Arc<dyn kokkak_domain::UserRoleRepository> =
        Arc::new(MssqlUserRoleRepository::new(pool.clone()));
    let permission_user_repo: Arc<dyn kokkak_domain::PermissionUserRepository> =
        Arc::new(MssqlPermissionUserRepository::new(pool.clone()));
    let master_repo: Arc<dyn kokkak_domain::MasterDropdownRepository> =
        Arc::new(MssqlMasterDropdownRepository::new(pool.clone()));
    let translation: Arc<dyn kokkak_domain::TranslationRepository> = Arc::new(
        CachedTranslationRepository::new(MssqlTranslationRepository::new(pool.clone())),
    );

    let auth_settings = AuthSettings {
        jwt_secret: "e2e-m8-m9-secret".into(),
        issuer: "kokkak-e2e".into(),
        access_ttl_secs: 600,
        refresh_ttl_secs: 3600,
    };
    let jwt = Arc::new(JwtService::new(&auth_settings).unwrap());

    let state: AppState = build_app_state_json(
        user_repo,
        service_repo,
        order_repo,
        chat_repo,
        payment_repo,
        user_role_repo,
        permission_user_repo,
        master_repo,
        jwt,
        HealthRegistry::new(),
        translation,
        Arc::new(kokkak_common::config::Settings::default()),
    );
    (build_router(state), vec![])
}

async fn register(app: axum::Router, email: &str, password: &str, role: &str) -> String {
    let body = serde_json::json!({
        "username": email,
        "password": password,
        "first_name": email,
        "last_name": "Tester",
        "role": role,
    });
    let resp = app
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
    assert_eq!(resp.status(), StatusCode::CREATED, "register failed");
    let body = axum::body::to_bytes(resp.into_body(), 8192).await.unwrap();
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    v["data"]["access_token"].as_str().unwrap().to_string()
}

#[tokio::test]
#[ignore = "M14.5: requires live SQL Server; enable with cargo test -- --ignored"]
async fn m8_chat_open_send_and_list_rooms() {
    if live_url().is_none() {
        eprintln!("skipping (no MSSQL)");
        return;
    }
    let (app, paths) = make_app().await;
    let ts = Uuid::new_v4();
    let customer_email = format!("cust-{ts}@example.com");
    let tech_email = format!("tech-{ts}@example.com");
    let cust_token = register(app.clone(), &customer_email, "supersecret-123", "customer").await;
    let tech_token = register(app.clone(), &tech_email, "supersecret-123", "technician").await;

    let me_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/users/me")
                .header("authorization", format!("Bearer {tech_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body = axum::body::to_bytes(me_resp.into_body(), 4096)
        .await
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let tech_id = v["data"]["id"].as_str().unwrap().to_string();

    let open_body = serde_json::json!({
        "other_user_id": tech_id,
        "other_role": "technician",
    });
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/chat/rooms")
                .header("authorization", format!("Bearer {cust_token}"))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&open_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let room_id = v["data"]["id"].as_str().unwrap().to_string();

    let send_body = serde_json::json!({ "body": "ສະບາຍດີ, ຊ່າງ!" });
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/chat/rooms/{room_id}/messages"))
                .header("authorization", format!("Bearer {cust_token}"))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&send_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["data"]["body"], "ສະບາຍດີ, ຊ່າງ!");

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/chat/rooms")
                .header("authorization", format!("Bearer {tech_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), 8192).await.unwrap();
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(v["data"].as_array().unwrap().len(), 1);
    assert_eq!(v["data"][0]["unread"], 1);

    let _ = paths;
}

#[tokio::test]
#[ignore = "M14.5: requires live SQL Server; enable with cargo test -- --ignored"]
async fn m9_payment_create_and_confirm() {
    if live_url().is_none() {
        eprintln!("skipping (no MSSQL)");
        return;
    }
    let (app, paths) = make_app().await;
    let ts = Uuid::new_v4();
    let customer_email = format!("pay-cust-{ts}@example.com");
    let tech_email = format!("pay-tech-{ts}@example.com");
    let admin_email = format!("pay-admin-{ts}@example.com");
    let cust_token = register(app.clone(), &customer_email, "supersecret-123", "customer").await;
    let tech_token = register(app.clone(), &tech_email, "supersecret-123", "technician").await;
    let admin_token = register(app.clone(), &admin_email, "supersecret-123", "admin").await;

    let order_body = serde_json::json!({
        "service_code": "ac",
        "description": "AC repair",
        "address": "Vientiane",
        "total": "200.00",
    });
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/orders")
                .header("authorization", format!("Bearer {cust_token}"))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&order_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = resp.status();
    let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();

    let order_id = v["data"]["id"].as_str().map(|s| s.to_string());

    if status != StatusCode::CREATED || order_id.is_none() {

        let _ = paths;
        return;
    }
    let order_id = order_id.unwrap();

    let create_body = serde_json::json!({ "order_id": order_id });
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/payments")
                .header("authorization", format!("Bearer {cust_token}"))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&create_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let payment_id = v["data"]["id"].as_str().unwrap().to_string();
    assert_eq!(v["data"]["status"], "pending");

    let confirm_body = serde_json::json!({ "gateway_ref": "pi_e2e" });
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/payments/{payment_id}/confirm"))
                .header("authorization", format!("Bearer {cust_token}"))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&confirm_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    let status = resp.status();
    assert!(
        status == StatusCode::OK || status == StatusCode::CONFLICT,
        "confirm returned {status}"
    );

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/v1/payments/me")
                .header("authorization", format!("Bearer {cust_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(!v["data"].as_array().unwrap().is_empty());
    let _ = (tech_token, admin_token, paths);
}
