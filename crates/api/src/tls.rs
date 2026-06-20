//! TLS (HTTPS) bootstrap for the API server (T-09, T-19).
//!
//! Pure helpers — no I/O beyond reading the PEM files on disk the
//! operator configured. The single public surface is
//! [`build_rustls_config`], which the binary's `main` calls when
//! [`kokkak_common::TlsSettings::enabled`] is `true`.
//!
//! Kept in a separate module so it can be unit-tested without
//! spinning up the axum router.

use std::path::Path;
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use axum_server::tls_rustls::RustlsConfig;
use tower_http::set_header::SetResponseHeaderLayer;

/// Build a [`RustlsConfig`] from a PEM-encoded certificate chain
/// and private key on disk.
///
/// Errors carry the offending path so the operator can fix the
/// deployment without grepping for the cause:
///
/// - I/O error reading either file → file path included.
/// - PEM parse error → file path included.
/// - PEM file contains zero certificates → `cert_path` included.
/// - PEM file contains zero private keys → `key_path` included.
/// - `rustls` rejects the chain/key (e.g. unsupported algorithm).
pub fn build_rustls_config(cert_path: &Path, key_path: &Path) -> Result<RustlsConfig> {
    let cert_pem = std::fs::read(cert_path).with_context(|| {
        format!(
            "failed to read TLS certificate from {}",
            cert_path.display()
        )
    })?;
    let key_pem = std::fs::read(key_path)
        .with_context(|| format!("failed to read TLS private key from {}", key_path.display()))?;

    // Detect empty key files up-front. Without this, a blank `key.pem`
    // would only surface as a "no private key found" error *after* the
    // cert file parsed cleanly — but if the cert file is also malformed
    // (e.g. an operator uploaded the wrong file), the cert check would
    // fire first and the empty-key condition would be masked. Checking
    // emptiness here makes both empty-file errors reachable
    // independently and produces symmetric diagnostics.
    if key_pem.is_empty() {
        return Err(anyhow!(
            "no private key found in {} (file is empty)",
            key_path.display()
        ));
    }

    // rustls-pemfile returns an iterator of `Result<CertificateData, _>`;
    // collecting surfaces every parse error with its position in the
    // file rather than failing on the first byte.
    let certs: Vec<_> = rustls_pemfile::certs(&mut cert_pem.as_slice())
        .collect::<Result<Vec<_>, _>>()
        .with_context(|| {
            format!(
                "failed to parse PEM certificates in {}",
                cert_path.display()
            )
        })?;
    if certs.is_empty() {
        return Err(anyhow!("no certificates found in {}", cert_path.display()));
    }

    let key = rustls_pemfile::private_key(&mut key_pem.as_slice())
        .with_context(|| format!("failed to parse PEM private key in {}", key_path.display()))?
        .ok_or_else(|| anyhow!("no private key found in {}", key_path.display()))?;

    let mut server_config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .context("rustls rejected the certificate chain / private key pair")?;

    // T-19: advertise both h2 and http/1.1 via ALPN so the TLS
    // handshake selects the right protocol. h2 brings connection
    // multiplexing (lower latency for mobile clients on flaky
    // networks) and header compression; we keep http/1.1 in the
    // list because not every client speaks h2 (curl <7.43, some
    // legacy SDKs). The order matters only for the server's
    // preference — most clients ignore it and pick by capability.
    server_config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];

    // axum-server 0.7 takes ownership of an Arc<ServerConfig> so
    // the same config can be shared across workers without a
    // clone of the (relatively expensive) rustls state.
    Ok(RustlsConfig::from_config(Arc::new(server_config)))
}

/// Build the HSTS middleware layer for the TLS-enabled path
/// (T-10). When `max_age_secs == 0` we return `None` so the
/// caller can skip applying the layer — adding
/// `Strict-Transport-Security: max-age=0` would actively tell
/// browsers to drop cached HSTS, which is the wrong default for
/// a fresh deployment.
///
/// The header value uses the `max-age` directive only. We do
/// NOT add `; includeSubDomains` by default — enabling that
/// without auditing every subdomain (staging, dev, internal
/// admin) is a footgun. Operators who need it can layer a
/// second `SetResponseHeaderLayer` over the same response.
pub fn hsts_layer(max_age_secs: u64) -> Option<SetResponseHeaderLayer<axum::http::HeaderValue>> {
    if max_age_secs == 0 {
        return None;
    }
    let value = format!("max-age={max_age_secs}");
    // `max-age=<number>` is always valid HeaderValue syntax —
    // the only ASCII chars involved are digits and the hyphen.
    let header =
        axum::http::HeaderValue::from_str(&value).expect("max-age header value is always valid");
    Some(SetResponseHeaderLayer::if_not_present(
        axum::http::header::STRICT_TRANSPORT_SECURITY,
        header,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// Per-test unique scratch dir under the OS temp dir. We avoid
    /// pulling in `tempfile` as a dev-dep just for four I/O
    /// tests; the harness is allowed to leave the dirs behind
    /// (they live in `std::env::temp_dir()` which the OS reclaims).
    static SEQ: AtomicU64 = AtomicU64::new(0);

    fn scratch_dir(tag: &str) -> std::path::PathBuf {
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let dir = std::env::temp_dir().join(format!("kokkak-tls-test-{tag}-{pid}-{n}"));
        std::fs::create_dir_all(&dir).expect("create scratch dir");
        dir
    }

    /// Build a self-signed cert + key pair at `tempdir/cert.pem`
    /// and `tempdir/key.pem` using `rcgen` would add a dev-dep;
    /// instead, the tests below exercise the I/O + empty-content
    /// paths (which is what most production incidents look like:
    /// wrong path, missing file, blank file) and skip the
    /// round-trip with a real cert. A live integration test is
    /// added in T-10 once the redirect server is in place.

    #[test]
    fn missing_cert_file_returns_io_error_with_path() {
        let dir = scratch_dir("missing-cert");
        let cert = dir.join("cert.pem");
        let key = dir.join("key.pem");
        std::fs::write(&key, b"placeholder").expect("write key");

        let err = build_rustls_config(&cert, &key).expect_err("missing cert must error");
        let msg = format!("{err:#}");
        assert!(
            msg.contains("certificate") && msg.contains("cert.pem"),
            "error must mention the cert path, got: {msg}"
        );
    }

    #[test]
    fn missing_key_file_returns_io_error_with_path() {
        let dir = scratch_dir("missing-key");
        let cert = dir.join("cert.pem");
        let key = dir.join("key.pem");
        std::fs::write(&cert, b"placeholder").expect("write cert");

        let err = build_rustls_config(&cert, &key).expect_err("missing key must error");
        let msg = format!("{err:#}");
        assert!(
            msg.contains("private key") && msg.contains("key.pem"),
            "error must mention the key path, got: {msg}"
        );
    }

    #[test]
    fn empty_cert_file_returns_parse_error() {
        let dir = scratch_dir("empty-cert");
        let cert = dir.join("cert.pem");
        let key = dir.join("key.pem");
        std::fs::write(&cert, b"").expect("write empty cert");
        std::fs::write(&key, b"placeholder").expect("write key");

        let err = build_rustls_config(&cert, &key).expect_err("empty cert must error");
        let msg = format!("{err:#}");
        assert!(
            msg.contains("no certificates") || msg.contains("parse"),
            "error should mention empty cert content, got: {msg}"
        );
    }

    #[test]
    fn empty_key_file_returns_parse_error() {
        let dir = scratch_dir("empty-key");
        let cert = dir.join("cert.pem");
        let key = dir.join("key.pem");
        std::fs::write(&cert, b"placeholder").expect("write cert");
        std::fs::write(&key, b"").expect("write empty key");

        let err = build_rustls_config(&cert, &key).expect_err("empty key must error");
        let msg = format!("{err:#}");
        assert!(
            msg.contains("no private key") || msg.contains("parse"),
            "error should mention empty key content, got: {msg}"
        );
    }

    // ---- T-10: HSTS layer ----

    #[test]
    fn hsts_layer_returns_none_for_zero_max_age() {
        // max-age=0 would tell browsers to drop cached HSTS;
        // we refuse to emit that header.
        assert!(hsts_layer(0).is_none());
    }

    #[test]
    fn hsts_layer_returns_some_for_positive_max_age() {
        let layer = hsts_layer(31_536_000).expect("non-zero must produce a layer");
        // Smoke-test that the layer is constructible; the
        // header value is internal so we just verify the layer
        // is non-trivially built (no panic, no error).
        let _ = format!("{:?}", layer);
    }
}
