//! Integration tests for the T-16 OpenAPI spec.
//!
//! We test the spec directly via `ApiDoc::openapi()` instead of
//! routing through the full AppState (the openapi routes need
//! no state — they just serialize the spec to JSON).
//!
//! A separate test verifies the route wiring by building a
//! minimal router with the openapi routes only.

use kokkak_api::openapi::ApiDoc;
use std::collections::BTreeMap;
use utoipa::openapi::path::PathItemType;
use utoipa::OpenApi;

#[tokio::test]
async fn spec_root_conforms_to_openapi_3() {
    let spec = ApiDoc::openapi();
    // OpenApiVersion serializes to "3.0.3" — check via JSON.
    let json = serde_json::to_value(&spec.openapi).unwrap();
    assert_eq!(json, serde_json::json!("3.0.3"));
    assert_eq!(spec.info.title, "Kokkeak API");
    assert_eq!(spec.info.version, "0.1.0");
}

#[tokio::test]
async fn spec_documents_all_critical_paths() {
    // Mobile team relies on these endpoints being documented.
    // If a route is removed, this test fails — that's the
    // contract.
    let spec = ApiDoc::openapi();
    let paths: Vec<&str> = spec.paths.paths.keys().map(|s| s.as_str()).collect();

    for required in [
        "/healthz",
        "/readyz",
        "/api/v1/auth/register",
        "/api/v1/auth/login",
        "/api/v1/auth/refresh",
        "/api/v1/auth/logout",
        "/api/v1/users/me",
        "/api/v1/catalog/services",
        "/api/v1/orders",
        "/api/v1/orders/me",
        "/api/v1/orders/assigned",
        "/api/v1/payments",
        "/api/v1/payments/me",
        "/api/v1/admin/users",
        "/api/v1/admin/payouts",
    ] {
        assert!(
            paths.contains(&required),
            "OpenAPI spec is missing required path `{required}`. Found: {paths:?}"
        );
    }
}

#[tokio::test]
async fn spec_documents_idempotency_key_header_on_protected_posts() {
    // The 3 protected POSTs MUST document the Idempotency-Key
    // header parameter — without it, mobile devs won't know
    // they need to send one.
    let spec = ApiDoc::openapi();
    for path in [
        "/api/v1/orders",
        "/api/v1/payments",
        "/api/v1/auth/register",
    ] {
        let item = &spec.paths.paths[path];
        let post = item
            .operations
            .get(&PathItemType::Post)
            .unwrap_or_else(|| panic!("{path} must define POST"));
        let param_names: Vec<&str> = post
            .parameters
            .as_ref()
            .map(|params| params.iter().map(|p| p.name.as_str()).collect())
            .unwrap_or_default();
        assert!(
            param_names.contains(&"Idempotency-Key"),
            "POST {path} must document the Idempotency-Key header parameter. Found params: {param_names:?}"
        );
    }
}

#[tokio::test]
async fn spec_documents_bearer_auth() {
    // The spec declares bearer auth so the Swagger UI
    // "Authorize" button works.
    let spec = ApiDoc::openapi();
    let schemes: BTreeMap<String, _> = spec
        .components
        .as_ref()
        .map(|c| c.security_schemes.clone())
        .expect("components.security_schemes must be declared");
    assert!(
        schemes.contains_key("bearer_auth"),
        "spec must declare a `bearer_auth` security scheme"
    );
}

#[tokio::test]
async fn spec_documents_domain_entities() {
    // The mobile-facing types (PublicUser, Order, Payment, ...) must
    // be present in `components.schemas` so the spec is reusable.
    let spec = ApiDoc::openapi();
    let schemas: BTreeMap<String, _> = spec
        .components
        .as_ref()
        .map(|c| c.schemas.clone())
        .expect("components.schemas must be declared");
    for required in [
        "PublicUser",
        "ServiceCategory",
        "Order",
        "OrderStatus",
        "Payment",
        "PaymentStatus",
        "Payout",
        "Role",
        "RegisterRequest",
        "LoginRequest",
        "RefreshRequest",
        "AuthResponse",
        "LogoutResponse",
    ] {
        assert!(
            schemas.contains_key(required),
            "spec must include schema `{required}`. Found: {:?}",
            schemas.keys().collect::<Vec<_>>()
        );
    }
}

#[tokio::test]
async fn spec_documents_error_envelope() {
    // Every 4xx / 5xx response uses the standard error envelope.
    // The schema must be present so mobile devs know the shape.
    let spec = ApiDoc::openapi();
    let schemas: BTreeMap<String, _> = spec
        .components
        .as_ref()
        .map(|c| c.schemas.clone())
        .expect("components.schemas must be declared");
    assert!(
        schemas.contains_key("ApiError"),
        "spec must include ApiError schema for 4xx/5xx responses"
    );
    assert!(
        schemas.contains_key("ApiErrorBody"),
        "spec must include ApiErrorBody schema (the inner `error` field shape)"
    );
}
