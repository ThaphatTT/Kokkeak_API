//! Cert file watcher (T-12) — auto-reload for Let's Encrypt 90-day rotation.
//!
//! Wraps the [`notify`] crate to watch the configured cert + key
//! paths for modifications. On every detected change the watcher
//! pushes a signal through a `tokio::sync::watch` channel; the
//! caller is responsible for whatever action is appropriate for
//! the deployment (graceful shutdown under systemd/k8s, log-only,
//! etc.).
//!
//! Two public surfaces:
//! - [`watch_cert_files`] — start watching, return watcher handle
//!   plus receiver. The handle must be kept alive for the watcher
//!   to keep firing; dropping it cancels the OS subscription.
//! - [`cert_fingerprint`] — cheap content fingerprint for change
//!   detection and operator-visible diff logs.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::Path;

use anyhow::{Context, Result};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::watch;

/// Watch `cert_path` and `key_path` for modifications.
///
/// The returned [`RecommendedWatcher`] MUST be kept alive — drop
/// it and the OS subscription goes away. The simplest pattern is
/// to move it into the spawned task that consumes the receiver:
///
/// ```no_run
/// # use std::path::Path;
/// # use kokkak_api::cert_watcher::watch_cert_files;
/// # async fn doc(cert: &Path, key: &Path) -> anyhow::Result<()> {
/// let (watcher, mut rx) = watch_cert_files(cert, key)?;
/// tokio::spawn(async move {
///     let _watcher = watcher; // keep alive for task lifetime
///     while rx.changed().await.is_ok() {
///         // do something with the change signal
///     }
/// });
/// # Ok(())
/// # }
/// ```
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
            // `tokio::sync::watch::Sender::send` is non-blocking and
            // thread-safe — safe to call from the notify worker thread.
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

/// Cheap content fingerprint for change detection and operator
/// logs. SipHash of the file bytes (non-crypto — we only need
/// collision-free change detection, not tamper resistance).
///
/// Two snapshots of the same file produce the same fingerprint;
/// any byte change produces a different one (modulo SipHash
/// collisions, which are vanishingly rare for our use case).
pub fn cert_fingerprint(path: &Path) -> Result<String> {
    let bytes =
        std::fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    let mut h = DefaultHasher::new();
    bytes.len().hash(&mut h);
    bytes.hash(&mut h);
    Ok(format!("{:016x}-{}", h.finish(), bytes.len()))
}

/// Compare paths component-wise. Avoids pulling in the `pathdiff`
/// crate just for one equality check; the notify callback receives
/// canonical paths on Linux but may receive different cases on
/// macOS/Windows depending on the filesystem, so we do a case
/// insensitive comparison on non-Unix.
fn paths_match(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    #[cfg(not(unix))]
    {
        // On Windows / macOS, compare case-insensitively because
        // notify may surface paths in different cases than the
        // operator configured. `to_ascii_lowercase` is sufficient
        // for our purposes (PEM file paths are ASCII).
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

    // ---- Fingerprint ----

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
        // Length suffix also reflects the size delta.
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

    // ---- Watcher ----

    #[tokio::test]
    async fn watcher_signals_on_file_modification() {
        // We don't need a real PEM cert here — the watcher is
        // content-agnostic. A plain text file exercises the same
        // notify path.
        let path = scratch_file("watch-modify");
        std::fs::write(&path, b"v1").unwrap();

        let (_watcher, mut rx) = watch_cert_files(&path, &path).expect("watcher must start");

        // Drain the initial false value so `changed()` awaits the
        // next true signal.
        rx.borrow_and_update();

        // Modify the file. notify needs a moment to surface the event.
        std::fs::write(&path, b"v2").unwrap();

        // Poll for up to 2 seconds. notify on Windows can be
        // sluggish; on Linux/macOS it's sub-millisecond.
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
