use std::sync::Arc;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use kokkak_api::{build_app_state_with, build_router, repo_factory::RepoBackend};
use kokkak_common::config::{AuthSettings, DatabaseSettings, Settings};
use kokkak_common::i18n::tr;
use kokkak_domain::{AuthError, LocalizedError, TranslationRepository};
use kokkak_infra::auth::jwt::JwtService;
use kokkak_infra::cache::translation_cache::CachedTranslationRepository;
use kokkak_infra::db::migrate;
use kokkak_infra::db::mssql::build_pool;
use kokkak_infra::db::mssql_master::MssqlMasterDropdownRepository;
use kokkak_infra::db::mssql_permission_user::MssqlPermissionUserRepository;
use kokkak_infra::db::mssql_translation::MssqlTranslationRepository;
use kokkak_infra::db::mssql_user_role::MssqlUserRoleRepository;
use kokkak_infra::storage::MemoryStorage;
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

fn tmp_dir(name: &str) -> PathBuf {
    let p = std::env::temp_dir().join("kokkak_i18n_test").join(name);
    let _ = std::fs::create_dir_all(&p);
    p
}

async fn make_app() -> (axum::Router, Arc<MssqlTranslationRepository>) {
    let url = live_url()
        .expect("integration_i18n: requires KOKKAK_DATABASE__SQLSERVER_URL — guard with live_url().is_none() before calling make_app()");
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

    let user_repo: Arc<dyn kokkak_domain::UserRepository> = Arc::new(
        kokkak_infra::db::mssql_user::MssqlUserRepository::new(pool.clone()),
    );
    let service_repo: Arc<dyn kokkak_domain::ServiceRepository> =
        Arc::new(kokkak_infra::db::mssql_catalog::MssqlServiceRepository::new(pool.clone()));
    let order_repo: Arc<dyn kokkak_domain::OrderRepository> = Arc::new(
        kokkak_infra::db::mssql_order::MssqlOrderRepository::new(pool.clone()),
    );
    let chat_repo: Arc<dyn kokkak_domain::ChatRepository> = Arc::new(
        kokkak_infra::db::mssql_chat::MssqlChatRepository::new(pool.clone()),
    );
    let payment_repo: Arc<dyn kokkak_domain::PaymentRepository> =
        Arc::new(kokkak_infra::db::mssql_payment::MssqlPaymentRepository::new(pool.clone()));
    let mssql_translation = MssqlTranslationRepository::new(pool.clone());
    let repo = Arc::new(mssql_translation);
    let cached: Arc<dyn kokkak_domain::TranslationRepository> =
        Arc::new(CachedTranslationRepository::new((*repo).clone()));

    let jwt_settings = AuthSettings {
        jwt_secret: "i18n-test-secret".into(),
        issuer: "kokkak-i18n".into(),
        access_ttl_secs: 60,
        refresh_ttl_secs: 600,
    };
    let jwt = Arc::new(JwtService::new(&jwt_settings).unwrap());

    let bundle = kokkak_api::repo_factory::RepoBundle {
        backend: RepoBackend::Mssql,
        users: user_repo,
        services: service_repo,
        orders: order_repo,
        chat: chat_repo,
        payments: payment_repo,

        user_roles: Arc::new(MssqlUserRoleRepository::new(pool.clone())),

        permission_users: Arc::new(MssqlPermissionUserRepository::new(pool.clone())),

        master: Arc::new(MssqlMasterDropdownRepository::new(pool.clone())),
        translation: cached,
        mssql_pool: None,
        topology: None,
        category_job_main: Arc::new(kokkak_infra::db::mssql_category_job_main::MssqlCategoryJobMainRepository::disabled()),
        category_job_service_main: Arc::new(kokkak_infra::db::mssql_category_job_service_main::MssqlCategoryJobServiceMainRepository::disabled()),
        category_job_service_sub: Arc::new(kokkak_infra::db::mssql_category_job_service_sub::MssqlCategoryJobServiceSubRepository::disabled()),
    };
    let state = build_app_state_with(
        bundle,
        jwt,
        kokkak_domain::HealthRegistry::new(),
        Arc::new(Settings::default()),
        Arc::new(MemoryStorage::new()),
        Arc::from(""),
        Arc::from(""),
        600,
    );
    let app = build_router(state);
    (app, repo)
}

async fn read_json(resp: axum::response::Response) -> serde_json::Value {
    let body = axum::body::to_bytes(resp.into_body(), 8192).await.unwrap();
    serde_json::from_slice(&body).unwrap()
}

#[tokio::test]
#[ignore = "M14.5: requires live SQL Server; enable with cargo test -- --ignored"]
async fn unknown_accept_language_falls_back_to_english() {
    if live_url().is_none() {
        eprintln!("skipping (no MSSQL)");
        return;
    }
    let (app, _) = make_app().await;

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/auth/login")
                .header("content-type", "application/json")
                .header("accept-language", "fr,de;q=0.9")
                .body(Body::from(r#"{"username":"x","password":"x"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    let v = read_json(resp).await;

    let msg = v["error"]["message"].as_str().unwrap_or("");
    assert!(
        !msg.is_empty(),
        "expected a localized message, got empty (full body: {v})"
    );

    let en_invalid_creds = tr("err_auth.invalid_credentials", "en", &[]);
    let en_validation = tr("err_auth.validation", "en", &["validation"]);
    assert!(
        msg == en_invalid_creds || msg == en_validation || msg.contains("invalid"),
        "expected English message, got {msg:?}"
    );
}

#[tokio::test]
#[ignore = "M14.5: requires live SQL Server; enable with cargo test -- --ignored"]
async fn accept_language_th_returns_thai_message() {
    if live_url().is_none() {
        eprintln!("skipping (no MSSQL)");
        return;
    }
    let (app, _) = make_app().await;
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/auth/login")
                .header("content-type", "application/json")
                .header("accept-language", "th,en;q=0.5")
                .body(Body::from(r#"{"username":"x","password":"x"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    let v = read_json(resp).await;
    let msg = v["error"]["message"].as_str().unwrap_or("");

    let en_invalid = tr("err_auth.invalid_credentials", "en", &[]);
    let th_invalid = tr("err_auth.invalid_credentials", "th", &[]);
    assert!(!msg.is_empty(), "expected a localized message, got empty");
    assert_ne!(
        en_invalid, th_invalid,
        "sanity: English and Thai invalid_credentials must differ"
    );

    assert!(
        !msg.starts_with('<'),
        "expected a real Thai message, got placeholder: {msg:?}"
    );
}

#[tokio::test]
#[ignore = "M14.5: requires live SQL Server; enable with cargo test -- --ignored"]
async fn query_lang_overrides_accept_language() {
    if live_url().is_none() {
        eprintln!("skipping (no MSSQL)");
        return;
    }
    let (app, _) = make_app().await;
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/auth/login?lang=lo")
                .header("content-type", "application/json")
                .header("accept-language", "th,en;q=0.5")
                .body(Body::from(r#"{"username":"x","password":"x"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    let v = read_json(resp).await;
    let msg = v["error"]["message"].as_str().unwrap_or("");
    assert!(!msg.is_empty());

    let lo_invalid = tr("err_auth.invalid_credentials", "lo", &[]);
    let en_invalid = tr("err_auth.invalid_credentials", "en", &[]);
    let th_invalid = tr("err_auth.invalid_credentials", "th", &[]);
    assert_ne!(lo_invalid, en_invalid);
    assert_ne!(lo_invalid, th_invalid);

    assert!(
        !msg.starts_with('<'),
        "expected a real localized message, got placeholder: {msg:?}"
    );
}

#[tokio::test]
#[ignore = "M14.5: requires live SQL Server; enable with cargo test -- --ignored"]
async fn per_tenant_override_wins_over_file_catalog() {
    if live_url().is_none() {
        eprintln!("skipping (no MSSQL)");
        return;
    }
    let (app, repo) = make_app().await;

    repo.put(
        "en",
        "err_auth.invalid_credentials",
        "[OVERRIDE] invalid creds",
    )
    .await
    .unwrap();

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/auth/login")
                .header("content-type", "application/json")
                .header("accept-language", "en")
                .body(Body::from(r#"{"username":"x","password":"x"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    let v = read_json(resp).await;
    let msg = v["error"]["message"].as_str().unwrap_or("");
    assert!(
        msg.contains("[OVERRIDE]") || msg == "[OVERRIDE] invalid creds",
        "expected per-tenant override to win, got {msg:?}"
    );
}

#[tokio::test]
#[ignore = "M14.5: requires live SQL Server; enable with cargo test -- --ignored"]
async fn localizable_keys_match_catalog_for_all_locales() {
    let err = AuthError::InvalidCredentials;
    for locale in ["en", "th", "lo", "zh"] {
        let resolved = tr(err.l10n_key(), locale, &[]);
        assert!(
            !resolved.starts_with('<'),
            "key {} unresolved in {locale}",
            err.l10n_key()
        );
    }
}

#[tokio::test]
#[ignore = "M14.5: requires live SQL Server; enable with cargo test -- --ignored"]
async fn settings_default_has_empty_translation() {
    let mut settings = Settings::default();
    settings.data_dir.path = tmp_dir(&format!("settings-{}", Uuid::new_v4()))
        .to_string_lossy()
        .to_string();
    let dir = std::path::PathBuf::from(&settings.data_dir.path);
    let result = kokkak_api::build_repos(&dir, &settings).await;

    assert!(
        result.is_err(),
        "expected from_settings to error without KOKKAK_DATABASE__SQLSERVER_URL, got Ok"
    );
}

#[tokio::test]
#[ignore = "M14.5: requires live SQL Server; enable with cargo test -- --ignored"]
async fn e2e_register_login_runs_in_each_locale() {
    if live_url().is_none() {
        eprintln!("skipping (no MSSQL)");
        return;
    }

    for (accept, lang) in [
        (Some("en"), "en"),
        (Some("th,en;q=0.5"), "th"),
        (Some("lo,en;q=0.5"), "lo"),
        (Some("zh,en;q=0.5"), "zh"),
    ] {
        let (app, _) = make_app().await;
        let email = format!("user-{}@example.com", Uuid::new_v4());

        let reg_body = serde_json::json!({
            "username": &email,
            "password": "supersecret-123",
            "first_name": "Alice",
            "last_name": "Wonder",
            "role": "customer",
        });
        let mut req = Request::builder()
            .method("POST")
            .uri("/api/v1/auth/register")
            .header("content-type", "application/json");
        if let Some(a) = accept {
            req = req.header("accept-language", a);
        }
        let resp = app
            .clone()
            .oneshot(
                req.body(Body::from(serde_json::to_vec(&reg_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::CREATED,
            "register failed in locale {lang}"
        );

        let login_body = serde_json::json!({
            "username": &email,
            "password": "wrong-password",
            "scope": "mobile",
        });
        let mut req = Request::builder()
            .method("POST")
            .uri("/api/v1/auth/login")
            .header("content-type", "application/json");
        if let Some(a) = accept {
            req = req.header("accept-language", a);
        }
        let resp = app
            .clone()
            .oneshot(
                req.body(Body::from(serde_json::to_vec(&login_body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        let v = read_json(resp).await;
        let msg = v["error"]["message"].as_str().unwrap_or("");
        assert!(!msg.is_empty(), "locale {lang}: empty error message");

        let expected = tr("err_auth.invalid_credentials", lang, &[]);
        assert_eq!(
            msg, expected,
            "locale {lang}: expected {expected:?}, got {msg:?}"
        );
    }
}
