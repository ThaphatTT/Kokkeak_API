

use axum::{
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
};
use bytes::Bytes;
use kokkak_domain::StorageKey;
use serde::Deserialize;

use crate::signed_url;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct SignedUrlParams {

    pub exp: Option<u64>,

    pub sig: Option<String>,
}

pub async fn files_handler(
    State(state): State<AppState>,
    Path(rel_path): Path<String>,
    Query(params): Query<SignedUrlParams>,
) -> Result<Response, Response> {
    let Some(exp) = params.exp else {
        return Err(reject("missing exp"));
    };
    let Some(sig) = params.sig.as_deref() else {
        return Err(reject("missing sig"));
    };

    if !signed_url::verify(state.signed_url_secret.as_bytes(), &rel_path, exp, sig) {

        return Err(reject("invalid or expired signature"));
    }

    let bytes: Option<Bytes> = state
        .storage
        .get(&StorageKey(rel_path.clone()))
        .await
        .map_err(|e| {
            tracing::warn!(
                key = %rel_path,
                error = %e,
                "storage get failed on signed-url fetch"
            );
            (StatusCode::INTERNAL_SERVER_ERROR, "storage backend error").into_response()
        })?;
    let Some(bytes) = bytes else {
        return Err((StatusCode::NOT_FOUND, "not found").into_response());
    };

    Ok((
        [(header::CONTENT_TYPE, guess_content_type(&rel_path))],
        bytes,
    )
        .into_response())
}

fn guess_content_type(path: &str) -> &'static str {
    let Some((_, ext)) = path.rsplit_once('.') else {
        return "application/octet-stream";
    };
    match ext.to_ascii_lowercase().as_str() {
        "webp" => "image/webp",
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        _ => "application/octet-stream",
    }
}

fn reject(reason: &'static str) -> Response {

    tracing::debug!(reason = reason, "signed-url fetch rejected");
    (StatusCode::FORBIDDEN, "forbidden").into_response()
}
