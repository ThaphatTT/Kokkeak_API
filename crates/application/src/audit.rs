//! Audit logging port (`AuditLogger` + `AuditEvent`).
//!
//! Auth events flow through this port to a concrete sink (file,
//! database, SIEM forwarder, ...) so operators can investigate
//! incidents and the security team can correlate brute-force
//! attempts. The HTTP-facing response is unaffected — the audit
//! record is for the server side, not the client.
//!
//! Wire contract: every line written to the audit sink is a JSON
//! object with at minimum the `timestamp`, `event`, `username`,
//! `user_id`, `ip`, `user_agent`, and `reason` fields. The concrete
//! adapter may add fields (e.g. `request_id`, `trace_id`) but must
//! not remove the ones listed here.

use chrono::{DateTime, Utc};
use serde::Serialize;
use std::net::IpAddr;
use uuid::Uuid;

/// Structured audit record. Serialized to JSON by the concrete
/// adapter (one object per line / per row / per event).
#[derive(Debug, Clone, Serialize)]
pub struct AuditEvent {
    /// When the event happened (UTC).
    pub timestamp: DateTime<Utc>,

    /// Stable event name. SIEM dashboards pivot on these strings —
    /// do not rename without coordinating with the ops team.
    ///
    /// Current set:
    /// - `auth.register.success`
    /// - `auth.register.failure`
    /// - `auth.login.success`
    /// - `auth.login.failure`
    /// - `auth.refresh.success`
    /// - `auth.refresh.failure`
    /// - `auth.login.rate_limited`
    pub event: &'static str,

    /// Username involved (lowercased). May be absent for events that
    /// are not tied to a username (e.g. token-only operations).
    pub username: Option<String>,

    /// User ID involved (when known). `None` for events fired before
    /// the user was located (e.g. user-not-found failures).
    pub user_id: Option<Uuid>,

    /// Client IP address. Sourced from the connection (`ConnectInfo`)
    /// or the `X-Forwarded-For` header when behind a trusted proxy.
    pub ip: Option<IpAddr>,

    /// `User-Agent` header value. Useful for spotting credential
    /// stuffing from headless clients vs legit mobile traffic.
    pub user_agent: Option<String>,

    /// Specific failure reason for `*.failure` events:
    /// `user_not_found` | `wrong_password` | `account_suspended` |
    /// `account_deleted` | `account_pending` | `rate_limited` |
    /// `invalid_token` | `username_taken` | `validation` |
    /// `backend_error`.
    pub reason: Option<&'static str>,

    /// Free-form context (request id, scope, route, ...). Keep this
    /// small — the audit sink may be on slow storage.
    #[serde(skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub context: std::collections::BTreeMap<&'static str, String>,
}

impl AuditEvent {
    /// Build a new event with `timestamp = now(UTC)` and every other
    /// field `None` / empty. Callers then `.set_username(...)` etc.
    pub fn new(event: &'static str) -> Self {
        Self {
            timestamp: Utc::now(),
            event,
            username: None,
            user_id: None,
            ip: None,
            user_agent: None,
            reason: None,
            context: std::collections::BTreeMap::new(),
        }
    }

    /// Attach the username to the event. Accepts anything that
    /// converts into a `String` (typically `&str`).
    pub fn with_username(mut self, username: impl Into<String>) -> Self {
        self.username = Some(username.into());
        self
    }
    /// Attach the user id (UUID) to the event.
    pub fn with_user_id(mut self, id: Uuid) -> Self {
        self.user_id = Some(id);
        self
    }
    /// Attach the client IP. Use [`with_ip_opt`](Self::with_ip_opt)
    /// when the IP may be `None` (e.g. in-process callers).
    pub fn with_ip(mut self, ip: IpAddr) -> Self {
        self.ip = Some(ip);
        self
    }
    /// `Option`-friendly variant: skips when the IP is `None`. Lets
    /// call sites stay one-liner even when the IP comes from a
    /// maybe-present source (`ConnectInfo` may be absent in tests,
    /// behind some proxies, ...).
    pub fn with_ip_opt(mut self, ip: Option<IpAddr>) -> Self {
        self.ip = ip;
        self
    }
    /// Attach the `User-Agent` header value (useful for spotting
    /// credential stuffing from headless clients).
    pub fn with_user_agent(mut self, ua: impl Into<String>) -> Self {
        self.user_agent = Some(ua.into());
        self
    }
    /// Attach the structured failure reason (e.g. `wrong_password`,
    /// `account_suspended`, `rate_limited`). Stable contract; SIEM
    /// dashboards pivot on these strings.
    pub fn with_reason(mut self, reason: &'static str) -> Self {
        self.reason = Some(reason);
        self
    }
    /// Attach a free-form key/value pair to the event's `context`
    /// (e.g. `scope = "mobile"`, `retry_after_secs = "293"`).
    pub fn with_context(mut self, key: &'static str, value: impl Into<String>) -> Self {
        self.context.insert(key, value.into());
        self
    }
}

/// Audit sink port. Implementations are responsible for the
/// transport (file append, DB insert, HTTP forward) and any
/// required durability guarantees.
///
/// ponytail: the port only exposes `log(AuditEvent)` — adding a
/// `flush()` or `batch()` here would tempt callers to await I/O on
/// the request hot path. The default impl in `infra` does a best-
/// effort `flush()` after each write; that's good enough for audit
/// (lossy writes are tolerable) and keeps the call site zero-cost
/// in the sense of "fire and forget".
pub trait AuditLogger: Send + Sync {
    /// Persist the event. Best-effort: a write failure is logged
    /// at WARN and swallowed rather than propagated into the auth
    /// path (an audit-sink failure must never turn a 200 login into
    /// a 500).
    fn log(&self, event: AuditEvent);
}

/// Test-only `AuditLogger` that buffers events in memory so tests
/// can assert what was recorded.
#[derive(Default)]
pub struct TestAuditLogger {
    /// Captured events (in arrival order). Lock + push per write.
    pub events: std::sync::Mutex<Vec<AuditEvent>>,
}

impl AuditLogger for TestAuditLogger {
    fn log(&self, event: AuditEvent) {
        self.events.lock().unwrap().push(event);
    }
}

/// No-op `AuditLogger` used as a graceful-degradation fallback when
/// the real sink (file / DB / SIEM forwarder) can't be opened at
/// startup. Better to lose audit lines than to refuse to boot.
pub struct NoopAuditLogger;

impl AuditLogger for NoopAuditLogger {
    fn log(&self, _event: AuditEvent) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builder_sets_each_field_independently() {
        let e = AuditEvent::new("auth.login.failure")
            .with_username("alice")
            .with_ip("203.0.113.5".parse().unwrap())
            .with_reason("wrong_password")
            .with_context("scope", "mobile");
        assert_eq!(e.event, "auth.login.failure");
        assert_eq!(e.username.as_deref(), Some("alice"));
        assert_eq!(e.ip, Some("203.0.113.5".parse::<IpAddr>().unwrap()));
        assert_eq!(e.reason, Some("wrong_password"));
        assert_eq!(e.context.get("scope").map(String::as_str), Some("mobile"));
    }

    #[test]
    fn serializes_to_expected_field_names() {
        // The wire format is part of the SIEM contract. Locking
        // the field names here means a rename can't ship without
        // updating the test.
        let e = AuditEvent::new("auth.login.success").with_username("alice");
        let s = serde_json::to_string(&e).unwrap();
        assert!(s.contains("\"event\":\"auth.login.success\""));
        assert!(s.contains("\"username\":\"alice\""));
        assert!(s.contains("\"timestamp\""));
        assert!(s.contains("\"user_id\":null"));
    }
}
