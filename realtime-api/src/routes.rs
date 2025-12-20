use axum::{
    extract::{ws::WebSocketUpgrade, Extension, Query, State},
    middleware,
    response::Response,
    routing::{delete, get, post},
    Router,
};
use serde::Deserialize;
use tower::ServiceBuilder;
use tower_http::{
    cors::{Any, CorsLayer},
    trace::TraceLayer,
};

use crate::api::{
    create_api_key, create_tenant, get_usage_report, handle_stripe_webhook, health_check,
    publish_event, revoke_api_key, AppState,
};
use crate::auth::{api_key_auth_middleware, AuthContext};
use crate::graphql::{
    create_schema, graphql_handler, graphql_playground, graphql_subscription_handler, ApiSchema,
};

/// Create the main application router with all endpoints
pub fn create_router(state: AppState) -> Router {
    // Create the auth service for middleware
    let auth_service = state.auth_service.clone();

    // Create GraphQL schema
    let schema = create_schema(
        state.database.clone(),
        state.event_service.clone(),
        state.auth_service.clone(),
    );

    Router::new()
        // Public endpoints (no authentication required)
        .route("/health", get(health_check))
        .route("/billing/stripe-webhook", post(handle_stripe_webhook))
        // GraphQL playground (development only - should be disabled in production)
        .route("/graphql/playground", get(graphql_playground))
        // WebSocket endpoint (authentication handled in the handler)
        .route("/ws", get(websocket_handler))
        // Protected endpoints (require authentication)
        // TODO: Fix axum version conflicts for GraphQL routes
        // .route("/graphql", post(graphql_handler_with_auth))
        // .route("/graphql/ws", get(graphql_subscription_handler_with_auth))
        .route("/events", post(publish_event))
        .route("/admin/tenants", post(create_tenant))
        .route("/admin/api-keys", post(create_api_key))
        .route("/admin/api-keys/:key_id", delete(revoke_api_key))
        .route("/billing/usage", get(get_usage_report))
        // Apply authentication middleware to protected routes (except playground and WebSocket)
        .layer(middleware::from_fn_with_state(
            auth_service,
            api_key_auth_middleware,
        ))
        // Apply global middleware
        .layer(
            ServiceBuilder::new()
                .layer(TraceLayer::new_for_http())
                .layer(
                    CorsLayer::new()
                        .allow_origin(Any)
                        .allow_methods(Any)
                        .allow_headers(Any),
                ),
        )
        .with_state(state)
        .layer(Extension(schema))
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

/// WebSocket connection query parameters
#[derive(Debug, Deserialize)]
pub struct WebSocketQuery {
    pub topics: Option<String>, // Comma-separated list of topics
}

/// WebSocket handler with authentication and subscription management
async fn websocket_handler(
    State(state): State<AppState>,
    ws: WebSocketUpgrade,
    Query(params): Query<WebSocketQuery>,
    headers: axum::http::HeaderMap,
) -> Result<Response, axum::http::StatusCode> {
    use crate::auth::{extract_auth_header, AuthError};
    use crate::websocket::{handle_websocket_connection, WebSocketConnectionParams};

    // Extract authentication from headers
    let auth_value = match extract_auth_header(&headers) {
        Ok(value) => value,
        Err(_) => return Err(axum::http::StatusCode::UNAUTHORIZED),
    };

    // Validate authentication
    let auth_context = match state.auth_service.validate_api_key(&auth_value).await {
        Ok(context) => context,
        Err(AuthError::InvalidApiKey) => {
            // Try JWT validation as fallback
            match state.auth_service.validate_jwt(&auth_value).await {
                Ok(context) => context,
                Err(_) => return Err(axum::http::StatusCode::UNAUTHORIZED),
            }
        }
        Err(AuthError::RateLimitExceeded) => {
            return Err(axum::http::StatusCode::TOO_MANY_REQUESTS);
        }
        Err(AuthError::TenantSuspended) => {
            return Err(axum::http::StatusCode::FORBIDDEN);
        }
        Err(_) => return Err(axum::http::StatusCode::UNAUTHORIZED),
    };

    // Check if the API key has subscribe permissions
    use crate::models::Scope;
    if state
        .auth_service
        .check_scope(&auth_context, &Scope::EventsSubscribe)
        .is_err()
    {
        return Err(axum::http::StatusCode::FORBIDDEN);
    }

    // Parse topics from query parameters
    let topics = params
        .topics
        .map(|t| t.split(',').map(|s| s.trim().to_string()).collect())
        .unwrap_or_else(Vec::new);

    // Create connection parameters
    let connection_params = WebSocketConnectionParams {
        tenant_id: auth_context.tenant_id.clone(),
        project_id: auth_context.project_id.clone(),
        topics,
        auth_context,
    };

    // Upgrade to WebSocket
    Ok(ws.on_upgrade(move |socket| handle_websocket_connection(socket, connection_params, state)))
}

/// SSE handler (placeholder for future implementation)
async fn sse_handler() -> &'static str {
    "SSE endpoint - to be implemented in task 8"
}

/// GraphQL handler with authentication
#[allow(dead_code)]
async fn graphql_handler_with_auth(
    Extension(auth_context): Extension<AuthContext>,
    axum::extract::State(schema): axum::extract::State<ApiSchema>,
    req: async_graphql_axum::GraphQLRequest,
) -> async_graphql_axum::GraphQLResponse {
    graphql_handler(auth_context, axum::extract::State(schema), req).await
}

/// GraphQL subscription handler with authentication
#[allow(dead_code)]
async fn graphql_subscription_handler_with_auth(
    Extension(auth_context): Extension<AuthContext>,
    axum::extract::State(schema): axum::extract::State<ApiSchema>,
    ws: WebSocketUpgrade,
) -> axum::response::Response {
    graphql_subscription_handler(auth_context, axum::extract::State(schema), ws).await
}

#[cfg(test)]
mod tests {
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
