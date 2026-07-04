

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::Path;

use anyhow::{Context, Result};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::watch;

pub fn watch_cert_files(
    cert_path: &Path,
    key_path: &Path,
) -> Result<(RecommendedWatcher, watch::Receiver<bool>)> {
    let (tx, rx) = watch::channel(false);
    let cert_buf = cert_path.to_path_buf();
    let key_buf = key_path.to_path_buf();

    let mut watcher: RecommendedWatcher =
        notify::recommended_watcher(move |res: notify::Result<Event>| {
            let Ok(event) = res else { return };
            let interesting = matches!(
                event.kind,
                EventKind::Modify(_) | EventKind::Create(_) | EventKind::Remove(_)
            );
            if !interesting {
                return;
            }
            let touched = event
                .paths
                .iter()
                .any(|p| paths_match(p, &cert_buf) || paths_match(p, &key_buf));
            if !touched {
                return;
            }

            let _ = tx.send(true);
        })
        .context("failed to construct notify watcher")?;

    watcher
        .watch(cert_path, RecursiveMode::NonRecursive)
        .with_context(|| format!("failed to watch cert file {}", cert_path.display()))?;
    watcher
        .watch(key_path, RecursiveMode::NonRecursive)
        .with_context(|| format!("failed to watch key file {}", key_path.display()))?;

    Ok((watcher, rx))
}

pub fn cert_fingerprint(path: &Path) -> Result<String> {
    let bytes =
        std::fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    let mut h = DefaultHasher::new();
    bytes.len().hash(&mut h);
    bytes.hash(&mut h);
    Ok(format!("{:016x}-{}", h.finish(), bytes.len()))
}

fn paths_match(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    #[cfg(not(unix))]
    {

        let a_lc = a.to_string_lossy().to_ascii_lowercase();
        let b_lc = b.to_string_lossy().to_ascii_lowercase();
        if a_lc == b_lc {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static SEQ: AtomicU64 = AtomicU64::new(0);

    fn scratch_file(tag: &str) -> PathBuf {
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        std::env::temp_dir().join(format!("kokkak-cert-test-{tag}-{pid}-{n}.txt"))
    }

    #[test]
    fn fingerprint_is_stable_for_same_content() {
        let path = scratch_file("stable");
        std::fs::write(&path, b"hello world").unwrap();

        let fp1 = cert_fingerprint(&path).unwrap();
        let fp2 = cert_fingerprint(&path).unwrap();
        assert_eq!(fp1, fp2, "same content must produce the same fingerprint");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn fingerprint_changes_when_content_changes() {
        let path = scratch_file("changes");
        std::fs::write(&path, b"hello world").unwrap();
        let fp1 = cert_fingerprint(&path).unwrap();

        std::fs::write(&path, b"hello WORLD").unwrap();
        let fp2 = cert_fingerprint(&path).unwrap();

        assert_ne!(
            fp1, fp2,
            "different content must produce different fingerprint"
        );

        assert!(
            fp1.ends_with("-11") && fp2.ends_with("-11"),
            "size prefix encodes byte length (11 in both cases here): {fp1} vs {fp2}"
        );
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn fingerprint_includes_byte_length() {
        let path = scratch_file("len");
        std::fs::write(&path, b"a").unwrap();
        let fp1 = cert_fingerprint(&path).unwrap();
        std::fs::write(&path, b"ab").unwrap();
        let fp2 = cert_fingerprint(&path).unwrap();
        assert!(
            fp1.ends_with("-1") && fp2.ends_with("-2"),
            "length prefix must reflect byte size: {fp1} vs {fp2}"
        );
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn fingerprint_missing_file_returns_io_error() {
        let path = scratch_file("missing");
        let err = cert_fingerprint(&path).expect_err("missing file must error");
        assert!(
            err.to_string().contains("failed to read"),
            "error should mention the read failure, got: {err}"
        );
    }

    #[tokio::test]
    async fn watcher_signals_on_file_modification() {

        let path = scratch_file("watch-modify");
        std::fs::write(&path, b"v1").unwrap();

        let (_watcher, mut rx) = watch_cert_files(&path, &path).expect("watcher must start");

        rx.borrow_and_update();

        std::fs::write(&path, b"v2").unwrap();

        let got = tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                if *rx.borrow_and_update() {
                    return true;
                }
                rx.changed().await.ok();
            }
        })
        .await
        .unwrap_or(false);

        assert!(
            got,
            "watcher must signal true within 2s of file modification"
        );
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn paths_match_handles_exact_equality() {
        let a = PathBuf::from("/etc/kokkak/cert.pem");
        assert!(paths_match(&a, &a));
    }

    #[test]
    fn paths_match_rejects_unrelated_paths() {
        let a = PathBuf::from("/etc/kokkak/cert.pem");
        let b = PathBuf::from("/etc/kokkak/key.pem");
        assert!(!paths_match(&a, &b));
    }
}
