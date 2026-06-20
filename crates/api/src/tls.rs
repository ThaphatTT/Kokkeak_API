//! TLS (HTTPS) bootstrap for the API server (T-09).
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

    let server_config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .context("rustls rejected the certificate chain / private key pair")?;

    // axum-server 0.7 takes ownership of an Arc<ServerConfig> so
    // the same config can be shared across workers without a
    // clone of the (relatively expensive) rustls state.
    Ok(RustlsConfig::from_config(Arc::new(server_config)))
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
}
