

#![deny(unsafe_code)]
#![warn(missing_docs)]

rust_i18n::i18n!("locales", fallback = "en");

pub mod config;
pub mod error;
pub mod error_codes;
pub mod i18n;
pub mod response;
pub mod telemetry;

pub use config::{
    ConfigError, Environment, LogFormat, LogSettings, ServerSettings, Settings, TlsSettings,
};
pub use error::{ApiErrorBody, AppError};
pub use i18n::{detect_locale, init_i18n, set_locale, substitute, tr, tr_with_repo, Locale};
pub use response::{created, ok, paginated, ApiResponse, PageMeta};
