

use axum::{
    extract::{Query, Request},
    http::header,
    middleware::Next,
    response::Response,
};
use kokkak_common::i18n::{detect_locale, set_locale};
use serde::Deserialize;

#[derive(Debug, Deserialize, Default, Clone)]
pub struct LocaleQuery {

    pub lang: Option<String>,
}

pub async fn locale_middleware(Query(q): Query<LocaleQuery>, req: Request, next: Next) -> Response {
    let accept_lang = req
        .headers()
        .get(header::ACCEPT_LANGUAGE)
        .and_then(|v| v.to_str().ok());
    let locale = detect_locale(q.lang.as_deref(), accept_lang, "en");
    set_locale(&locale);
    next.run(req).await
}

pub fn resolve_locale_for(req: &Request) -> String {
    let query_lang = req
        .extensions()
        .get::<LocaleQuery>()
        .and_then(|q| q.lang.clone());
    let accept_lang = req
        .headers()
        .get(header::ACCEPT_LANGUAGE)
        .and_then(|v| v.to_str().ok());
    detect_locale(query_lang.as_deref(), accept_lang, "en")
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use std::collections::HashMap;

    fn build_request(query: Option<&str>, accept: Option<&str>) -> Request<Body> {
        let uri = match query {
            Some(q) => format!("/api/v1/x?{q}"),
            None => "/api/v1/x".to_string(),
        };
        let mut b = Request::builder().uri(uri);
        if let Some(a) = accept {
            b = b.header(header::ACCEPT_LANGUAGE, a);
        }
        b.body(Body::empty()).unwrap()
    }

    #[test]
    fn accept_language_only() {
        let req = build_request(None, Some("th,en;q=0.5"));
        assert_eq!(resolve_locale_for(&req), "th");
    }

    #[test]
    fn unknown_header_falls_back_to_en() {
        let req = build_request(None, Some("fr,de;q=0.9"));
        assert_eq!(resolve_locale_for(&req), "en");
    }

    #[test]
    fn missing_everything_falls_back_to_en() {
        let req = build_request(None, None);
        assert_eq!(resolve_locale_for(&req), "en");
    }

    #[test]
    fn query_takes_priority_over_header() {
        let mut req = build_request(Some("lang=lo"), Some("th,en;q=0.5"));
        req.extensions_mut().insert(LocaleQuery {
            lang: Some("lo".into()),
        });
        assert_eq!(resolve_locale_for(&req), "lo");
    }

    #[test]
    fn invalid_query_falls_through_to_header() {
        let mut req = build_request(Some("lang=zz"), Some("th"));
        req.extensions_mut().insert(LocaleQuery {
            lang: Some("zz".into()),
        });
        assert_eq!(resolve_locale_for(&req), "th");
    }

    #[test]
    fn supported_locales_are_th_en_lo_zh() {
        let supported: HashMap<&str, ()> =
            ["th", "en", "lo", "zh"].iter().map(|s| (*s, ())).collect();
        assert!(supported.contains_key("th"));
        assert!(supported.contains_key("en"));
        assert!(supported.contains_key("lo"));
        assert!(supported.contains_key("zh"));
        assert!(!supported.contains_key("zz"));
    }
}
