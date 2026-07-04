

use kokkak_api::openapi::ApiDoc;
use std::collections::BTreeMap;
use utoipa::openapi::path::PathItemType;
use utoipa::OpenApi;

#[tokio::test]
async fn spec_root_conforms_to_openapi_3() {
    let spec = ApiDoc::openapi();

    let json = serde_json::to_value(&spec.openapi).unwrap();
    assert_eq!(json, serde_json::json!("3.0.3"));
    assert_eq!(spec.info.title, "Kokkeak API");
    assert_eq!(spec.info.version, "0.1.0");
}

#[tokio::test]
async fn spec_documents_all_critical_paths() {

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
        "/api/v1/admin/users/{guid}/permissions",
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

#[tokio::test]
async fn error_codes_catalog_includes_all_published_codes() {

    use kokkak_api::openapi::error_codes_catalog;

    let catalog = error_codes_catalog();
    let catalog_codes: std::collections::HashSet<&str> = catalog.iter().map(|e| e.code).collect();

    for required in [
        kokkak_common::error_codes::ErrorCode::BAD_REQUEST,
        kokkak_common::error_codes::ErrorCode::IDEMPOTENCY_KEY_REQUIRED,
        kokkak_common::error_codes::ErrorCode::UNAUTHORIZED,
        kokkak_common::error_codes::ErrorCode::FORBIDDEN,
        kokkak_common::error_codes::ErrorCode::NOT_FOUND,
        kokkak_common::error_codes::ErrorCode::USERNAME_TAKEN,
        kokkak_common::error_codes::ErrorCode::VALIDATION,
        kokkak_common::error_codes::ErrorCode::RATE_LIMITED,
        kokkak_common::error_codes::ErrorCode::INTERNAL,
    ] {
        assert!(
            catalog_codes.contains(required),
            "error_codes_catalog() must include `{required}`. Missing!"
        );
    }
}

#[tokio::test]
async fn error_codes_catalog_status_matches_semantics() {
    use kokkak_api::openapi::error_codes_catalog;
    let catalog = error_codes_catalog();
    for entry in catalog {

        let valid_range = (400..500).contains(&entry.status) || (500..600).contains(&entry.status);
        assert!(
            valid_range,
            "code `{}` has status {} which is outside 4xx/5xx",
            entry.code, entry.status
        );
    }
}
