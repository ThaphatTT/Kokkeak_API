//! Telemetry: tracing (structured log) + Prometheus metrics.
//!
//! Initialise once at startup (idempotent):
//!
//! ```no_run
//! use kokkak_common::config::Settings;
//! use kokkak_common::telemetry;
//!
//! let settings = Settings::load().expect("invalid config");
//! telemetry::init_tracing(settings.log.format);
//! let _handle = telemetry::init_metrics();
//! ```
//!
//! Then expose `/metrics` with [`render_metrics`].

use std::sync::OnceLock;

use metrics_exporter_prometheus::PrometheusHandle;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use crate::config::LogFormat;

/// Lazily-initialised handle to the Prometheus recorder.
///
/// Calling [`init_metrics`] more than once returns the same handle; no
/// second recorder is installed (the global recorder is a singleton).
static METRICS_HANDLE: OnceLock<PrometheusHandle> = OnceLock::new();

/// Initialise the global tracing subscriber.
///
/// Honours `RUST_LOG` for the filter (default: `info,kokkak_api=debug`).
/// Format is chosen by `format` (JSON for prod, pretty for dev).
///
/// Idempotent: if a subscriber is already installed, this is a no-op
/// (prints a notice to stderr).
pub fn init_tracing(format: LogFormat) {
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,kokkak_api=debug"));

    let result = match format {
        LogFormat::Json => tracing_subscriber::registry()
            .with(env_filter)
            .with(fmt::layer().json().flatten_event(true))
            .try_init(),
        LogFormat::Pretty => tracing_subscriber::registry()
            .with(env_filter)
            .with(fmt::layer().pretty().with_target(true))
            .try_init(),
    };

    if result.is_err() {
        eprintln!(
            "[kokkak-common] tracing subscriber already initialised (this is OK if called twice)"
        );
    }
}

/// Initialise the Prometheus metrics recorder and return its handle.
///
/// Idempotent: subsequent calls return the same handle without
/// trying to install a second recorder.
pub fn init_metrics() -> &'static PrometheusHandle {
    METRICS_HANDLE.get_or_init(|| {
        metrics_exporter_prometheus::PrometheusBuilder::new()
            .set_buckets_for_metric(
                metrics_exporter_prometheus::Matcher::Full(String::new()),
                &[
                    0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
                ],
            )
            .expect("valid histogram buckets")
            .install_recorder()
            .expect("install Prometheus recorder (is another recorder already installed?)")
    })
}

/// Render Prometheus text-format metrics (for `GET /metrics`).
pub fn render_metrics() -> String {
    init_metrics().render()
}

#[cfg(test)]
mod tests {
    use super::*;
    use metrics::counter;

    #[test]
    fn init_metrics_returns_static_handle() {
        let h1 = init_metrics();
        let h2 = init_metrics();
        assert!(
            std::ptr::eq(h1, h2),
            "init_metrics must return the same static handle on every call"
        );
    }

    #[test]
    fn render_metrics_succeeds_after_counter() {
        // Smoke test: recording a metric then rendering must not panic,
        // and the handle must work. The actual text format is verified
        // by the integration smoke test of GET /metrics on a running server.
        counter!("kokkak_test_setup_total").increment(1);
        let _ = init_metrics().render();
    }
}
