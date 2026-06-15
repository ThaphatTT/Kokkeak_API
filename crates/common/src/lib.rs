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

pub use config::{ConfigError, LogFormat, LogSettings, ServerSettings, Settings};
