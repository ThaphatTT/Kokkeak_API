//! Telemetry: tracing (structured log) + Prometheus metrics, with
//! optional OpenTelemetry export (T-24).
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
//! // T-24: opt-in OTLP bridge (requires `--features kokkak-common/otel`)
//! telemetry::init_otel("kokkak-api", None);
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

// ---------------------------------------------------------------------------
// T-24: OpenTelemetry bridge.
//
// Opt-in via `--features kokkak-common/otel`. When enabled, this
// function adds an OTLP/gRPC exporter for traces + metrics and wires
// it into the existing tracing subscriber.
//
// Env vars (standard OpenTelemetry SDK names):
//   OTEL_EXPORTER_OTLP_ENDPOINT   e.g. http://otel-collector:4317
//   OTEL_SERVICE_NAME             e.g. kokkak-api   (we also pass this in code)
//   OTEL_RESOURCE_ATTRIBUTES      key=value,key=value
//
// Without the `otel` feature, the function is a no-op so callers
// in `crates/api/src/main.rs` and `crates/worker/src/main.rs` can
// invoke it unconditionally.
// ---------------------------------------------------------------------------

#[cfg(feature = "otel")]
mod otel_impl {
    //! Thin OTLP exporter setup. Kept behind the `otel` feature so
    //! the default build doesn't pull in `tonic` + the OTLP stack.
    //!
    //! ponytail: the implementation is the smallest one that
    //! actually exports — no custom resource attributes beyond
    //! `service.name`, no custom sampling policy (parent_based
    //! is the default and matches upstream expectations).

    use std::sync::OnceLock;

    use opentelemetry::{global, trace::TracerProvider as _, KeyValue};
    use opentelemetry_otlp::WithExportConfig;
    use opentelemetry_sdk::{metrics::SdkMeterProvider, trace::TracerProvider, Resource};
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;
    use tracing_subscriber::EnvFilter;

    static METER_PROVIDER: OnceLock<SdkMeterProvider> = OnceLock::new();

    /// Initialise the OTLP trace + metrics exporters.
    ///
    /// Returns `true` when a real exporter was wired in, `false`
    /// when no endpoint was configured (in which case the call is
    /// a no-op so the rest of the service still boots).
    pub fn init(service_name: &'static str, otlp_endpoint: Option<&str>) -> bool {
        let endpoint = otlp_endpoint
            .map(|s| s.to_string())
            .or_else(|| std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT").ok());

        let Some(endpoint) = endpoint else {
            eprintln!(
                "[kokkak-common] OTEL_EXPORTER_OTLP_ENDPOINT not set; \
                 OTel bridge disabled. To enable, set the env var or \
                 pass an endpoint to init_otel()."
            );
            return false;
        };

        let resource = Resource::new(vec![
            KeyValue::new("service.name", service_name),
            KeyValue::new("service.version", env!("CARGO_PKG_VERSION")),
        ]);

        // ---- Traces ----
        let span_exporter = opentelemetry_otlp::SpanExporter::builder()
            .with_tonic()
            .with_endpoint(endpoint.clone())
            .build()
            .expect("build OTLP span exporter");

        let tracer_provider = TracerProvider::builder()
            .with_batch_exporter(span_exporter, opentelemetry_sdk::runtime::Tokio)
            .with_resource(resource.clone())
            .build();

        let tracer = tracer_provider.tracer(service_name);
        global::set_tracer_provider(tracer_provider);

        // Bridge the OTel tracer into tracing — every
        // `tracing::info_span!` becomes an OTel span automatically.
        let otel_layer = tracing_opentelemetry::layer().with_tracer(tracer);

        // Re-install the subscriber with the OTel layer attached.
        // Callers should invoke `init_tracing()` BEFORE `init_otel()`.
        let env_filter = EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| EnvFilter::new("info,kokkak_api=debug"));

        let _ = tracing_subscriber::registry()
            .with(env_filter)
            .with(otel_layer)
            .try_init();

        // ---- Metrics ----
        let metric_exporter = opentelemetry_otlp::MetricExporter::builder()
            .with_tonic()
            .with_endpoint(endpoint)
            .build()
            .expect("build OTLP metric exporter");

        let reader = opentelemetry_sdk::metrics::PeriodicReader::builder(
            metric_exporter,
            opentelemetry_sdk::runtime::Tokio,
        )
        .build();

        let meter_provider = SdkMeterProvider::builder()
            .with_reader(reader)
            .with_resource(resource)
            .build();

        let _ = METER_PROVIDER.set(meter_provider);
        global::set_meter_provider(METER_PROVIDER.get().expect("just installed").clone());

        true
    }
}

/// Initialise the OpenTelemetry OTLP bridge (T-24).
///
/// Re-export of the feature-gated implementation so callers in
/// `main.rs` and `worker/src/main.rs` can call it unconditionally
/// without sprinkling `#[cfg(feature = "otel")]` everywhere.
#[cfg(feature = "otel")]
pub fn init_otel(service_name: &'static str, otlp_endpoint: Option<&str>) -> bool {
    otel_impl::init(service_name, otlp_endpoint)
}

/// Stub when the `otel` feature is disabled — never panics, always
/// returns false so callers can ignore the result.
#[cfg(not(feature = "otel"))]
pub fn init_otel(_service_name: &'static str, _otlp_endpoint: Option<&str>) -> bool {
    false
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

    #[test]
    fn init_otel_without_endpoint_returns_false() {
        // Without OTEL_EXPORTER_OTLP_ENDPOINT set, init_otel must
        // return false regardless of feature state. With the
        // feature disabled (default), it's a stub. With the
        // feature enabled, it checks env and bails. Both paths
        // must agree.
        std::env::remove_var("OTEL_EXPORTER_OTLP_ENDPOINT");
        assert!(!init_otel("kokkak-test", None));
    }
}
