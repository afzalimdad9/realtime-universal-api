use anyhow::Result;
use axum::{
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse, Response,
    },
};
use futures_util::{stream::{self, Stream}, StreamExt};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::broadcast;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use crate::api::AppState;
use crate::auth::{extract_auth_header, AuthContext, AuthError};
use crate::models::{Event as EventModel, Scope, UsageMetric, UsageRecord};

/// SSE connection query parameters
#[derive(Debug, Deserialize)]
pub struct SSEQuery {
    pub topics: Option<String>, // Comma-separated list of topics
}

/// SSE connection parameters
#[derive(Debug, Clone)]
pub struct SSEConnectionParams {
    pub tenant_id: String,
    pub project_id: String,
    pub topics: Vec<String>,
    pub auth_context: AuthContext,
}

/// SSE message types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum SSEMessage {
    /// Event delivery
    Event {
        id: String,
        topic: String,
        payload: serde_json::Value,
        published_at: String,
    },
    /// Connection acknowledgment
    Connected {
        connection_id: String,
    },
    /// Error message
    Error {
        message: String,
    },
    /// Heartbeat for connection health
    Heartbeat {
        timestamp: String,
    },
}

/// SSE connection state
#[derive(Debug, Clone)]
pub struct SSEConnection {
    pub id: String,
    pub tenant_id: String,
    pub project_id: String,
    pub subscribed_topics: Vec<String>,
    pub sender: broadcast::Sender<SSEMessage>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Global SSE connection manager
#[derive(Debug, Clone)]
pub struct SSEManager {
    connections: Arc<Mutex<HashMap<String, SSEConnection>>>,
    connection_limits: Arc<Mutex<HashMap<String, i32>>>, // tenant_id -> limit
}

impl Default for SSEManager {
    fn default() -> Self {
        Self::new()
    }
}

impl SSEManager {
    pub fn new() -> Self {
        Self {
            connections: Arc::new(Mutex::new(HashMap::new())),
            connection_limits: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Add a new SSE connection
    pub fn add_connection(&self, connection: SSEConnection) -> Result<(), String> {
        let mut connections = self.connections.lock().unwrap();

        // Check connection limits
        let tenant_connection_count = connections
            .values()
            .filter(|conn| conn.tenant_id == connection.tenant_id)
            .count();

        let limits = self.connection_limits.lock().unwrap();
        let limit = limits.get(&connection.tenant_id).unwrap_or(&1000); // Default limit

        if tenant_connection_count >= *limit as usize {
            return Err(format!(
                "SSE connection limit exceeded for tenant {}: {}/{}",
                connection.tenant_id, tenant_connection_count, limit
            ));
        }

        connections.insert(connection.id.clone(), connection);
        Ok(())
    }

    /// Remove an SSE connection
    pub fn remove_connection(&self, connection_id: &str) {
        let mut connections = self.connections.lock().unwrap();
        connections.remove(connection_id);
    }

    /// Get connections for a tenant/project/topic
    pub fn get_connections_for_event(
        &self,
        tenant_id: &str,
        project_id: &str,
        topic: &str,
    ) -> Vec<SSEConnection> {
        let connections = self.connections.lock().unwrap();
        connections
            .values()
            .filter(|conn| {
                conn.tenant_id == tenant_id
                    && conn.project_id == project_id
                    && (conn.subscribed_topics.is_empty()
                        || conn.subscribed_topics.iter().any(|t| topic.starts_with(t)))
            })
            .cloned()
            .collect()
    }

    /// Get connection count for a tenant
    pub fn get_tenant_connection_count(&self, tenant_id: &str) -> usize {
        let connections = self.connections.lock().unwrap();
        connections
            .values()
            .filter(|conn| conn.tenant_id == tenant_id)
            .count()
    }

    /// Set connection limit for a tenant
    pub fn set_connection_limit(&self, tenant_id: String, limit: i32) {
        let mut limits = self.connection_limits.lock().unwrap();
        limits.insert(tenant_id, limit);
    }

    /// Terminate all connections for a tenant (for suspension)
    pub fn terminate_tenant_connections(&self, tenant_id: &str) -> Vec<String> {
        let mut connections = self.connections.lock().unwrap();
        let connection_ids: Vec<String> = connections
            .iter()
            .filter(|(_, conn)| conn.tenant_id == tenant_id)
            .map(|(id, _)| id.clone())
            .collect();

        // Send termination message to all tenant connections
        for (_, conn) in connections.iter() {
            if conn.tenant_id == tenant_id {
                let _ = conn.sender.send(SSEMessage::Error {
                    message: "Tenant suspended - connection terminated".to_string(),
                });
            }
        }

        // Remove connections
        connections.retain(|_, conn| conn.tenant_id != tenant_id);

        connection_ids
    }
}

// Global SSE manager instance
lazy_static::lazy_static! {
    static ref SSE_MANAGER: SSEManager = SSEManager::new();
}

/// SSE handler with authentication and subscription management
pub async fn sse_handler(
    State(state): State<AppState>,
    Query(params): Query<SSEQuery>,
    headers: HeaderMap,
) -> Result<Response, StatusCode> {
    // Extract authentication from headers
    let auth_value = match extract_auth_header(&headers) {
        Ok(value) => value,
        Err(_) => return Err(StatusCode::UNAUTHORIZED),
    };

    // Validate authentication
    let auth_context = match state.auth_service.validate_api_key(&auth_value).await {
        Ok(context) => context,
        Err(AuthError::InvalidApiKey) => {
            // Try JWT validation as fallback
            match state.auth_service.validate_jwt(&auth_value).await {
                Ok(context) => context,
                Err(_) => return Err(StatusCode::UNAUTHORIZED),
            }
        }
        Err(AuthError::RateLimitExceeded) => {
            return Err(StatusCode::TOO_MANY_REQUESTS);
        }
        Err(AuthError::TenantSuspended) => {
            return Err(StatusCode::FORBIDDEN);
        }
        Err(_) => return Err(StatusCode::UNAUTHORIZED),
    };

    // Check if the API key has subscribe permissions
    if state
        .auth_service
        .check_scope(&auth_context, &Scope::EventsSubscribe)
        .is_err()
    {
        return Err(StatusCode::FORBIDDEN);
    }

    // Parse topics from query parameters
    let topics = params
        .topics
        .map(|t| t.split(',').map(|s| s.trim().to_string()).collect())
        .unwrap_or_else(Vec::new);

    // Create connection parameters
    let connection_params = SSEConnectionParams {
        tenant_id: auth_context.tenant_id.clone(),
        project_id: auth_context.project_id.clone(),
        topics,
        auth_context,
    };

    // Create SSE stream
    let stream = create_sse_stream(connection_params, state).await;

    // Return SSE response
    Ok(Sse::new(stream)
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(30)))
        .into_response())
}

/// Create SSE stream for a connection
async fn create_sse_stream(
    params: SSEConnectionParams,
    state: AppState,
) -> impl Stream<Item = Result<Event, Infallible>> {
    let connection_id = Uuid::new_v4().to_string();

    info!(
        "New SSE connection {} for tenant/project: {}/{}",
        connection_id, params.tenant_id, params.project_id
    );

    // Create broadcast channel for this connection
    let (sender, mut receiver) = broadcast::channel(1000);

    // Create connection object
    let connection = SSEConnection {
        id: connection_id.clone(),
        tenant_id: params.tenant_id.clone(),
        project_id: params.project_id.clone(),
        subscribed_topics: params.topics.clone(),
        sender: sender.clone(),
        created_at: chrono::Utc::now(),
    };

    // Add connection to manager
    if let Err(e) = SSE_MANAGER.add_connection(connection.clone()) {
        error!("Failed to add SSE connection: {}", e);
        return stream::once(async move {
            Ok(Event::default()
                .event("error")
                .data(format!("Connection failed: {}", e)))
        })
        .boxed();
    }

    // Set connection limit based on project limits
    if let Ok(Some(project)) = state
        .database
        .get_project_with_tenant(&params.tenant_id, &params.project_id)
        .await
    {
        SSE_MANAGER.set_connection_limit(params.tenant_id.clone(), project.limits.max_connections);
    }

    // Subscribe to initial topics if provided
    if !params.topics.is_empty() {
        if let Err(e) = subscribe_to_topics(
            &state,
            &params.tenant_id,
            &params.project_id,
            &params.topics,
        )
        .await
        {
            warn!("Failed to subscribe to initial topics: {}", e);
        }
    }

    // Track SSE connection usage
    let usage_record = UsageRecord::new(
        params.tenant_id.clone(),
        params.project_id.clone(),
        UsageMetric::WebSocketMinutes, // Reuse WebSocket metric for SSE
        1,
        chrono::Utc::now()
            .date_naive()
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_utc(),
    );

    if let Err(e) = state.database.create_usage_record(&usage_record).await {
        warn!("Failed to track SSE usage: {}", e);
    }

    // Send connection acknowledgment
    let connected_msg = SSEMessage::Connected {
        connection_id: connection_id.clone(),
    };
    let _ = sender.send(connected_msg);

    // Create the stream that converts broadcast messages to SSE events
    let connection_id_clone = connection_id.clone();
    let stream = async_stream::stream! {
        while let Ok(message) = receiver.recv().await {
            match message {
                SSEMessage::Event { id, topic, payload, published_at } => {
                    let event_data = serde_json::json!({
                        "id": id,
                        "topic": topic,
                        "payload": payload,
                        "published_at": published_at
                    });
                    
                    if let Ok(data_str) = serde_json::to_string(&event_data) {
                        yield Ok(Event::default()
                            .event("event")
                            .id(id)
                            .data(data_str));
                    }
                }
                SSEMessage::Connected { connection_id } => {
                    let connect_data = serde_json::json!({
                        "connection_id": connection_id,
                        "status": "connected"
                    });
                    
                    if let Ok(data_str) = serde_json::to_string(&connect_data) {
                        yield Ok(Event::default()
                            .event("connected")
                            .data(data_str));
                    }
                }
                SSEMessage::Error { message } => {
                    let error_data = serde_json::json!({
                        "error": message
                    });
                    
                    if let Ok(data_str) = serde_json::to_string(&error_data) {
                        yield Ok(Event::default()
                            .event("error")
                            .data(data_str));
                    }
                    break; // Close connection on error
                }
                SSEMessage::Heartbeat { timestamp } => {
                    let heartbeat_data = serde_json::json!({
                        "timestamp": timestamp
                    });
                    
                    if let Ok(data_str) = serde_json::to_string(&heartbeat_data) {
                        yield Ok(Event::default()
                            .event("heartbeat")
                            .data(data_str));
                    }
                }
            }
        }
        
        // Clean up connection when stream ends
        debug!("SSE stream ended for connection {}", connection_id_clone);
        SSE_MANAGER.remove_connection(&connection_id_clone);
    };

    Box::pin(stream)
}

/// Subscribe to topics for event delivery
async fn subscribe_to_topics(
    state: &AppState,
    tenant_id: &str,
    project_id: &str,
    topics: &[String],
) -> Result<()> {
    let consumer_name = format!("sse_{}_{}", tenant_id, project_id);

    // Create subscription in event service
    let _subscription = state
        .event_service
        .create_subscription(
            tenant_id,
            project_id,
            topics.to_vec(),
            consumer_name,
            false, // Non-durable for SSE connections
        )
        .await?;

    debug!("Created SSE subscription for topics: {:?}", topics);
    Ok(())
}

/// Broadcast an event to all relevant SSE connections
pub async fn broadcast_event_to_sse(event: &EventModel) -> Result<()> {
    let connections = SSE_MANAGER.get_connections_for_event(
        &event.tenant_id,
        &event.project_id,
        &event.topic,
    );

    if connections.is_empty() {
        debug!("No SSE connections found for event {}", event.id);
        return Ok(());
    }

    let sse_message = SSEMessage::Event {
        id: event.id.clone(),
        topic: event.topic.clone(),
        payload: event.payload.clone(),
        published_at: event.published_at.to_rfc3339(),
    };

    let mut delivered_count = 0;

    for connection in connections {
        if let Err(e) = connection.sender.send(sse_message.clone()) {
            warn!(
                "Failed to send event to SSE connection {}: {}",
                connection.id, e
            );
        } else {
            delivered_count += 1;
        }
    }

    info!(
        "Broadcasted event {} to {} SSE connections",
        event.id, delivered_count
    );

    Ok(())
}

/// Terminate all SSE connections for a suspended tenant
pub async fn terminate_tenant_sse_connections(tenant_id: &str) -> Vec<String> {
    info!(
        "Terminating all SSE connections for suspended tenant: {}",
        tenant_id
    );
    SSE_MANAGER.terminate_tenant_connections(tenant_id)
}

/// Get SSE connection statistics
pub fn get_sse_stats() -> HashMap<String, serde_json::Value> {
    let connections = SSE_MANAGER.connections.lock().unwrap();
    let mut stats = HashMap::new();

    stats.insert(
        "total_connections".to_string(),
        serde_json::Value::Number(connections.len().into()),
    );

    // Count connections per tenant
    let mut tenant_counts: HashMap<String, usize> = HashMap::new();
    for connection in connections.values() {
        *tenant_counts
            .entry(connection.tenant_id.clone())
            .or_insert(0) += 1;
    }

    stats.insert(
        "connections_per_tenant".to_string(),
        serde_json::to_value(tenant_counts).unwrap(),
    );

    stats
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sse_manager_creation() {
        let manager = SSEManager::new();
        assert_eq!(manager.get_tenant_connection_count("test_tenant"), 0);
    }

    #[test]
    fn test_sse_message_serialization() {
        let message = SSEMessage::Connected {
            connection_id: "test_123".to_string(),
        };

        let json = serde_json::to_string(&message).unwrap();
        assert!(json.contains("Connected"));
        assert!(json.contains("test_123"));

        let deserialized: SSEMessage = serde_json::from_str(&json).unwrap();
        match deserialized {
            SSEMessage::Connected { connection_id } => {
                assert_eq!(connection_id, "test_123");
            }
            _ => panic!("Wrong message type"),
        }
    }

    #[test]
    fn test_sse_connection_limits() {
        let manager = SSEManager::new();
        manager.set_connection_limit("tenant_1".to_string(), 2);

        let (sender, _) = broadcast::channel(100);

        // Add first connection
        let conn1 = SSEConnection {
            id: "conn_1".to_string(),
            tenant_id: "tenant_1".to_string(),
            project_id: "project_1".to_string(),
            subscribed_topics: vec![],
            sender: sender.clone(),
            created_at: chrono::Utc::now(),
        };

        assert!(manager.add_connection(conn1).is_ok());
        assert_eq!(manager.get_tenant_connection_count("tenant_1"), 1);

        // Add second connection
        let conn2 = SSEConnection {
            id: "conn_2".to_string(),
            tenant_id: "tenant_1".to_string(),
            project_id: "project_1".to_string(),
            subscribed_topics: vec![],
            sender: sender.clone(),
            created_at: chrono::Utc::now(),
        };

        assert!(manager.add_connection(conn2).is_ok());
        assert_eq!(manager.get_tenant_connection_count("tenant_1"), 2);

        // Try to add third connection (should fail)
        let conn3 = SSEConnection {
            id: "conn_3".to_string(),
            tenant_id: "tenant_1".to_string(),
            project_id: "project_1".to_string(),
            subscribed_topics: vec![],
            sender: sender.clone(),
            created_at: chrono::Utc::now(),
        };

        assert!(manager.add_connection(conn3).is_err());
        assert_eq!(manager.get_tenant_connection_count("tenant_1"), 2);
    }
}