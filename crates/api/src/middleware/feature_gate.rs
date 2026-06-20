//! Feature gate middleware (T-31, Strangler).
//!
//! Each Strangler-flagged route group gets a middleware that reads
//! the current flag value from a captured `Arc<AppState>` and
//! returns 404 if the flag is off. The upstream proxy / BFF sees
//! the 404 and forwards the request to the legacy ASP.NET service.
//!
//! ## Wiring
//!
//! ```ignore
//! use kokkak_api::middleware::feature_gate::auth_flag;
//!
//! let auth_routes = Router::new()
//!     .route("/api/v1/auth/login", post(login))
//!     .layer(from_fn(auth_flag(state.clone())));
//! ```
//!
//! ## Why factory functions returning closures?
//!
//! `axum::middleware::from_fn` accepts `Fn(Request, Next) -> Future`.
//! A plain async fn with a `State` extractor works behind
//! `from_fn_with_state`, but its impl bounds make `Router::layer`
//! reject the layer in some compile paths. Capturing the
//! `Arc<AppState>` in a closure and passing it through to an
//! inner async fn matches the pattern already used in
//! `crates/api/src/main.rs` for the idempotency layer.
//! ponytail: minimum code that compiles.
//!
//! When a sixth flag arrives, copy one of these factories —
//! don't reach for a macro or generic abstraction.

use std::sync::Arc;

use axum::{
    extract::Request,
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use kokkak_common::config::FeatureFlagSettings;
use serde_json::json;

use crate::state::AppState;

/// Build the 404 response when a flag is disabled. Kept private
/// so all five gates go through one place — easy to update the
/// error code or message if the upstream proxy starts parsing it.
fn feature_disabled_response() -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(json!({
            "success": false,
            "data": null,
            "error": {
                "code": "feature_disabled",
                "message": "Endpoint is currently served by the legacy service"
            }
        })),
    )
        .into_response()
}

/// Inner async helper: run the next middleware if the flag is
/// enabled, else return the 404 envelope.
async fn gate<F>(state: &AppState, req: Request, next: Next, is_enabled: F) -> Response
where
    F: Fn(&FeatureFlagSettings) -> bool,
{
    if is_enabled(&state.settings.middleware.features) {
        next.run(req).await
    } else {
        feature_disabled_response()
    }
}

// ---------------------------------------------------------------------------
// Factory functions (preferred path).
//
// Each returns a closure that captures the shared `Arc<AppState>`
// and applies the matching gate. Matches the existing idempotency
// pattern in main.rs.
//
// ```ignore
// .layer(from_fn(auth_flag(state.clone())))
// ```
// ---------------------------------------------------------------------------

/// Gate for `/api/v1/auth/*` (login / register / refresh / logout).
pub fn auth_flag(
    state: Arc<AppState>,
) -> impl Fn(Request, Next) -> std::pin::Pin<Box<dyn std::future::Future<Output = Response> + Send>>
       + Clone
       + Send
       + Sync
       + 'static {
    move |req, next| {
        let state = state.clone();
        Box::pin(async move { gate(&state, req, next, |f| f.auth).await })
    }
}

/// Gate for `/api/v1/orders/*` (create / get / list / track).
pub fn orders_flag(
    state: Arc<AppState>,
) -> impl Fn(Request, Next) -> std::pin::Pin<Box<dyn std::future::Future<Output = Response> + Send>>
       + Clone
       + Send
       + Sync
       + 'static {
    move |req, next| {
        let state = state.clone();
        Box::pin(async move { gate(&state, req, next, |f| f.orders).await })
    }
}

/// Gate for `/api/v1/payments/*` (confirm / payout / statement).
pub fn payments_flag(
    state: Arc<AppState>,
) -> impl Fn(Request, Next) -> std::pin::Pin<Box<dyn std::future::Future<Output = Response> + Send>>
       + Clone
       + Send
       + Sync
       + 'static {
    move |req, next| {
        let state = state.clone();
        Box::pin(async move { gate(&state, req, next, |f| f.payments).await })
    }
}

/// Gate for `/api/v1/chat/*` (REST + WebSocket).
pub fn chat_flag(
    state: Arc<AppState>,
) -> impl Fn(Request, Next) -> std::pin::Pin<Box<dyn std::future::Future<Output = Response> + Send>>
       + Clone
       + Send
       + Sync
       + 'static {
    move |req, next| {
        let state = state.clone();
        Box::pin(async move { gate(&state, req, next, |f| f.chat).await })
    }
}

/// Gate for `/api/v1/admin/*` (RBAC, content mgmt, audit).
pub fn admin_flag(
    state: Arc<AppState>,
) -> impl Fn(Request, Next) -> std::pin::Pin<Box<dyn std::future::Future<Output = Response> + Send>>
       + Clone
       + Send
       + Sync
       + 'static {
    move |req, next| {
        let state = state.clone();
        Box::pin(async move { gate(&state, req, next, |f| f.admin).await })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_all_enabled() {
        let flags = FeatureFlagSettings::default();
        assert!(flags.auth);
        assert!(flags.orders);
        assert!(flags.payments);
        assert!(flags.chat);
        assert!(flags.admin);
    }

    #[test]
    fn gate_can_be_disabled_individually() {
        // Round-trip through serde so we exercise the actual
        // config path operators use.
        let yaml = "auth: false\norders: true\npayments: false\nchat: true\nadmin: true\n";
        let flags: FeatureFlagSettings = serde_yaml::from_str(yaml).expect("parse");
        assert!(!flags.auth);
        assert!(flags.orders);
        assert!(!flags.payments);
        assert!(flags.chat);
        assert!(flags.admin);
    }
}
