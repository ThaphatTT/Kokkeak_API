//! `GET /files/*` — serves blobs behind HMAC-signed URLs.
//!
//! Replaces the T-23 open `ServeDir` mount in production.
//! Designed to work for BOTH adapters:
//! - `LocalStorage`: bytes come from disk
//! - `S3Storage` (future): bytes come from S3 via the same
//!   `Storage::get` port
//!
//! In both cases the URL is HMAC-signed by the API when it
//! was handed out (see [`crate::signed_url`]). The frontend
//! only ever sees signed URLs; an unsigned request hits 403
//! at `verify()` and never reaches the adapter.

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

/// Query params expected on every `/files/*` request.
/// Both are required (the wire contract is "no anon fetch").
#[derive(Debug, Deserialize)]
pub struct SignedUrlParams {
    /// Unix-epoch second at which this URL stops verifying.
    pub exp: Option<u64>,
    /// URL-safe base64 HMAC-SHA256 of `"{storage_key}|{exp}"`.
    pub sig: Option<String>,
}

/// Axum handler for `GET /files/{*rest_of_path}`.
///
/// Path traversal: `axum` decodes the URL segment into
/// `Path<String>` already percent-decoded — but the wild
/// segment can still contain `%2F` (URL-encoded `/`). The
/// adapter's own `resolve()` rejects absolute / `..`
/// segments as a defense-in-depth.
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
        // 403 (NOT 401) — the request was authenticated but
        // the credential (signature) was invalid. Same code
        // covers "expired", "tampered", "missing-secret".
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

/// ponytail: cheap extension sniff — no MIME sniffing of the
/// payload (a vector of opens with EF BB BF would be wrong for
/// every format we ship). Every uploaded blob is `.webp` today,
/// but the dispatcher returns a generic octet-stream for
/// anything else so a future uploader that emits `.heic` etc.
/// doesn't crash the browser with a wrong type.
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
    // INFO (not WARN) — unsigned requests are expected to
    // happen (stale browser history, leaked URLs). Forensics
    // log lives at debug level.
    tracing::debug!(reason = reason, "signed-url fetch rejected");
    (StatusCode::FORBIDDEN, "forbidden").into_response()
}
