

use std::sync::OnceLock;

use kokkak_domain::traits::translation::{TranslationError, TranslationRepository};

static INIT: OnceLock<()> = OnceLock::new();

pub fn init_i18n(default_locale: &str) {
    INIT.get_or_init(|| {
        rust_i18n::set_locale(default_locale);
    });
}

pub fn set_locale(locale: &str) {
    rust_i18n::set_locale(locale);
}

pub fn current_locale() -> String {
    rust_i18n::locale().to_string()
}

#[derive(Debug, Clone)]
pub struct Locale(pub String);

impl Locale {

    pub fn from_header(value: &str) -> Self {
        for raw in value.split(',') {
            let tag = raw.split(';').next().unwrap_or("").trim();
            let primary = tag.split('-').next().unwrap_or("").to_lowercase();

            if matches!(primary.as_str(), "th" | "en" | "lo" | "zh") {
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

pub fn detect_locale(query_lang: Option<&str>, accept_lang: Option<&str>, default: &str) -> String {
    if let Some(q) = query_lang {
        let primary = q.split('-').next().unwrap_or("").to_lowercase();
        if matches!(primary.as_str(), "th" | "en" | "lo" | "zh") {
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

pub fn substitute(template: &str, args: &[&str]) -> String {
    let mut out = String::with_capacity(template.len());
    let mut chars = template.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '{' {

            if chars.peek() == Some(&'{') {
                chars.next();
                out.push('{');
                continue;
            }

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

                out.push('{');
                out.push_str(&digits);
                out.push('}');
            } else {

                out.push('{');
                out.push_str(&digits);
            }
        } else if c == '}' {

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

pub async fn tr_with_repo<R>(repo: &R, locale: &str, key: &str, args: &[&str]) -> String
where
    R: TranslationRepository + ?Sized,
{
    if let Ok(Some(custom)) = repo.get(locale, key).await {
        return substitute(&custom, args);
    }
    tr(key, locale, args)
}

pub fn tr(key: &str, locale: &str, args: &[&str]) -> String {
    let raw = rust_i18n::t!(key, locale = locale).to_string();
    if raw == key {

        return format!("<{key}>");
    }
    substitute(&raw, args)
}

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
    fn parse_zh_explicit() {

        let l = Locale::from_header("zh");
        assert_eq!(l.as_str(), "zh");

        let l = Locale::from_header("zh-CN,en;q=0.5");
        assert_eq!(l.as_str(), "zh");
    }

    #[test]
    fn zh_catalog_resolves_known_keys() {

        let cases: &[(&str, &str)] = &[
            ("err_auth.invalid_credentials", "用户名或密码错误"),
            ("err_auth.admin_required", "需要管理员权限"),
            ("err_auth.username_taken", "该用户名已被使用"),
            ("err_repo.not_found", "未找到"),
            ("err.bad_request", "请求无效"),
            ("err.rate_limited", "请求频率超限"),
        ];
        for (key, expected_fragment) in cases {
            let resolved = tr(key, "zh", &[]);
            assert!(
                resolved.contains(expected_fragment),
                "zh translation of {key} = {resolved:?}, expected fragment {expected_fragment:?}"
            );
            assert!(
                !resolved.starts_with('<'),
                "zh key {key} unresolved (rust_i18n returned the key verbatim)"
            );
        }
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

        let out = tr("err_repo.not_found", "en", &["widget x"]);
        assert!(out.contains("widget x"));
    }
}
