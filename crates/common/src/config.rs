//! Configuration loader.
//!
//! Loads [`Settings`] from environment variables (prefix `KOKKAK_`,
//! separator `__`) and validates them at startup. Process exits
//! fast on misconfiguration (fail-fast principle).
//!
//! ## Environment variable convention
//!
//! ```text
//! KOKKAK_<SECTION>__<KEY>=value
//! ```
//!
//! Examples:
//! - `KOKKAK_SERVER__ADDR=0.0.0.0:3000`
//! - `KOKKAK_SERVER__WORKERS=4`
//! - `KOKKAK_LOG__FORMAT=json`  (or `pretty`)

use figment::{providers::Env, Figment};
use serde::Deserialize;
use thiserror::Error;

/// Errors when loading or validating configuration.
#[derive(Debug, Error)]
pub enum ConfigError {
    /// Underlying figment provider error (missing/invalid env, parse error, etc.).
    /// Boxed to keep `ConfigError` small (clippy::result_large_err).
    #[error("config provider error: {0}")]
    Figment(#[from] Box<figment::Error>),

    /// Semantically invalid value: a specific setting failed a post-load check.
    #[error("invalid config: key={key}, {message}")]
    Invalid {
        /// The dotted config key (e.g. `"server.addr"`).
        key: String,
        /// Human-readable explanation of why the value is invalid.
        message: String,
    },
}

impl From<figment::Error> for ConfigError {
    fn from(err: figment::Error) -> Self {
        Self::Figment(Box::new(err))
    }
}

/// Top-level settings struct.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct Settings {
    /// HTTP server settings.
    #[serde(default)]
    pub server: ServerSettings,

    /// Logging settings.
    #[serde(default)]
    pub log: LogSettings,
}

/// HTTP server settings.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct ServerSettings {
    /// Bind address (e.g. `0.0.0.0:3000`).
    #[serde(default = "default_addr")]
    pub addr: String,

    /// Number of Tokio worker threads (currently informational — `axum::serve`
    /// runs on a single thread; multi-worker requires a process manager).
    #[serde(default = "default_workers")]
    pub workers: usize,
}

impl Default for ServerSettings {
    fn default() -> Self {
        Self {
            addr: default_addr(),
            workers: default_workers(),
        }
    }
}

/// Logging settings.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct LogSettings {
    /// Output format for the structured logger.
    #[serde(default = "default_log_format")]
    pub format: LogFormat,
}

impl Default for LogSettings {
    fn default() -> Self {
        Self {
            format: default_log_format(),
        }
    }
}

/// Output format for the structured logger.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LogFormat {
    /// Newline-delimited JSON (for log aggregators).
    Json,
    /// Human-readable pretty format (for local dev).
    Pretty,
}

fn default_addr() -> String {
    "0.0.0.0:3000".into()
}

fn default_workers() -> usize {
    4
}

fn default_log_format() -> LogFormat {
    LogFormat::Pretty
}

impl Settings {
    /// Load from environment variables. Fails fast on errors.
    pub fn load() -> Result<Self, ConfigError> {
        let settings: Settings = Figment::new()
            .merge(Env::prefixed("KOKKAK_").split("__"))
            .extract()?;
        settings.validate()?;
        Ok(settings)
    }

    /// Validate the loaded settings.
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.server.addr.trim().is_empty() {
            return Err(ConfigError::Invalid {
                key: "KOKKAK_SERVER__ADDR".into(),
                message: "must not be empty".into(),
            });
        }
        if self.server.workers == 0 {
            return Err(ConfigError::Invalid {
                key: "KOKKAK_SERVER__WORKERS".into(),
                message: "must be >= 1".into(),
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Tests that touch env vars must hold this lock to avoid races
    /// with other tests modifying the same variables in parallel.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn clear_kokkak_env() {
        for key in [
            "KOKKAK_SERVER__ADDR",
            "KOKKAK_SERVER__WORKERS",
            "KOKKAK_LOG__FORMAT",
        ] {
            std::env::remove_var(key);
        }
    }

    #[test]
    fn default_settings_validate() {
        let s = Settings::default();
        assert_eq!(s.server.addr, "0.0.0.0:3000");
        assert_eq!(s.server.workers, 4);
        assert_eq!(s.log.format, LogFormat::Pretty);
        assert!(s.validate().is_ok());
    }

    #[test]
    fn load_with_no_env_uses_defaults() {
        let _guard = ENV_LOCK.lock().expect("mutex poisoned");
        clear_kokkak_env();

        let s = Settings::load().expect("load should succeed with defaults");
        assert_eq!(s.server.addr, "0.0.0.0:3000");
        assert_eq!(s.server.workers, 4);
        assert_eq!(s.log.format, LogFormat::Pretty);
    }

    #[test]
    fn load_from_env_overrides() {
        let _guard = ENV_LOCK.lock().expect("mutex poisoned");
        clear_kokkak_env();

        std::env::set_var("KOKKAK_SERVER__ADDR", "127.0.0.1:9999");
        std::env::set_var("KOKKAK_SERVER__WORKERS", "8");
        std::env::set_var("KOKKAK_LOG__FORMAT", "json");

        let s = Settings::load().expect("load should succeed");
        assert_eq!(s.server.addr, "127.0.0.1:9999");
        assert_eq!(s.server.workers, 8);
        assert_eq!(s.log.format, LogFormat::Json);

        clear_kokkak_env();
    }

    #[test]
    fn invalid_log_format_fails() {
        let _guard = ENV_LOCK.lock().expect("mutex poisoned");
        clear_kokkak_env();
        std::env::set_var("KOKKAK_LOG__FORMAT", "xml");

        let result = Settings::load();
        assert!(result.is_err(), "invalid format should fail to parse");
    }

    #[test]
    fn empty_addr_fails_validation() {
        let s = Settings {
            server: ServerSettings {
                addr: "".into(),
                workers: 4,
            },
            log: LogSettings {
                format: LogFormat::Pretty,
            },
        };
        assert!(s.validate().is_err());
    }

    #[test]
    fn whitespace_only_addr_fails_validation() {
        let s = Settings {
            server: ServerSettings {
                addr: "   ".into(),
                workers: 4,
            },
            log: LogSettings {
                format: LogFormat::Pretty,
            },
        };
        assert!(s.validate().is_err());
    }

    #[test]
    fn zero_workers_fails_validation() {
        let s = Settings {
            server: ServerSettings {
                addr: "0.0.0.0:3000".into(),
                workers: 0,
            },
            log: LogSettings {
                format: LogFormat::Pretty,
            },
        };
        assert!(s.validate().is_err());
    }

    #[test]
    fn log_format_parses_both_variants() {
        let json: LogFormat = serde_json::from_str("\"json\"").unwrap();
        let pretty: LogFormat = serde_json::from_str("\"pretty\"").unwrap();
        assert_eq!(json, LogFormat::Json);
        assert_eq!(pretty, LogFormat::Pretty);
    }
}
