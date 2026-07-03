//! HMAC-SHA256 signed `/files/*` URLs (T-23-b).
//!
//! Frontend contract is unchanged: take the URL from the API
//! response, paste it into `<img src=...>`. The URL the API hands
//! out carries a one-way HMAC signature that the same API
//! verifies on the matching `GET /files/*` request. Replay past
//! `exp` returns 403; tampering with the path or the signature
//! also returns 403; the secret never leaves the server.
//!
//! ## Wire format
//!
//! ```text
//! {base}/files/{storage_key}?exp={unix_seconds}&sig={url_safe_b64_hmac_sha256}
//! ```
//!
//! The HMAC payload is `"{storage_key}|{exp}"` — binding the
//! expiry to the path so an attacker can't extend a leaked URL
//! by replaying the same signature under a different exp (or
//! vice-versa).
//!
//! ponytail: HMAC-SHA256 + secret >=32 bytes satisfies the
//! AGENTS.md §21.7 NIST 800-175B requirement (one of SHA-2
//! family with key). Constant-time compare happens inside
//! `Hmac::verify_slice` (RustCrypto guarantee) so we don't
//! need a separate `subtle` import.

use std::time::{SystemTime, UNIX_EPOCH};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use hmac::{Hmac, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

/// Compose a signed `/files/*` URL. `None` when:
/// - `public_base_url` is empty
/// - `storage_key` is empty
/// - `secret` is empty
/// - `ttl_secs` is zero (or system clock is pre-epoch, which
///   would be a bug — keep the call site from panicking).
pub fn signed_image_url(
    public_base_url: &str,
    storage_key: &str,
    secret: &str,
    ttl_secs: u32,
) -> Option<String> {
    let base = public_base_url.trim().trim_end_matches('/');
    if base.is_empty() || storage_key.is_empty() || secret.is_empty() || ttl_secs == 0 {
        return None;
    }
    let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs();
    let exp = now.saturating_add(ttl_secs as u64);
    let sig = sign(secret.as_bytes(), storage_key, exp);
    Some(format!(
        "{}/files/{}?exp={}&sig={}",
        base, storage_key, exp, sig
    ))
}

/// Verify the query parameters on a `/files/*` request.
///
/// Returns `false` on:
/// - signature can't be decoded as URL-safe base64
/// - signature is wrong (constant-time mismatch)
/// - `exp` is in the past (or now-or-earlier).
pub fn verify(secret: &[u8], storage_key: &str, exp: u64, sig: &str) -> bool {
    let Ok(now) = SystemTime::now().duration_since(UNIX_EPOCH) else {
        return false;
    };
    if exp <= now.as_secs() {
        return false;
    }
    let Ok(sig_bytes) = URL_SAFE_NO_PAD.decode(sig) else {
        return false;
    };
    let Ok(mut mac) = HmacSha256::new_from_slice(secret) else {
        return false;
    };
    mac.update(storage_key.as_bytes());
    mac.update(b"|");
    mac.update(exp.to_string().as_bytes());
    // Hmac::verify_slice uses constant-time compare internally.
    mac.verify_slice(&sig_bytes).is_ok()
}

fn sign(secret: &[u8], storage_key: &str, exp: u64) -> String {
    let mut mac = HmacSha256::new_from_slice(secret)
        .expect("HMAC-SHA256 accepts any key length (NIST 800-175B)");
    mac.update(storage_key.as_bytes());
    mac.update(b"|");
    mac.update(exp.to_string().as_bytes());
    URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &[u8] = b"this-is-a-thirty-two-byte-test-secret-x";

    #[test]
    fn round_trip_within_ttl() {
        let url = signed_image_url(
            "https://api.example.com",
            "users/u-1/profile/uuid.webp",
            std::str::from_utf8(SECRET).unwrap(),
            600,
        )
        .expect("non-empty inputs");
        // Shape: <base>/files/<key>?exp=<N>&sig=<base64>
        assert!(
            url.starts_with("https://api.example.com/files/users/u-1/profile/uuid.webp?exp="),
            "url shape wrong: {url}"
        );
        assert!(url.contains("&sig="), "url missing sig: {url}");

        // Pull out exp + sig and verify.
        let q = url.rsplit_once('?').unwrap().1;
        let mut parts = q.split('&');
        let exp_str = parts.next().unwrap().strip_prefix("exp=").unwrap();
        let sig = parts.next().unwrap().strip_prefix("sig=").unwrap();
        let exp: u64 = exp_str.parse().unwrap();
        assert!(
            verify(SECRET, "users/u-1/profile/uuid.webp", exp, sig),
            "fresh signature must verify"
        );
    }

    #[test]
    fn empty_inputs_return_none() {
        assert!(signed_image_url("", "k", "secret", 600).is_none());
        assert!(signed_image_url("https://x", "", "secret", 600).is_none());
        assert!(signed_image_url("https://x", "k", "", 600).is_none());
        assert!(signed_image_url("https://x", "k", "secret", 0).is_none());
    }

    #[test]
    fn rejects_wrong_path() {
        let url = signed_image_url(
            "https://x",
            "users/a/profile/uuid.webp",
            std::str::from_utf8(SECRET).unwrap(),
            600,
        )
        .unwrap();
        let q = url.rsplit_once('?').unwrap().1;
        let mut parts = q.split('&');
        let exp: u64 = parts
            .next()
            .unwrap()
            .strip_prefix("exp=")
            .unwrap()
            .parse()
            .unwrap();
        let sig = parts.next().unwrap().strip_prefix("sig=").unwrap();
        assert!(!verify(SECRET, "users/a/profile/OTHER.webp", exp, sig));
    }

    #[test]
    fn rejects_tampered_signature() {
        let url =
            signed_image_url("https://x", "k", std::str::from_utf8(SECRET).unwrap(), 600).unwrap();
        let q = url.rsplit_once('?').unwrap().1;
        let mut parts = q.split('&');
        let exp: u64 = parts
            .next()
            .unwrap()
            .strip_prefix("exp=")
            .unwrap()
            .parse()
            .unwrap();
        let sig = parts.next().unwrap().strip_prefix("sig=").unwrap();
        // Flip the first char of the sig.
        let tampered = if sig.starts_with('A') {
            format!("B{}", &sig[1..])
        } else {
            format!("A{}", &sig[1..])
        };
        assert!(!verify(SECRET, "k", exp, &tampered));
    }

    #[test]
    fn rejects_expired() {
        // exp = 1s in the past is for sure expired.
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let exp_past = now.saturating_sub(10);
        let sig = sign(SECRET, "k", exp_past);
        assert!(!verify(SECRET, "k", exp_past, &sig));
    }

    #[test]
    fn rejects_non_base64_sig() {
        assert!(!verify(SECRET, "k", u64::MAX, "not!base64!"));
    }

    #[test]
    fn signature_differs_for_different_exp() {
        // The exp is part of the HMAC payload — bumping it
        // produces a fresh signature even with the same path.
        let a = sign(SECRET, "k", 1000);
        let b = sign(SECRET, "k", 2000);
        assert_ne!(a, b);
    }
}
