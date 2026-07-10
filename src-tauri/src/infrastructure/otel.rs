use opentelemetry::trace::TracerProvider as _;
use opentelemetry::KeyValue;
use opentelemetry_otlp::{Protocol, WithExportConfig};
use opentelemetry_sdk::trace::SdkTracerProvider;
use opentelemetry_sdk::Resource;
use std::env;
use std::time::Duration;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

const DEFAULT_SERVICE_NAME: &str = "omiga";
const OTLP_TRACES_PATH: &str = "/v1/traces";
const OTEL_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(3);
const OTEL_EXPORT_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OtelExportConfig {
    pub endpoint: String,
    pub service_name: String,
}

impl OtelExportConfig {
    fn traces_endpoint(&self) -> String {
        let base = self.endpoint.trim_end_matches('/');
        // Accept both the OTLP base endpoint and a full traces URL: never
        // append the signal path twice.
        if base.ends_with(OTLP_TRACES_PATH) {
            return base.to_string();
        }
        format!("{}/{}", base, OTLP_TRACES_PATH.trim_start_matches('/'))
    }
}

pub struct OtelGuard {
    tracer_provider: SdkTracerProvider,
}

impl OtelGuard {
    fn new(tracer_provider: SdkTracerProvider) -> Self {
        Self { tracer_provider }
    }
}

impl Drop for OtelGuard {
    fn drop(&mut self) {
        if let Err(err) = self
            .tracer_provider
            .shutdown_with_timeout(OTEL_SHUTDOWN_TIMEOUT)
        {
            // Best-effort: the subscriber may already be torn down at exit,
            // so also print to stderr for visibility.
            tracing::warn!(target: "omiga::otel", "OTLP tracer shutdown failed: {err}");
            eprintln!("omiga: OTLP tracer shutdown failed: {err}");
        }
    }
}

pub fn init_tracing() -> Option<OtelGuard> {
    let Some(config) = otel_export_config_from_env() else {
        init_fmt_only();
        return None;
    };

    let exporter = match opentelemetry_otlp::SpanExporter::builder()
        .with_http()
        .with_endpoint(config.traces_endpoint())
        .with_protocol(Protocol::HttpBinary)
        // Keep each export attempt shorter than OTEL_SHUTDOWN_TIMEOUT so the
        // final flush on exit can actually complete instead of being cut off.
        .with_timeout(OTEL_EXPORT_TIMEOUT)
        .build()
    {
        Ok(exporter) => exporter,
        Err(err) => {
            init_fmt_only();
            tracing::warn!(
                target: "omiga::otel",
                "Failed to initialize OTLP trace exporter: {}",
                err
            );
            return None;
        }
    };

    let resource = Resource::builder_empty()
        .with_service_name(config.service_name)
        .with_attribute(KeyValue::new("service.version", env!("CARGO_PKG_VERSION")))
        .build();
    let tracer_provider = SdkTracerProvider::builder()
        .with_resource(resource)
        .with_batch_exporter(exporter)
        .build();
    let tracer = tracer_provider.tracer("omiga");
    let otel_layer = tracing_opentelemetry::layer().with_tracer(tracer);

    tracing_subscriber::registry()
        .with(EnvFilter::from_default_env())
        .with(tracing_subscriber::fmt::layer())
        .with(otel_layer)
        .init();

    Some(OtelGuard::new(tracer_provider))
}

pub fn otel_export_config_from_env_values(
    endpoint: Option<&str>,
    service: Option<&str>,
) -> Option<OtelExportConfig> {
    let endpoint = endpoint?.trim();
    if endpoint.is_empty() {
        return None;
    }

    let service_name = service
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(DEFAULT_SERVICE_NAME);

    Some(OtelExportConfig {
        endpoint: endpoint.to_string(),
        service_name: service_name.to_string(),
    })
}

fn otel_export_config_from_env() -> Option<OtelExportConfig> {
    let endpoint = env::var("OTEL_EXPORTER_OTLP_ENDPOINT").ok();
    let service = env::var("OTEL_SERVICE_NAME").ok();
    otel_export_config_from_env_values(endpoint.as_deref(), service.as_deref())
}

fn init_fmt_only() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn otel_export_config_is_none_without_endpoint() {
        assert_eq!(otel_export_config_from_env_values(None, None), None);
    }

    #[test]
    fn otel_export_config_is_none_for_blank_endpoint() {
        assert_eq!(
            otel_export_config_from_env_values(Some(" \t\n "), Some("ignored")),
            None
        );
    }

    #[test]
    fn otel_export_config_uses_default_service_for_blank_service() {
        assert_eq!(
            otel_export_config_from_env_values(Some(" http://localhost:4318 "), Some("  ")),
            Some(OtelExportConfig {
                endpoint: "http://localhost:4318".to_string(),
                service_name: DEFAULT_SERVICE_NAME.to_string(),
            })
        );
    }

    #[test]
    fn traces_endpoint_appends_signal_path_to_base_endpoint() {
        let config = OtelExportConfig {
            endpoint: "http://localhost:4318/".to_string(),
            service_name: DEFAULT_SERVICE_NAME.to_string(),
        };
        assert_eq!(config.traces_endpoint(), "http://localhost:4318/v1/traces");
    }

    #[test]
    fn traces_endpoint_does_not_double_signal_path() {
        let config = OtelExportConfig {
            endpoint: "http://localhost:4318/v1/traces".to_string(),
            service_name: DEFAULT_SERVICE_NAME.to_string(),
        };
        assert_eq!(config.traces_endpoint(), "http://localhost:4318/v1/traces");
    }

    #[test]
    fn otel_export_config_uses_provided_service() {
        assert_eq!(
            otel_export_config_from_env_values(Some("http://collector:4318"), Some(" omiga-dev ")),
            Some(OtelExportConfig {
                endpoint: "http://collector:4318".to_string(),
                service_name: "omiga-dev".to_string(),
            })
        );
    }
}
