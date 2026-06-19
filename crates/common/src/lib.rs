//! Common layer
//!
//! Houses shared infrastructure used by every other crate:
//! error types, configuration loader, telemetry, response envelope,
//! and small utilities (UUID v7, time, decimal).
//!
//! See AGENTS.md § 3, 11, 12, 14 for the standards this layer enforces.

#![deny(unsafe_code)]
#![warn(missing_docs)]

// Initialize the i18n catalog at the crate root so the generated
// `_rust_i18n_t` is reachable from `rust_i18n::t!` in every module
// (including `i18n::tr`). `locales/` resolves relative to this
// crate's `Cargo.toml`.
rust_i18n::i18n!("locales", fallback = "en");

pub mod config;
pub mod error;
pub mod i18n;
pub mod response;
pub mod telemetry;

pub use config::{ConfigError, LogFormat, LogSettings, ServerSettings, Settings};
pub use error::{ApiErrorBody, AppError};
pub use i18n::{detect_locale, init_i18n, set_locale, substitute, tr, tr_with_repo, Locale};
pub use response::{created, ok, paginated, ApiResponse, PageMeta};
