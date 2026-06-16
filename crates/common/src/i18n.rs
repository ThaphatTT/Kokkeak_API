//! i18n (M7) — minimal rust-i18n wrapper.
//!
//! Per AGENTS.md § 13, the API serves three locales: `th`, `en`,
//! `lo`. The catalog lives in `locales/{th,en,lo}.yml`; the
//! `rust_i18n::t!` macro reads the message for the current
//! task-local locale. Handlers that need a non-default locale set
//! it via [`set_locale`] before resolving.
//!
//! AGENTS.md says "do not hardcode UI strings" — the goal here is to
//! have the plumbing in place; per-handler `rust_i18n::t!("order.created")`
//! is an opt-in change for the next pass.

use std::sync::OnceLock;

rust_i18n::i18n!("locales", fallback = "en");

static INIT: OnceLock<()> = OnceLock::new();

/// Initialize the i18n catalog (call once from main). Subsequent
/// calls are no-ops.
pub fn init_i18n(default_locale: &str) {
    INIT.get_or_init(|| {
        rust_i18n::set_locale(default_locale);
    });
}

/// Set the current locale for the calling task.
pub fn set_locale(locale: &str) {
    rust_i18n::set_locale(locale);
}

/// Get the current locale.
pub fn current_locale() -> String {
    rust_i18n::locale().to_string()
}

/// Per-request locale — wraps the parsed `Accept-Language` value.
#[derive(Debug, Clone)]
pub struct Locale(pub String);

impl Locale {
    /// Parse `Accept-Language` (e.g. `"th,en-US;q=0.7,en;q=0.5"`) and
    /// pick the first supported locale (`th` / `en` / `lo`).
    pub fn from_header(value: &str) -> Self {
        for raw in value.split(',') {
            let tag = raw.split(';').next().unwrap_or("").trim();
            let primary = tag.split('-').next().unwrap_or("").to_lowercase();
            if matches!(primary.as_str(), "th" | "en" | "lo") {
                return Locale(primary);
            }
        }
        Locale("en".to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for Locale {
    fn default() -> Self {
        Locale("en".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_thai_only() {
        let l = Locale::from_header("th");
        assert_eq!(l.as_str(), "th");
    }

    #[test]
    fn parse_thai_with_qvalues() {
        let l = Locale::from_header("th,en-US;q=0.7,en;q=0.5");
        assert_eq!(l.as_str(), "th");
    }

    #[test]
    fn parse_unknown_falls_back_to_en() {
        let l = Locale::from_header("fr,de;q=0.9");
        assert_eq!(l.as_str(), "en");
    }

    #[test]
    fn parse_la_explicit() {
        let l = Locale::from_header("lo");
        assert_eq!(l.as_str(), "lo");
    }
}
