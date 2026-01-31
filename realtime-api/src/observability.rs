use anyhow::Result;
use axum_prometheus::PrometheusMetricLayer;
use opentelemetry::global;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::{runtime, Resource};
use prometheus::{Counter, Histogram, Registry, Gauge};
use std::sync::Arc;
use tracing::{info, warn, Span};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};
use uuid::Uuid;

use crate::config::Config;

/// Metrics collector for the realtime platform
#[derive(Clone)]
pub struct Metrics {
    pub registry: Arc<Registry>,
    pub events_published_total: Counter,
    pub events_delivered_total: Counter,
    pub websocket_connections_active: Gauge,
    pub sse_connections_active: Gauge,
    pub api_requests_total: Counter,
    pub api_request_duration: Histogram,
    pub billing_operations_total: Counter,
    pub auth_operations_total: Counter,
    pub errors_total: Counter,
}

impl Metrics {
    pub fn new() -> Result<Self> {
        let registry = Arc::new(Registry::new());
        
        let events_published_total = Counter::new(
            "realtime_events_published_total",
            "Total number of events published"
        )?;
        
        let events_delivered_total = Counter::new(
            "realtime_events_delivered_total", 
            "Total number of events delivered to clients"
        )?;
        
        let websocket_connections_active = Gauge::new(
            "realtime_websocket_connections_active",
            "Number of active WebSocket connections"
        )?;
        
        let sse_connections_active = Gauge::new(
            "realtime_sse_connections_active",
            "Number of active SSE connections"
        )?;
        
        let api_requests_total = Counter::new(
            "realtime_api_requests_total",
            "Total number of API requests"
        )?;
        
        let api_request_duration = Histogram::with_opts(
            prometheus::HistogramOpts::new(
                "realtime_api_request_duration_seconds",
                "API request duration in seconds"
            )
        )?;
        
        let billing_operations_total = Counter::new(
            "realtime_billing_operations_total",
            "Total number of billing operations"
        )?;
        
        let auth_operations_total = Counter::new(
            "realtime_auth_operations_total",
            "Total number of authentication operations"
        )?;
        
        let errors_total = Counter::new(
            "realtime_errors_total",
            "Total number of errors by type"
        )?;

        // Register all metrics
        registry.register(Box::new(events_published_total.clone()))?;
        registry.register(Box::new(events_delivered_total.clone()))?;
        registry.register(Box::new(websocket_connections_active.clone()))?;
        registry.register(Box::new(sse_connections_active.clone()))?;
        registry.register(Box::new(api_requests_total.clone()))?;
        registry.register(Box::new(api_request_duration.clone()))?;
        registry.register(Box::new(billing_operations_total.clone()))?;
        registry.register(Box::new(auth_operations_total.clone()))?;
        registry.register(Box::new(errors_total.clone()))?;

        Ok(Self {
            registry,
            events_published_total,
            events_delivered_total,
            websocket_connections_active,
            sse_connections_active,
            api_requests_total,
            api_request_duration,
            billing_operations_total,
            auth_operations_total,
            errors_total,
        })
    }
    
    /// Get the Prometheus metrics layer for Axum
    pub fn prometheus_layer(&self) -> PrometheusMetricLayer<'static> {
        PrometheusMetricLayer::new()
    }
    
    /// Record an event publication
    pub fn record_event_published(&self, tenant_id: &str, topic: &str) {
        self.events_published_total.inc();
        tracing::info!(
            tenant_id = tenant_id,
            topic = topic,
            "Event published"
        );
    }
    
    /// Record an event delivery
    pub fn record_event_delivered(&self, tenant_id: &str, connection_type: &str) {
        self.events_delivered_total.inc();
        tracing::debug!(
            tenant_id = tenant_id,
            connection_type = connection_type,
            "Event delivered"
        );
    }
    
    /// Record WebSocket connection change
    pub fn record_websocket_connection_change(&self, delta: i64) {
        if delta > 0 {
            self.websocket_connections_active.add(delta as f64);
        } else {
            self.websocket_connections_active.sub((-delta) as f64);
        }
    }
    
    /// Record SSE connection change
    pub fn record_sse_connection_change(&self, delta: i64) {
        if delta > 0 {
            self.sse_connections_active.add(delta as f64);
        } else {
            self.sse_connections_active.sub((-delta) as f64);
        }
    }
    
    /// Record API request
    pub fn record_api_request(&self, method: &str, path: &str, duration_seconds: f64) {
        self.api_requests_total.inc();
        self.api_request_duration.observe(duration_seconds);
        tracing::debug!(
            method = method,
            path = path,
            duration_seconds = duration_seconds,
            "API request completed"
        );
    }
    
    /// Record billing operation
    pub fn record_billing_operation(&self, operation: &str, tenant_id: &str) {
        self.billing_operations_total.inc();
        tracing::info!(
            operation = operation,
            tenant_id = tenant_id,
            "Billing operation completed"
        );
    }
    
    /// Record authentication operation
    pub fn record_auth_operation(&self, operation: &str, success: bool) {
        self.auth_operations_total.inc();
        tracing::info!(
            operation = operation,
            success = success,
            "Authentication operation completed"
        );
    }
    
    /// Record error
    pub fn record_error(&self, error_type: &str, context: &str) {
        self.errors_total.inc();
        tracing::error!(
            error_type = error_type,
            context = context,
            "Error occurred"
        );
    }
}

/// Add correlation ID to the current span
pub fn add_correlation_id() -> String {
    let correlation_id = Uuid::new_v4().to_string();
    Span::current().record("correlation_id", &correlation_id);
    correlation_id
}

/// Initialize comprehensive observability including tracing, metrics, and structured logging
pub async fn init_observability(config: &Config) -> Result<Metrics> {
    // Initialize metrics first
    let metrics = Metrics::new()?;
    
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
            .json()
            .with_current_span(true)
            .with_span_list(true),
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

    info!("Comprehensive observability initialized successfully");
    Ok(metrics)
}

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
