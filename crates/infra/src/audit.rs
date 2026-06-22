//! `FileAuditLogger` — writes `AuditEvent`s as JSONL to a file on disk.
//!
//! One JSON object per line. Operators can `cat`, `grep`, `jq`, or pipe
//! into a SIEM forwarder. The file is opened in append mode; parent
//! directories are created on first use.
//!
//! ponytail: deliberately minimal — no rotation, no compression, no
//! async batching. The audit path is fire-and-forget; if we lose a
//! line on crash, the surrounding `tracing` event still survives.
//! Upgrade path for rotation:
//!   1. check size after each write
//!   2. on threshold, rename `auth-audit.jsonl` → `auth-audit.<ts>.jsonl`
//!   3. reopen
//!
//! OR hand off to `logrotate` (postrotate: `kill -USR1 <pid>` if we add
//! reopen signal support).

use std::error::Error as StdError;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use kokkak_application::audit::{AuditEvent, AuditLogger};

/// Boxed error used by the file-IO paths. Kept local (no `anyhow`
/// dep) so the `infra` crate stays free of `anyhow` and the error
/// type stays small + `Send + Sync + 'static`.
type BoxError = Box<dyn StdError + Send + Sync + 'static>;

/// Append-only JSONL audit logger. Thread-safe — `OpenOptions` +
/// `Mutex<File>` is enough for the expected throughput (login / refresh
/// is sub-ms work; one syscall per write).
///
/// Construction opens the file once. If the file or its parent does
/// not exist, the parent is created and the file is opened in
/// create+append mode.
pub struct FileAuditLogger {
    inner: Mutex<FileAuditInner>,
}

struct FileAuditInner {
    file: File,
    path: PathBuf,
}

impl FileAuditLogger {
    /// Open (or create) the file at `path` in append mode. Parent
    /// directories are created on demand. Returns an error if the
    /// file cannot be opened — the caller (composition root) should
    /// fail fast at startup rather than degrade silently.
    pub fn new(path: impl AsRef<Path>) -> Result<Self, BoxError> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    format!(
                        "failed to create audit-log parent directory {}: {e}",
                        parent.display()
                    )
                })?;
            }
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|e| format!("failed to open audit log file {}: {e}", path.display()))?;
        Ok(Self {
            inner: Mutex::new(FileAuditInner { file, path }),
        })
    }
}

impl AuditLogger for FileAuditLogger {
    fn log(&self, event: AuditEvent) {
        // Best-effort serialise: a failure here is logged at WARN
        // and swallowed — the auth path must not block on the audit
        // sink. The alternative — propagating an error into the
        // auth service — would mean a broken audit file causes 500s
        // on login, which is worse than a dropped audit line.
        let line = match serde_json::to_string(&event) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    event = event.event,
                    "audit: failed to serialise AuditEvent — line dropped",
                );
                return;
            }
        };
        let mut inner = match self.inner.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        match writeln!(inner.file, "{line}") {
            Ok(()) => {
                // Flush after each line so `tail -f` works in
                // real-time. Cost is one fsync per event — acceptable
                // for login/refresh volume; tighten only if profiling
                // shows it as a bottleneck.
                let _ = inner.file.flush();
            }
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    path = %inner.path.display(),
                    event = event.event,
                    "audit: failed to write to file — line dropped",
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kokkak_application::audit::AuditEvent;
    use std::net::IpAddr;

    fn tmp_path(name: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!(
            "kokkak-audit-{}-{}-{}.jsonl",
            name,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        // Best-effort cleanup of stale files from previous runs.
        let _ = std::fs::remove_file(&p);
        p
    }

    #[test]
    fn creates_parent_dir_and_appends_lines() {
        let dir = std::env::temp_dir().join(format!("kokkak-audit-nested-{}", std::process::id()));
        let path = dir.join("sub").join("audit.jsonl");
        let _ = std::fs::remove_dir_all(&dir);

        let logger = FileAuditLogger::new(&path).expect("open");
        logger.log(
            AuditEvent::new("auth.login.success")
                .with_username("alice")
                .with_ip("127.0.0.1".parse::<IpAddr>().unwrap()),
        );
        logger.log(
            AuditEvent::new("auth.login.failure")
                .with_username("alice")
                .with_reason("wrong_password"),
        );

        let body = std::fs::read_to_string(&path).expect("read");
        // Each line is independent JSON.
        let lines: Vec<&str> = body.lines().collect();
        assert_eq!(lines.len(), 2, "expected 2 lines, got: {body}");
        assert!(lines[0].contains("auth.login.success"));
        assert!(lines[0].contains("alice"));
        assert!(lines[1].contains("auth.login.failure"));
        assert!(lines[1].contains("wrong_password"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn reuses_existing_file_across_instances() {
        let path = tmp_path("reuse");
        {
            let logger = FileAuditLogger::new(&path).expect("open1");
            logger.log(AuditEvent::new("auth.login.success").with_username("a"));
        }
        {
            let logger = FileAuditLogger::new(&path).expect("open2");
            logger.log(AuditEvent::new("auth.login.success").with_username("b"));
        }
        let body = std::fs::read_to_string(&path).expect("read");
        assert_eq!(body.lines().count(), 2);
        assert!(body.contains("\"username\":\"a\""));
        assert!(body.contains("\"username\":\"b\""));

        let _ = std::fs::remove_file(&path);
    }
}
