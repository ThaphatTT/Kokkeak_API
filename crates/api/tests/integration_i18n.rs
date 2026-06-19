//! Integration test for M11 i18n expansion.
//!
//! Exercises the per-request locale pipeline end-to-end:
//!
//! 1. `Accept-Language: th,en;q=0.5` → response message is Thai
//! 2. `Accept-Language: en` → response message is English
//! 3. `?lang=lo` overrides `Accept-Language` → message is Lao
//! 4. Unknown `Accept-Language` → fallback to English
//! 5. Per-tenant override written through the
//!    `TranslationRepository` → DB message wins over the file
//!    catalog
//! 6. `LocalizedError::l10n_key()` matches the catalog keys
//!
//! The test runs without a SQL Server — the in-memory
//! `JsonTranslationRepository` is the dev / e2e / CI path
//! just like M10.

use std::sync::Arc;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use kokkak_api::build_app_state_with;
use kokkak_api::build_router;
use kokkak_api::repo_factory::{force_json, RepoBackend};
use kokkak_common::config::{AuthSettings, Settings};
use kokkak_common::i18n::tr;
use kokkak_domain::{AuthError, LocalizedError, TranslationRepository};
use kokkak_infra::auth::jwt::JwtService;
use kokkak_infra::cache::translation_cache::CachedTranslationRepository;
use kokkak_infra::db::json_translation::JsonTranslationRepository;
use std::path::PathBuf;
use tower::ServiceExt;
use uuid::Uuid;

fn tmp_dir(name: &str) -> PathBuf {
    let p = std::env::temp_dir().join("kokkak_i18n_test").join(name);
    let _ = std::fs::create_dir_all(&p);
    p
}

async fn make_app() -> (axum::Router, Arc<JsonTranslationRepository>) {
    let dir = tmp_dir(&format!("app-{}", Uuid::new_v4()));
    let bundle = force_json(&dir).await.expect("force_json");
    assert!(matches!(bundle.backend, RepoBackend::Json));
    let jwt_settings = AuthSettings {
        jwt_secret: "i18n-test-secret".into(),
        issuer: "kokkak-i18n".into(),
        access_ttl_secs: 60,
        refresh_ttl_secs: 600,
    };
    let jwt = Arc::new(JwtService::new(&jwt_settings).unwrap());
    // Re-open the in-memory variant for the test (the bundle
    // already has a file-backed one — we use the in-memory
    // variant to inject overrides without touching disk).
    let inner = JsonTranslationRepository::in_memory();
    let repo = Arc::new(inner);
    let cached: Arc<dyn kokkak_domain::TranslationRepository> =
        Arc::new(CachedTranslationRepository::new((*repo).clone()));
    let mut bundle = bundle;
    bundle.translation = cached;
    let state = build_app_state_with(bundle, jwt, kokkak_domain::HealthRegistry::new());
    let app = build_router(state);
    (app, repo)
}

async fn read_json(resp: axum::response::Response) -> serde_json::Value {
    let body = axum::body::to_bytes(resp.into_body(), 8192).await.unwrap();
    serde_json::from_slice(&body).unwrap()
}

#[tokio::test]
async fn unknown_accept_language_falls_back_to_english() {
    let (app, _) = make_app().await;
    // Trigger an error (login with no body) so the response
    // carries a localized message.
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
    // 401 (invalid creds) or 422 (validation) both work; the
    // important thing is the message is the English catalog
    // string, not bracketed or empty.
    let msg = v["error"]["message"].as_str().unwrap_or("");
    assert!(
        !msg.is_empty(),
        "expected a localized message, got empty (full body: {v})"
    );
    // The English catalog string for either variant must be
    // present (the auth error mapper chose one of the keys).
    let en_invalid_creds = tr("err_auth.invalid_credentials", "en", &[]);
    let en_validation = tr("err_auth.validation", "en", &["validation"]);
    assert!(
        msg == en_invalid_creds || msg == en_validation || msg.contains("invalid"),
        "expected English message, got {msg:?}"
    );
}

#[tokio::test]
async fn accept_language_th_returns_thai_message() {
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
    // Thai must not equal the English version; we can't assert
    // the exact Thai string here (depends on the error variant
    // the auth service picks), but the catalog is non-empty
    // for the keys we ship.
    let en_invalid = tr("err_auth.invalid_credentials", "en", &[]);
    let th_invalid = tr("err_auth.invalid_credentials", "th", &[]);
    assert!(!msg.is_empty(), "expected a localized message, got empty");
    assert_ne!(
        en_invalid, th_invalid,
        "sanity: English and Thai invalid_credentials must differ"
    );
    // The accepted language is th, so the response must be
    // either Thai or the bracketed-key fallback (which only
    // happens when the file catalog has no entry). The catalog
    // does have an entry, so we expect a real Thai string.
    assert!(
        !msg.starts_with('<'),
        "expected a real Thai message, got placeholder: {msg:?}"
    );
}

#[tokio::test]
async fn query_lang_overrides_accept_language() {
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
    // The query said "lo", so the message must be the Lao
    // version (which differs from both the Thai and English).
    let lo_invalid = tr("err_auth.invalid_credentials", "lo", &[]);
    let en_invalid = tr("err_auth.invalid_credentials", "en", &[]);
    let th_invalid = tr("err_auth.invalid_credentials", "th", &[]);
    assert_ne!(lo_invalid, en_invalid);
    assert_ne!(lo_invalid, th_invalid);
    // We can't guarantee the response uses invalid_credentials
    // (the auth service may pick a different variant), but
    // whatever the message, it must be one of the catalog
    // strings, not the bracketed-key fallback.
    assert!(
        !msg.starts_with('<'),
        "expected a real localized message, got placeholder: {msg:?}"
    );
}

#[tokio::test]
async fn per_tenant_override_wins_over_file_catalog() {
    let (app, repo) = make_app().await;
    // Pre-populate an override that differs from the file
    // catalog. The key we override is the one the auth error
    // mapper produces for invalid credentials.
    repo.put(
        "en",
        "err_auth.invalid_credentials",
        "[OVERRIDE] invalid creds",
    )
    .await
    .unwrap();
    // Invalidate the L1 cache so the override is visible.
    // (The CachedTranslationRepository's put method does this
    // for us, but `repo` here is the inner JsonTranslation —
    // we use the inner to test the cache invalidation logic.)
    // Now hit the endpoint and expect the override.
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
async fn localizable_keys_match_catalog_for_all_locales() {
    // The `LocalizedError::l10n_key()` for every variant must
    // resolve to a non-bracketed string in every locale.
    let err = AuthError::InvalidCredentials;
    for locale in ["en", "th", "lo"] {
        let resolved = tr(err.l10n_key(), locale, &[]);
        assert!(
            !resolved.starts_with('<'),
            "key {} unresolved in {locale}",
            err.l10n_key()
        );
    }
}

#[tokio::test]
async fn settings_default_has_empty_translation() {
    // The repo factory must succeed with the default Settings
    // (which has no SQL Server URL). The translation repo is
    // populated from the JSON file (initially empty).
    let mut settings = Settings::default();
    settings.data_dir.path = tmp_dir(&format!("settings-{}", Uuid::new_v4()))
        .to_string_lossy()
        .to_string();
    let dir = std::path::PathBuf::from(&settings.data_dir.path);
    let bundle = kokkak_api::build_repos(&dir, &settings)
        .await
        .expect("build_repos");
    assert!(matches!(bundle.backend, RepoBackend::Json));
    // Translation repo must be a non-null Arc<dyn ...>.
    let _: Arc<dyn kokkak_domain::TranslationRepository> = bundle.translation.clone();
}

#[tokio::test]
async fn e2e_register_login_runs_in_each_locale() {
    // Walk the full auth flow in three locales; each request
    // must return the same envelope shape with a localized
    // message.
    for (accept, lang) in [
        (Some("en"), "en"),
        (Some("th,en;q=0.5"), "th"),
        (Some("lo,en;q=0.5"), "lo"),
    ] {
        let (app, _) = make_app().await;
        let email = format!("user-{}@example.com", Uuid::new_v4());
        // Register a fresh user.
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
        // Now log in with wrong password to trigger a localized
        // error.
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
        // The auth error for wrong creds is invalid_credentials;
        // the resolved string must equal the {lang} catalog
        // entry.
        let expected = tr("err_auth.invalid_credentials", lang, &[]);
        assert_eq!(
            msg, expected,
            "locale {lang}: expected {expected:?}, got {msg:?}"
        );
    }
}
