use anyhow::Result;
use opentelemetry::global;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::{runtime, Resource};
use tracing::{info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use crate::config::Config;

pub async fn init_tracing(config: &Config) -> Result<()> {
    // Create a resource that identifies this service
    let resource = Resource::new(vec![
        opentelemetry::KeyValue::new("service.name", config.observability.service_name.clone()),
        opentelemetry::KeyValue::new("service.version", env!("CARGO_PKG_VERSION")),
    ]);

    // Set up the tracing subscriber with multiple layers
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(&config.observability.log_level));

    let subscriber = tracing_subscriber::registry().with(env_filter).with(
        tracing_subscriber::fmt::layer()
            .with_target(false)
            .with_thread_ids(true)
            .with_file(true)
            .with_line_number(true)
            .json(),
    );

    // Add OpenTelemetry tracing if endpoint is configured
    if let Some(endpoint) = &config.observability.tracing_endpoint {
        info!(
            "Initializing OpenTelemetry tracing with endpoint: {}",
            endpoint
        );

        let tracer = opentelemetry_otlp::new_pipeline()
            .tracing()
            .with_exporter(
                opentelemetry_otlp::new_exporter()
                    .tonic()
                    .with_endpoint(endpoint),
            )
            .with_trace_config(opentelemetry_sdk::trace::config().with_resource(resource))
            .install_batch(runtime::Tokio)?;

        let telemetry_layer = tracing_opentelemetry::layer().with_tracer(tracer);

        subscriber.with(telemetry_layer).try_init()?;
    } else {
        warn!("OpenTelemetry endpoint not configured, using local logging only");
        subscriber.try_init()?;
    }

    info!("Tracing initialized successfully");
    Ok(())
}

pub fn shutdown_tracing() {
    global::shutdown_tracer_provider();
}
