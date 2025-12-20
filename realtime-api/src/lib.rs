// Library module for shared functionality and testing
pub mod api;
pub mod auth;
pub mod config;
pub mod database;
pub mod event_service;
pub mod graphql;
pub mod models;
pub mod nats;
pub mod observability;
pub mod routes;
pub mod schema_validator;
pub mod websocket;

pub use api::{AppState, PublishEventRequest, PublishEventResponse, ErrorResponse};
pub use auth::*;
pub use config::Config;
pub use database::Database;
pub use event_service::{EventService, PublishResult, EventSubscription};
pub use graphql::{create_schema, graphql_handler, graphql_playground, graphql_subscription_handler, ApiSchema};
pub use models::*;
pub use nats::{NatsClient, EventCursor, ReplayRequest, SubscriptionConfig};
pub use observability::{init_tracing, shutdown_tracing};
pub use routes::create_router;
pub use schema_validator::{SchemaValidator, validate_tenant_isolation, validate_api_key_security, validate_event_structure};
pub use websocket::{WebSocketConnectionParams, WebSocketMessage, broadcast_event_to_websockets, terminate_tenant_websocket_connections, get_websocket_stats};