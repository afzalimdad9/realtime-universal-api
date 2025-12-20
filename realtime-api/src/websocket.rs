use anyhow::Result;
use axum::extract::ws::{Message, WebSocket};
use futures_util::{sink::SinkExt, stream::StreamExt};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use crate::api::AppState;
use crate::auth::AuthContext;
use crate::models::{Event, UsageMetric, UsageRecord};

/// WebSocket connection parameters
#[derive(Debug, Clone)]
pub struct WebSocketConnectionParams {
    pub tenant_id: String,
    pub project_id: String,
    pub topics: Vec<String>,
    pub auth_context: AuthContext,
}

/// WebSocket message types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum WebSocketMessage {
    /// Subscribe to topics
    Subscribe { topics: Vec<String> },
    /// Unsubscribe from topics
    Unsubscribe { topics: Vec<String> },
    /// Event delivery
    Event {
        id: String,
        topic: String,
        payload: serde_json::Value,
        published_at: String,
    },
    /// Connection acknowledgment
    Connected { connection_id: String },
    /// Error message
    Error { message: String },
    /// Ping/Pong for keepalive
    Ping,
    Pong,
}

/// WebSocket connection state
#[derive(Debug, Clone)]
pub struct WebSocketConnection {
    pub id: String,
    pub tenant_id: String,
    pub project_id: String,
    pub subscribed_topics: Vec<String>,
    pub sender: broadcast::Sender<WebSocketMessage>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Global WebSocket connection manager
#[derive(Debug, Clone)]
pub struct WebSocketManager {
    connections: Arc<Mutex<HashMap<String, WebSocketConnection>>>,
    connection_limits: Arc<Mutex<HashMap<String, i32>>>, // tenant_id -> limit
}

impl WebSocketManager {
    pub fn new() -> Self {
        Self {
            connections: Arc::new(Mutex::new(HashMap::new())),
            connection_limits: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Add a new connection
    pub fn add_connection(&self, connection: WebSocketConnection) -> Result<(), String> {
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
                "Connection limit exceeded for tenant {}: {}/{}",
                connection.tenant_id, tenant_connection_count, limit
            ));
        }
        
        connections.insert(connection.id.clone(), connection);
        Ok(())
    }

    /// Remove a connection
    pub fn remove_connection(&self, connection_id: &str) {
        let mut connections = self.connections.lock().unwrap();
        connections.remove(connection_id);
    }

    /// Get connections for a tenant/project/topic
    pub fn get_connections_for_event(&self, tenant_id: &str, project_id: &str, topic: &str) -> Vec<WebSocketConnection> {
        let connections = self.connections.lock().unwrap();
        connections
            .values()
            .filter(|conn| {
                conn.tenant_id == tenant_id
                    && conn.project_id == project_id
                    && (conn.subscribed_topics.is_empty() || conn.subscribed_topics.iter().any(|t| topic.starts_with(t)))
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
                let _ = conn.sender.send(WebSocketMessage::Error {
                    message: "Tenant suspended - connection terminated".to_string(),
                });
            }
        }
        
        // Remove connections
        connections.retain(|_, conn| conn.tenant_id != tenant_id);
        
        connection_ids
    }
}

// Global WebSocket manager instance
lazy_static::lazy_static! {
    static ref WEBSOCKET_MANAGER: WebSocketManager = WebSocketManager::new();
}

/// Handle a WebSocket connection
pub async fn handle_websocket_connection(
    socket: WebSocket,
    params: WebSocketConnectionParams,
    state: AppState,
) {
    let connection_id = Uuid::new_v4().to_string();
    
    info!(
        "New WebSocket connection {} for tenant/project: {}/{}",
        connection_id, params.tenant_id, params.project_id
    );

    // Create broadcast channel for this connection
    let (sender, mut receiver) = broadcast::channel(1000);
    
    // Create connection object
    let connection = WebSocketConnection {
        id: connection_id.clone(),
        tenant_id: params.tenant_id.clone(),
        project_id: params.project_id.clone(),
        subscribed_topics: params.topics.clone(),
        sender: sender.clone(),
        created_at: chrono::Utc::now(),
    };

    // Add connection to manager
    if let Err(e) = WEBSOCKET_MANAGER.add_connection(connection.clone()) {
        error!("Failed to add WebSocket connection: {}", e);
        return;
    }

    // Set connection limit based on project limits
    if let Ok(Some(project)) = state.database.get_project_with_tenant(&params.tenant_id, &params.project_id).await {
        WEBSOCKET_MANAGER.set_connection_limit(params.tenant_id.clone(), project.limits.max_connections);
    }

    // Split the socket into sender and receiver
    let (mut ws_sender, mut ws_receiver) = socket.split();

    // Send connection acknowledgment
    let connected_msg = WebSocketMessage::Connected {
        connection_id: connection_id.clone(),
    };
    
    if let Ok(msg_json) = serde_json::to_string(&connected_msg) {
        if let Err(e) = ws_sender.send(Message::Text(msg_json)).await {
            error!("Failed to send connection acknowledgment: {}", e);
            WEBSOCKET_MANAGER.remove_connection(&connection_id);
            return;
        }
    }

    // Subscribe to initial topics if provided
    if !params.topics.is_empty() {
        if let Err(e) = subscribe_to_topics(&state, &params.tenant_id, &params.project_id, &params.topics).await {
            warn!("Failed to subscribe to initial topics: {}", e);
        }
    }

    // Track WebSocket connection usage
    let usage_record = UsageRecord::new(
        params.tenant_id.clone(),
        params.project_id.clone(),
        UsageMetric::WebSocketMinutes,
        1,
        chrono::Utc::now().date_naive().and_hms_opt(0, 0, 0).unwrap().and_utc(),
    );
    
    if let Err(e) = state.database.create_usage_record(&usage_record).await {
        warn!("Failed to track WebSocket usage: {}", e);
    }

    // Spawn task to handle outgoing messages
    let connection_id_clone = connection_id.clone();
    let outgoing_task = tokio::spawn(async move {
        while let Ok(message) = receiver.recv().await {
            if let Ok(msg_json) = serde_json::to_string(&message) {
                if let Err(e) = ws_sender.send(Message::Text(msg_json)).await {
                    error!("Failed to send WebSocket message: {}", e);
                    break;
                }
            }
        }
        debug!("Outgoing message task ended for connection {}", connection_id_clone);
    });

    // Handle incoming messages
    let connection_id_clone = connection_id.clone();
    let state_clone = state.clone();
    let params_clone = params.clone();
    
    while let Some(msg) = ws_receiver.next().await {
        match msg {
            Ok(Message::Text(text)) => {
                if let Err(e) = handle_websocket_message(
                    &text,
                    &connection_id_clone,
                    &params_clone,
                    &state_clone,
                ).await {
                    error!("Error handling WebSocket message: {}", e);
                    
                    let error_msg = WebSocketMessage::Error {
                        message: format!("Message handling error: {}", e),
                    };
                    
                    if let Ok(error_json) = serde_json::to_string(&error_msg) {
                        let _ = sender.send(error_msg);
                    }
                }
            }
            Ok(Message::Close(_)) => {
                info!("WebSocket connection {} closed by client", connection_id_clone);
                break;
            }
            Ok(Message::Ping(data)) => {
                // Respond to ping with pong
                let pong_msg = WebSocketMessage::Pong;
                let _ = sender.send(pong_msg);
            }
            Ok(Message::Pong(_)) => {
                // Handle pong response
                debug!("Received pong from connection {}", connection_id_clone);
            }
            Ok(_) => {
                // Handle other message types (binary, etc.)
                debug!("Received non-text message from connection {}", connection_id_clone);
            }
            Err(e) => {
                error!("WebSocket error for connection {}: {}", connection_id_clone, e);
                break;
            }
        }
    }

    // Clean up connection
    info!("Cleaning up WebSocket connection {}", connection_id);
    WEBSOCKET_MANAGER.remove_connection(&connection_id);
    outgoing_task.abort();
}

/// Handle incoming WebSocket messages
async fn handle_websocket_message(
    message: &str,
    connection_id: &str,
    params: &WebSocketConnectionParams,
    state: &AppState,
) -> Result<()> {
    let ws_message: WebSocketMessage = serde_json::from_str(message)?;
    
    match ws_message {
        WebSocketMessage::Subscribe { topics } => {
            info!("Connection {} subscribing to topics: {:?}", connection_id, topics);
            subscribe_to_topics(state, &params.tenant_id, &params.project_id, &topics).await?;
            
            // Update connection's subscribed topics
            let mut connections = WEBSOCKET_MANAGER.connections.lock().unwrap();
            if let Some(conn) = connections.get_mut(connection_id) {
                conn.subscribed_topics.extend(topics);
                conn.subscribed_topics.sort();
                conn.subscribed_topics.dedup();
            }
        }
        WebSocketMessage::Unsubscribe { topics } => {
            info!("Connection {} unsubscribing from topics: {:?}", connection_id, topics);
            
            // Update connection's subscribed topics
            let mut connections = WEBSOCKET_MANAGER.connections.lock().unwrap();
            if let Some(conn) = connections.get_mut(connection_id) {
                conn.subscribed_topics.retain(|t| !topics.contains(t));
            }
        }
        WebSocketMessage::Ping => {
            // Send pong response
            let connections = WEBSOCKET_MANAGER.connections.lock().unwrap();
            if let Some(conn) = connections.get(connection_id) {
                let _ = conn.sender.send(WebSocketMessage::Pong);
            }
        }
        _ => {
            warn!("Received unexpected message type from connection {}", connection_id);
        }
    }
    
    Ok(())
}

/// Subscribe to topics for event delivery
async fn subscribe_to_topics(
    state: &AppState,
    tenant_id: &str,
    project_id: &str,
    topics: &[String],
) -> Result<()> {
    let consumer_name = format!("websocket_{}_{}", tenant_id, project_id);
    
    // Create subscription in event service
    let _subscription = state.event_service.create_subscription(
        tenant_id,
        project_id,
        topics.to_vec(),
        consumer_name,
        false, // Non-durable for WebSocket connections
    ).await?;
    
    debug!("Created subscription for topics: {:?}", topics);
    Ok(())
}

/// Broadcast an event to all relevant WebSocket connections
pub async fn broadcast_event_to_websockets(event: &Event) -> Result<()> {
    let connections = WEBSOCKET_MANAGER.get_connections_for_event(
        &event.tenant_id,
        &event.project_id,
        &event.topic,
    );
    
    if connections.is_empty() {
        debug!("No WebSocket connections found for event {}", event.id);
        return Ok(());
    }
    
    let ws_message = WebSocketMessage::Event {
        id: event.id.clone(),
        topic: event.topic.clone(),
        payload: event.payload.clone(),
        published_at: event.published_at.to_rfc3339(),
    };
    
    let mut delivered_count = 0;
    
    for connection in connections {
        if let Err(e) = connection.sender.send(ws_message.clone()) {
            warn!("Failed to send event to WebSocket connection {}: {}", connection.id, e);
        } else {
            delivered_count += 1;
        }
    }
    
    info!(
        "Broadcasted event {} to {} WebSocket connections",
        event.id, delivered_count
    );
    
    Ok(())
}

/// Terminate all WebSocket connections for a suspended tenant
pub async fn terminate_tenant_websocket_connections(tenant_id: &str) -> Vec<String> {
    info!("Terminating all WebSocket connections for suspended tenant: {}", tenant_id);
    WEBSOCKET_MANAGER.terminate_tenant_connections(tenant_id)
}

/// Get WebSocket connection statistics
pub fn get_websocket_stats() -> HashMap<String, serde_json::Value> {
    let connections = WEBSOCKET_MANAGER.connections.lock().unwrap();
    let mut stats = HashMap::new();
    
    stats.insert("total_connections".to_string(), serde_json::Value::Number(connections.len().into()));
    
    // Count connections per tenant
    let mut tenant_counts: HashMap<String, usize> = HashMap::new();
    for connection in connections.values() {
        *tenant_counts.entry(connection.tenant_id.clone()).or_insert(0) += 1;
    }
    
    stats.insert("connections_per_tenant".to_string(), serde_json::to_value(tenant_counts).unwrap());
    
    stats
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_websocket_manager_creation() {
        let manager = WebSocketManager::new();
        assert_eq!(manager.get_tenant_connection_count("test_tenant"), 0);
    }

    #[test]
    fn test_websocket_message_serialization() {
        let message = WebSocketMessage::Connected {
            connection_id: "test_123".to_string(),
        };
        
        let json = serde_json::to_string(&message).unwrap();
        assert!(json.contains("Connected"));
        assert!(json.contains("test_123"));
        
        let deserialized: WebSocketMessage = serde_json::from_str(&json).unwrap();
        match deserialized {
            WebSocketMessage::Connected { connection_id } => {
                assert_eq!(connection_id, "test_123");
            }
            _ => panic!("Wrong message type"),
        }
    }

    #[test]
    fn test_connection_limits() {
        let manager = WebSocketManager::new();
        manager.set_connection_limit("tenant_1".to_string(), 2);
        
        let (sender, _) = broadcast::channel(100);
        
        // Add first connection
        let conn1 = WebSocketConnection {
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
        let conn2 = WebSocketConnection {
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
        let conn3 = WebSocketConnection {
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