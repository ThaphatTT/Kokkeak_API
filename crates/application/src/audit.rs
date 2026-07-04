

use chrono::{DateTime, Utc};
use serde::Serialize;
use std::net::IpAddr;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
pub struct AuditEvent {

    pub timestamp: DateTime<Utc>,

    pub event: &'static str,

    pub username: Option<String>,

    pub user_id: Option<Uuid>,

    pub ip: Option<IpAddr>,

    pub user_agent: Option<String>,

    pub reason: Option<&'static str>,

    #[serde(skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub context: std::collections::BTreeMap<&'static str, String>,
}

impl AuditEvent {

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

    pub fn with_username(mut self, username: impl Into<String>) -> Self {
        self.username = Some(username.into());
        self
    }

    pub fn with_user_id(mut self, id: Uuid) -> Self {
        self.user_id = Some(id);
        self
    }

    pub fn with_ip(mut self, ip: IpAddr) -> Self {
        self.ip = Some(ip);
        self
    }

    pub fn with_ip_opt(mut self, ip: Option<IpAddr>) -> Self {
        self.ip = ip;
        self
    }

    pub fn with_user_agent(mut self, ua: impl Into<String>) -> Self {
        self.user_agent = Some(ua.into());
        self
    }

    pub fn with_reason(mut self, reason: &'static str) -> Self {
        self.reason = Some(reason);
        self
    }

    pub fn with_context(mut self, key: &'static str, value: impl Into<String>) -> Self {
        self.context.insert(key, value.into());
        self
    }
}

pub trait AuditLogger: Send + Sync {

    fn log(&self, event: AuditEvent);
}

#[derive(Default)]
pub struct TestAuditLogger {

    pub events: std::sync::Mutex<Vec<AuditEvent>>,
}

impl AuditLogger for TestAuditLogger {
    fn log(&self, event: AuditEvent) {
        self.events.lock().unwrap().push(event);
    }
}

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

        let e = AuditEvent::new("auth.login.success").with_username("alice");
        let s = serde_json::to_string(&e).unwrap();
        assert!(s.contains("\"event\":\"auth.login.success\""));
        assert!(s.contains("\"username\":\"alice\""));
        assert!(s.contains("\"timestamp\""));
        assert!(s.contains("\"user_id\":null"));
    }
}
