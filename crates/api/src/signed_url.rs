

use std::time::{SystemTime, UNIX_EPOCH};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use hmac::{Hmac, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

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

        assert!(
            url.starts_with("https://api.example.com/files/users/u-1/profile/uuid.webp?exp="),
            "url shape wrong: {url}"
        );
        assert!(url.contains("&sig="), "url missing sig: {url}");

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

        let tampered = if sig.starts_with('A') {
            format!("B{}", &sig[1..])
        } else {
            format!("A{}", &sig[1..])
        };
        assert!(!verify(SECRET, "k", exp, &tampered));
    }

    #[test]
    fn rejects_expired() {

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

        let a = sign(SECRET, "k", 1000);
        let b = sign(SECRET, "k", 2000);
        assert_ne!(a, b);
    }
}
