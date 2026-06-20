//! i18n (M7 + M11).
//!
//! M7 brought the `rust-i18n` plumbing: a static catalog embedded at
//! compile time from `crates/common/locales/{en,th,lo}.yml` and a
//! task-local locale that the `t!` macro reads. M11 layers on top:
//!
//! 1. **Per-tenant DB override** — an optional
//!    [`kokkak_domain::traits::translation::TranslationRepository`]
//!    injected at startup. When a `(locale, key)` lookup hits a row
//!    in the override store, that value is used; otherwise we fall
//!    through to the embedded catalog.
//!
//! 2. **Single entry point** — [`tr_with_repo`] takes the repo +
//!    locale + key + positional args and returns the final string.
//!    The async signature is the only practical shape: the DB
//!    lookup is an IO hop, and handlers are already async.
//!
//! 3. **Locale detection** — [`detect_locale`] resolves the
//!    priority: `?lang=` query > `Accept-Language` header >
//!    caller-supplied default. The locale middleware
//!    (`kokkak_api::middleware::i18n`) calls this and sets the
//!    task-local locale before the handler runs, so the
//!    `rust_i18n::t!` macro and the override repo both resolve
//!    against the same locale.
//!
//! ## Argument substitution
//!
//! The catalog uses positional placeholders (`{0}`, `{1}`, ...).
//! [`substitute`] walks the template once, leaving any
//! `{{` and `}}` alone (so translators can emit literal braces
//! when needed). The substitution is a one-pass char walker —
//! no regex, no second allocation unless the template actually
//! has placeholders.

use std::sync::OnceLock;

use kokkak_domain::traits::translation::{TranslationError, TranslationRepository};

// NOTE: `rust_i18n::i18n!("locales", ...)` must be invoked at the
// crate root so the generated `_rust_i18n_t` is reachable from
// `rust_i18n::t!` in any module. The init lives in `lib.rs` — see
// that file.

static INIT: OnceLock<()> = OnceLock::new();

/// Initialize the i18n catalog (call once from main). Subsequent
/// calls are no-ops. The `default_locale` is the fallback used
/// when a request omits `Accept-Language` and `?lang=`.
pub fn init_i18n(default_locale: &str) {
    INIT.get_or_init(|| {
        rust_i18n::set_locale(default_locale);
    });
}

/// Set the current locale for the calling task. The value
/// sticks for the duration of the task (axum runs each request
/// on its own task), so this is the right place to scope the
/// locale per request.
pub fn set_locale(locale: &str) {
    rust_i18n::set_locale(locale);
}

/// Get the current task-local locale.
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

    /// Return the locale as a `&str` (e.g. `"en"`, `"th"`, `"lo"`).
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for Locale {
    fn default() -> Self {
        Locale("en".to_string())
    }
}

/// Resolve locale priority:
///
/// 1. `?lang=` query (if present and recognized)
/// 2. `Accept-Language` header (if present and recognized)
/// 3. `default` argument (typically `"en"` or the user's profile locale)
///
/// The check for "recognized" is a closed allowlist (`th` | `en`
/// | `lo`) so a malformed header can't smuggle an arbitrary
/// string into the translation path.
pub fn detect_locale(query_lang: Option<&str>, accept_lang: Option<&str>, default: &str) -> String {
    if let Some(q) = query_lang {
        let primary = q.split('-').next().unwrap_or("").to_lowercase();
        if matches!(primary.as_str(), "th" | "en" | "lo") {
            return primary;
        }
    }
    if let Some(h) = accept_lang {
        let parsed = Locale::from_header(h).0;
        if !parsed.is_empty() {
            return parsed;
        }
    }
    default.to_string()
}

/// Substitute `{0}`, `{1}`, ... placeholders in `template` with
/// `args`. Escaped braces (`{{`, `}}`) emit a literal `{` / `}`.
/// Unknown placeholders are left as-is so a typo doesn't silently
/// swallow data.
pub fn substitute(template: &str, args: &[&str]) -> String {
    let mut out = String::with_capacity(template.len());
    let mut chars = template.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '{' {
            // Escaped brace: `{{` -> `{`.
            if chars.peek() == Some(&'{') {
                chars.next();
                out.push('{');
                continue;
            }
            // Positional placeholder: read digits up to `}`.
            let mut digits = String::new();
            while let Some(&d) = chars.peek() {
                if d == '}' {
                    break;
                }
                if d.is_ascii_digit() {
                    digits.push(d);
                    chars.next();
                } else {
                    break;
                }
            }
            if chars.peek() == Some(&'}') {
                chars.next();
                if let Ok(idx) = digits.parse::<usize>() {
                    if let Some(arg) = args.get(idx) {
                        out.push_str(arg);
                        continue;
                    }
                }
                // Unknown / out-of-range placeholder: emit verbatim.
                out.push('{');
                out.push_str(&digits);
                out.push('}');
            } else {
                // No closing brace — push the leading `{` and the
                // digits we already consumed so we don't lose data.
                out.push('{');
                out.push_str(&digits);
            }
        } else if c == '}' {
            // Escaped brace: `}}` -> `}`.
            if chars.peek() == Some(&'}') {
                chars.next();
                out.push('}');
            } else {
                out.push('}');
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Render a localized string with a per-tenant DB override and
/// a file-based fallback.
///
/// Resolution order:
/// 1. `repo.get(locale, key)` — if `Some(value)`, substitute
///    placeholders and return.
/// 2. File catalog — `rust_i18n::t!(key, locale = locale)` —
///    substitute placeholders and return.
/// 3. Key fallback — return the key itself wrapped in angle
///    brackets (`<key>`) so a missing translation is visible
///    in the response rather than silently rendering the key.
///
/// Step 3 matters because `rust_i18n` returns the key verbatim
/// when the key is unknown; emitting `<key>` instead makes the
/// gap obvious to the API consumer and to log scrapers.
pub async fn tr_with_repo<R>(repo: &R, locale: &str, key: &str, args: &[&str]) -> String
where
    R: TranslationRepository + ?Sized,
{
    if let Ok(Some(custom)) = repo.get(locale, key).await {
        return substitute(&custom, args);
    }
    tr(key, locale, args)
}

/// File-based translation (no DB). Mirrors `tr_with_repo` but
/// skips the repo lookup. Useful for logs and tests.
pub fn tr(key: &str, locale: &str, args: &[&str]) -> String {
    let raw = rust_i18n::t!(key, locale = locale).to_string();
    if raw == key {
        // rust_i18n returns the key unchanged when missing —
        // wrap it so the gap is visible.
        return format!("<{key}>");
    }
    substitute(&raw, args)
}

/// Convenience: build a [`TranslationError::Backend`] with a
/// caller-supplied context string. Used by adapters that need
/// to translate driver errors into the port's vocabulary.
pub fn backend_err(ctx: impl Into<String>) -> TranslationError {
    TranslationError::Backend(ctx.into())
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

    #[test]
    fn detect_locale_priority_query_over_header() {
        assert_eq!(detect_locale(Some("lo"), Some("th,en;q=0.5"), "en"), "lo");
    }

    #[test]
    fn detect_locale_falls_back_to_header() {
        assert_eq!(detect_locale(None, Some("th,en;q=0.5"), "en"), "th");
    }

    #[test]
    fn detect_locale_falls_back_to_default() {
        assert_eq!(detect_locale(None, None, "lo"), "lo");
    }

    #[test]
    fn detect_locale_rejects_unknown_query() {
        assert_eq!(detect_locale(Some("zz"), Some("th"), "en"), "th");
    }

    #[test]
    fn substitute_replaces_positional_placeholders() {
        assert_eq!(substitute("hello {0} {1}", &["a", "b"]), "hello a b");
    }

    #[test]
    fn substitute_passes_through_unknown_placeholder() {
        // Out-of-range index is left as `{N}` so a typo surfaces.
        assert_eq!(substitute("x {0} y {5}", &["a"]), "x a y {5}");
    }

    #[test]
    fn substitute_escapes_double_braces() {
        assert_eq!(
            substitute("set {{name}} = {0}", &["alice"]),
            "set {name} = alice"
        );
    }

    #[test]
    fn substitute_no_args_is_noop() {
        assert_eq!(substitute("plain", &[]), "plain");
    }

    #[test]
    fn tr_returns_bracketed_key_when_missing() {
        assert_eq!(tr("no.such.key", "en", &[]), "<no.such.key>");
    }

    #[test]
    fn tr_renders_known_key_in_default_locale() {
        // The English file is always embedded.
        let out = tr("err_repo.not_found", "en", &["widget x"]);
        assert!(out.contains("widget x"));
    }
}
