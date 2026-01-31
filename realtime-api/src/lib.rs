// Library module for shared functionality and testing
pub mod alerting;
pub mod api;
pub mod auth;
pub mod billing;
pub mod config;
pub mod database;
pub mod event_service;
pub mod graphql;
pub mod models;
pub mod nats;
pub mod observability;
pub mod rbac;
pub mod routes;
pub mod schema_validator;
pub mod sse;
pub mod websocket;

pub use alerting::{Alert, AlertSeverity, AlertingService};
pub use billing::BillingService;

pub use api::{AppState, ErrorResponse, PublishEventRequest, PublishEventResponse};
pub use auth::*;
pub use config::Config;
pub use database::Database;
pub use event_service::{EventService, EventSubscription, PublishResult};
pub use graphql::{
    create_schema, graphql_handler, graphql_playground, graphql_subscription_handler, ApiSchema,
};
pub use models::*;
pub use nats::{EventCursor, NatsClient, ReplayRequest, SubscriptionConfig};
pub use observability::{init_observability, init_tracing, shutdown_tracing, Metrics, add_correlation_id};
pub use routes::create_router;
pub use schema_validator::{
    validate_api_key_security, validate_event_structure, validate_tenant_isolation, SchemaValidator,
};
pub use sse::{
    broadcast_event_to_sse, get_sse_stats, sse_handler, terminate_tenant_sse_connections,
    SSEConnectionParams, SSEMessage,
};
pub use websocket::{
    broadcast_event_to_websockets, get_websocket_stats, terminate_tenant_websocket_connections,
    WebSocketConnectionParams, WebSocketMessage,
};
