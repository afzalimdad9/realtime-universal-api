use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::env;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub server: ServerConfig,
    pub database: DatabaseConfig,
    pub nats: NatsConfig,
    pub observability: ObservabilityConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseConfig {
    pub url: String,
    pub max_connections: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NatsConfig {
    pub url: String,
    pub stream_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObservabilityConfig {
    pub tracing_endpoint: Option<String>,
    pub service_name: String,
    pub log_level: String,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        dotenvy::dotenv().ok(); // Load .env file if it exists
        
        let config = Config {
            server: ServerConfig {
                host: env::var("SERVER_HOST").unwrap_or_else(|_| "0.0.0.0".to_string()),
                port: env::var("SERVER_PORT")
                    .unwrap_or_else(|_| "3000".to_string())
                    .parse()?,
            },
            database: DatabaseConfig {
                url: env::var("DATABASE_URL")
                    .unwrap_or_else(|_| "postgresql://postgres:password@localhost:5432/realtime_platform".to_string()),
                max_connections: env::var("DATABASE_MAX_CONNECTIONS")
                    .unwrap_or_else(|_| "10".to_string())
                    .parse()?,
            },
            nats: NatsConfig {
                url: env::var("NATS_URL")
                    .unwrap_or_else(|_| "nats://localhost:4222".to_string()),
                stream_name: env::var("NATS_STREAM_NAME")
                    .unwrap_or_else(|_| "EVENTS".to_string()),
            },
            observability: ObservabilityConfig {
                tracing_endpoint: env::var("OTEL_EXPORTER_OTLP_ENDPOINT").ok(),
                service_name: env::var("OTEL_SERVICE_NAME")
                    .unwrap_or_else(|_| "realtime-api".to_string()),
                log_level: env::var("RUST_LOG")
                    .unwrap_or_else(|_| "info".to_string()),
            },
        };
        
        Ok(config)
    }
}