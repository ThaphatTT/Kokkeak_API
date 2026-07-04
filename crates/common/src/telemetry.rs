

use std::sync::OnceLock;

use metrics_exporter_prometheus::PrometheusHandle;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use crate::config::LogFormat;

static METRICS_HANDLE: OnceLock<PrometheusHandle> = OnceLock::new();

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

pub fn render_metrics() -> String {
    init_metrics().render()
}

#[cfg(feature = "otel")]
mod otel_impl {

    use std::sync::OnceLock;

    use opentelemetry::{global, trace::TracerProvider as _, KeyValue};
    use opentelemetry_otlp::WithExportConfig;
    use opentelemetry_sdk::{metrics::SdkMeterProvider, trace::TracerProvider, Resource};
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;
    use tracing_subscriber::EnvFilter;

    static METER_PROVIDER: OnceLock<SdkMeterProvider> = OnceLock::new();

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

        let otel_layer = tracing_opentelemetry::layer().with_tracer(tracer);

        let env_filter = EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| EnvFilter::new("info,kokkak_api=debug"));

        let _ = tracing_subscriber::registry()
            .with(env_filter)
            .with(otel_layer)
            .try_init();

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

#[cfg(feature = "otel")]
pub fn init_otel(service_name: &'static str, otlp_endpoint: Option<&str>) -> bool {
    otel_impl::init(service_name, otlp_endpoint)
}

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

        counter!("kokkak_test_setup_total").increment(1);
        let _ = init_metrics().render();
    }

    #[test]
    fn init_otel_without_endpoint_returns_false() {

        std::env::remove_var("OTEL_EXPORTER_OTLP_ENDPOINT");
        assert!(!init_otel("kokkak-test", None));
    }
}
