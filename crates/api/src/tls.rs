

use std::path::Path;
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use axum_server::tls_rustls::RustlsConfig;
use tower_http::set_header::SetResponseHeaderLayer;

pub fn build_rustls_config(cert_path: &Path, key_path: &Path) -> Result<RustlsConfig> {
    let cert_pem = std::fs::read(cert_path).with_context(|| {
        format!(
            "failed to read TLS certificate from {}",
            cert_path.display()
        )
    })?;
    let key_pem = std::fs::read(key_path)
        .with_context(|| format!("failed to read TLS private key from {}", key_path.display()))?;

    if key_pem.is_empty() {
        return Err(anyhow!(
            "no private key found in {} (file is empty)",
            key_path.display()
        ));
    }

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

    server_config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];

    Ok(RustlsConfig::from_config(Arc::new(server_config)))
}

pub fn hsts_layer(max_age_secs: u64) -> Option<SetResponseHeaderLayer<axum::http::HeaderValue>> {
    if max_age_secs == 0 {
        return None;
    }
    let value = format!("max-age={max_age_secs}");

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

    static SEQ: AtomicU64 = AtomicU64::new(0);

    fn scratch_dir(tag: &str) -> std::path::PathBuf {
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let dir = std::env::temp_dir().join(format!("kokkak-tls-test-{tag}-{pid}-{n}"));
        std::fs::create_dir_all(&dir).expect("create scratch dir");
        dir
    }

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

    #[test]
    fn hsts_layer_returns_none_for_zero_max_age() {

        assert!(hsts_layer(0).is_none());
    }

    #[test]
    fn hsts_layer_returns_some_for_positive_max_age() {
        let layer = hsts_layer(31_536_000).expect("non-zero must produce a layer");

        let _ = format!("{:?}", layer);
    }
}
