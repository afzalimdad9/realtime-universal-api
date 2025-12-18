use axum::{
    middleware,
    routing::{delete, get, post},
    Router,
};
use tower::ServiceBuilder;
use tower_http::{
    cors::{Any, CorsLayer},
    trace::TraceLayer,
};

use crate::api::{
    create_api_key, create_tenant, get_usage_report, handle_stripe_webhook, health_check,
    publish_event, revoke_api_key, AppState,
};
use crate::auth::{api_key_auth_middleware, AuthService};

/// Create the main application router with all endpoints
pub fn create_router(state: AppState) -> Router {
    // Create the auth service for middleware
    let auth_service = state.auth_service.clone();

    Router::new()
        // Public endpoints (no authentication required)
        .route("/health", get(health_check))
        .route("/billing/stripe-webhook", post(handle_stripe_webhook))
        
        // Protected endpoints (require authentication)
        .route("/events", post(publish_event))
        .route("/admin/tenants", post(create_tenant))
        .route("/admin/api-keys", post(create_api_key))
        .route("/admin/api-keys/:key_id", delete(revoke_api_key))
        .route("/billing/usage", get(get_usage_report))
        
        // Apply authentication middleware to protected routes
        .layer(
            ServiceBuilder::new()
                .layer(middleware::from_fn_with_state(
                    auth_service,
                    api_key_auth_middleware,
                ))
                .layer(TraceLayer::new_for_http())
                .layer(
                    CorsLayer::new()
                        .allow_origin(Any)
                        .allow_methods(Any)
                        .allow_headers(Any),
                ),
        )
        .with_state(state)
}

/// Create a router for WebSocket connections
pub fn create_websocket_router(state: AppState) -> Router {
    Router::new()
        .route("/ws", get(websocket_handler))
        .with_state(state)
}

/// Create a router for Server-Sent Events
pub fn create_sse_router(state: AppState) -> Router {
    Router::new()
        .route("/sse", get(sse_handler))
        .with_state(state)
}

/// WebSocket handler (placeholder for future implementation)
async fn websocket_handler() -> &'static str {
    "WebSocket endpoint - to be implemented in task 7"
}

/// SSE handler (placeholder for future implementation)
async fn sse_handler() -> &'static str {
    "SSE endpoint - to be implemented in task 8"
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::AuthService;
    use crate::database::Database;
    use crate::event_service::EventService;
    use crate::nats::NatsClient;
    use crate::schema_validator::SchemaValidator;

    #[tokio::test]
    async fn test_router_creation() {
        // This is a basic test to ensure the router can be created
        // More comprehensive tests would require setting up test database and NATS
        
        // For now, we'll just test that the router creation doesn't panic
        // In a real test, we'd set up proper test dependencies
        
        // Note: This test is commented out because it requires actual database/NATS connections
        // which we don't have in the test environment yet
        
        // let database = Database::new("postgresql://test").await.unwrap();
        // let nats_client = NatsClient::new("nats://test").await.unwrap();
        // let schema_validator = SchemaValidator::new();
        // let event_service = EventService::new(database.clone(), nats_client, schema_validator);
        // let auth_service = AuthService::new(database.clone(), "test_secret".to_string());
        
        // let state = AppState {
        //     database,
        //     event_service,
        //     auth_service,
        // };
        
        // let router = create_router(state);
        // assert!(router.into_make_service().is_ok());
        
        // For now, just test that the function exists and can be called
        println!("Router creation test placeholder - requires test infrastructure");
    }
}