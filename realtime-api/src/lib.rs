// Library module for shared functionality and testing
pub mod auth;
pub mod config;
pub mod database;
pub mod event_service;
pub mod models;
pub mod nats;
pub mod observability;
pub mod schema_validator;

pub use auth::*;
pub use config::Config;
pub use database::Database;
pub use event_service::{EventService, PublishResult, EventSubscription};
pub use models::*;
pub use nats::{NatsClient, EventCursor, ReplayRequest, SubscriptionConfig};
pub use observability::{init_tracing, shutdown_tracing};
pub use schema_validator::{SchemaValidator, validate_tenant_isolation, validate_api_key_security, validate_event_structure};