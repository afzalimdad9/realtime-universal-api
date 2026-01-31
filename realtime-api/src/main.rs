use anyhow::Result;
use tracing::{info, instrument};

mod alerting;
mod api;
mod auth;
mod billing;
mod config;
mod database;
mod event_service;
mod graphql;
mod models;
mod nats;
mod observability;
mod rbac;
mod routes;
mod schema_validator;
mod sse;
mod websocket;

use alerting::AlertingService;
use api::AppState;
use auth::AuthService;
use config::Config;
use database::Database;
use event_service::EventService;
use nats::NatsClient;
use observability::init_observability;
use routes::create_router;
use schema_validator::SchemaValidator;

#[tokio::main]
#[instrument]
async fn main() -> Result<()> {
    // Load configuration
    let config = Config::from_env()?;

    // Initialize comprehensive observability (tracing, metrics, alerting)
    info!("Initializing observability...");
    let metrics = init_observability(&config).await?;
    
    // Initialize alerting service
    let alerting = AlertingService::new(config.observability.clone());

    info!("Starting Realtime SaaS Platform API");
    info!("Configuration loaded successfully");

    // Initialize database connection
    info!("Connecting to database...");
    let database = Database::new(&config.database.url).await?;

    // Run database migrations
    database.migrate().await?;
    info!("Database connection established and migrations completed");

    // Initialize NATS connection
    info!("Connecting to NATS...");
    let nats_client = NatsClient::new(&config.nats.url, config.nats.stream_name.clone()).await?;
    info!("NATS connection established");

    // Initialize schema validator
    let schema_validator = SchemaValidator::new();

    // Initialize event service
    let event_service = EventService::new(database.clone(), nats_client, schema_validator);

    // Initialize auth service
    let auth_service = AuthService::new(database.clone(), config.jwt_secret.clone());

    // Create application state
    let app_state = AppState {
        database,
        event_service,
        auth_service,
        metrics,
        alerting,
    };

    // Create the router
    let app = create_router(app_state);

    // Start HTTP server
    let listener =
        tokio::net::TcpListener::bind(&format!("{}:{}", config.server.host, config.server.port))
            .await?;
    info!(
        "Server listening on {}:{}",
        config.server.host, config.server.port
    );

    info!("Realtime API server started successfully");

    // Start the server
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    info!("Server shut down gracefully");
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    info!("Shutdown signal received");
}
