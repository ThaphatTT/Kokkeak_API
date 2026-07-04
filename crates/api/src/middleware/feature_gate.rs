

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

        let yaml = "auth: false\norders: true\npayments: false\nchat: true\nadmin: true\n";
        let flags: FeatureFlagSettings = serde_yaml::from_str(yaml).expect("parse");
        assert!(!flags.auth);
        assert!(flags.orders);
        assert!(!flags.payments);
        assert!(flags.chat);
        assert!(flags.admin);
    }
}
