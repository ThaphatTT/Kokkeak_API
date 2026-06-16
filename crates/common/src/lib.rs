//! Common layer
//!
//! Houses shared infrastructure used by every other crate:
//! error types, configuration loader, telemetry, response envelope,
//! and small utilities (UUID v7, time, decimal).
//!
//! See AGENTS.md § 3, 11, 12, 14 for the standards this layer enforces.

#![deny(unsafe_code)]
#![warn(missing_docs)]

pub mod config;
pub mod error;
pub mod i18n;
pub mod response;
pub mod telemetry;

pub use config::{ConfigError, LogFormat, LogSettings, ServerSettings, Settings};
pub use error::{ApiErrorBody, AppError};
pub use i18n::{init_i18n, Locale};
pub use response::{created, ok, paginated, ApiResponse, PageMeta};
