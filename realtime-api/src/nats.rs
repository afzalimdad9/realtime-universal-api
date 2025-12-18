use anyhow::{anyhow, Result};
use async_nats::jetstream::{
    consumer::{pull::Config as ConsumerConfig, DeliverPolicy},
    stream::{Config as StreamConfig, RetentionPolicy, StorageType},
    Context as JetStreamContext,
};
use chrono::{DateTime, Utc};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;
use tracing::{error, info, warn};

use crate::models::Event;

/// NATS JetStream client for event streaming and persistence
#[derive(Debug, Clone)]
pub struct NatsClient {
    client: async_nats::Client,
    jetstream: JetStreamContext,
    stream_name: String,
}

/// Event cursor for replay functionality
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventCursor {
    pub sequence: u64,
    pub timestamp: DateTime<Utc>,
}

/// Event replay request
#[derive(Debug, Clone)]
pub struct ReplayRequest {
    pub tenant_id: String,
    pub project_id: String,
    pub topic: Option<String>,
    pub cursor: Option<EventCursor>,
    pub limit: Option<usize>,
}

/// Event subscription configuration
#[derive(Debug, Clone)]
pub struct SubscriptionConfig {
    pub tenant_id: String,
    pub project_id: String,
    pub topics: Vec<String>,
    pub consumer_name: String,
    pub durable: bool,
}

impl NatsClient {
    /// Create a new NATS client and initialize JetStream
    pub async fn new(nats_url: &str, stream_name: String) -> Result<Self> {
        let client = async_nats::connect(nats_url).await?;
        let jetstream = async_nats::jetstream::new(client.clone());

        let nats_client = Self {
            client,
            jetstream,
            stream_name: stream_name.clone(),
        };

        // Initialize the stream
        nats_client.initialize_stream().await?;

        info!("NATS JetStream client initialized with stream: {}", stream_name);
        Ok(nats_client)
    }

    /// Initialize the JetStream stream for events
    async fn initialize_stream(&self) -> Result<()> {
        let stream_config = StreamConfig {
            name: self.stream_name.clone(),
            subjects: vec![format!("events.*.*.>")], // events.{tenant_id}.{project_id}.{topic}
            retention: RetentionPolicy::Limits,
            storage: StorageType::File,
            max_age: Duration::from_secs(30 * 24 * 60 * 60), // 30 days
            max_bytes: 1024 * 1024 * 1024 * 10, // 10GB
            max_messages: 1_000_000,
            ..Default::default()
        };

        match self.jetstream.get_or_create_stream(stream_config).await {
            Ok(_) => {
                info!("JetStream stream '{}' initialized successfully", self.stream_name);
                Ok(())
            }
            Err(e) => {
                error!("Failed to initialize JetStream stream: {}", e);
                Err(anyhow!("Failed to initialize JetStream stream: {}", e))
            }
        }
    }

    /// Publish an event to JetStream with tenant/project scoping
    pub async fn publish_event(&self, event: &Event) -> Result<u64> {
        let subject = format!("events.{}.{}.{}", event.tenant_id, event.project_id, event.topic);
        
        // Serialize the event
        let payload = serde_json::to_vec(event)?;
        
        // Add metadata headers
        let mut headers = async_nats::HeaderMap::new();
        headers.insert("tenant_id", event.tenant_id.as_str());
        headers.insert("project_id", event.project_id.as_str());
        headers.insert("topic", event.topic.as_str());
        headers.insert("event_id", event.id.as_str());
        headers.insert("published_at", event.published_at.to_rfc3339().as_str());

        // Publish to JetStream
        let ack = self
            .jetstream
            .publish_with_headers(subject, headers, payload.into())
            .await?;

        let ack_result = ack.await?;
        let sequence = ack_result.sequence;
        
        info!(
            "Published event {} to JetStream with sequence: {}",
            event.id, sequence
        );
        
        Ok(sequence)
    }

    /// Create a durable consumer for WebSocket/SSE delivery
    pub async fn create_consumer(&self, config: &SubscriptionConfig) -> Result<()> {
        let filter_subjects: Vec<String> = if config.topics.is_empty() {
            // Subscribe to all topics for this tenant/project
            vec![format!("events.{}.{}.>", config.tenant_id, config.project_id)]
        } else {
            // Subscribe to specific topics
            config.topics
                .iter()
                .map(|topic| format!("events.{}.{}.{}", config.tenant_id, config.project_id, topic))
                .collect()
        };

        let consumer_config = ConsumerConfig {
            name: Some(config.consumer_name.clone()),
            durable_name: if config.durable { Some(config.consumer_name.clone()) } else { None },
            deliver_policy: DeliverPolicy::New,
            filter_subjects,
            ..Default::default()
        };

        // Get the stream first, then create consumer
        let stream = self.jetstream.get_stream(&self.stream_name).await?;
        
        match stream.create_consumer(consumer_config).await {
            Ok(_) => {
                info!("Created consumer '{}' for tenant/project: {}/{}", 
                      config.consumer_name, config.tenant_id, config.project_id);
                Ok(())
            }
            Err(e) => {
                error!("Failed to create consumer: {}", e);
                Err(anyhow!("Failed to create consumer: {}", e))
            }
        }
    }

    /// Get events for replay with cursor support
    pub async fn replay_events(&self, request: &ReplayRequest) -> Result<Vec<(Event, EventCursor)>> {
        let subject_filter = if let Some(topic) = &request.topic {
            format!("events.{}.{}.{}", request.tenant_id, request.project_id, topic)
        } else {
            format!("events.{}.{}.>", request.tenant_id, request.project_id)
        };

        // Create a temporary consumer for replay
        let consumer_name = format!("replay_{}_{}", request.tenant_id, chrono::Utc::now().timestamp());
        
        let deliver_policy = if let Some(cursor) = &request.cursor {
            DeliverPolicy::ByStartSequence {
                start_sequence: cursor.sequence,
            }
        } else {
            DeliverPolicy::All
        };

        let consumer_config = ConsumerConfig {
            name: Some(consumer_name.clone()),
            deliver_policy,
            filter_subjects: vec![subject_filter],
            ..Default::default()
        };

        let stream = self.jetstream.get_stream(&self.stream_name).await?;
        let consumer = stream.create_consumer(consumer_config).await?;

        let mut events = Vec::new();
        let limit = request.limit.unwrap_or(100);
        
        // Fetch messages
        let mut messages = consumer.messages().await?;
        
        for _ in 0..limit {
            if let Some(message) = messages.next().await {
                match message {
                    Ok(msg) => {
                        // Deserialize the event
                        match serde_json::from_slice::<Event>(&msg.payload) {
                            Ok(event) => {
                                let cursor = EventCursor {
                                    sequence: msg.info().unwrap().stream_sequence,
                                    timestamp: event.published_at,
                                };
                                events.push((event, cursor));
                                
                                // Acknowledge the message
                                if let Err(e) = msg.ack().await {
                                    warn!("Failed to ack message: {}", e);
                                }
                            }
                            Err(e) => {
                                error!("Failed to deserialize event: {}", e);
                                if let Err(e) = msg.ack().await {
                                    warn!("Failed to ack invalid message: {}", e);
                                }
                            }
                        }
                    }
                    Err(e) => {
                        error!("Error receiving message: {}", e);
                        break;
                    }
                }
            } else {
                break;
            }
        }

        // Clean up temporary consumer
        let stream = self.jetstream.get_stream(&self.stream_name).await?;
        if let Err(e) = stream.delete_consumer(&consumer_name).await {
            warn!("Failed to delete temporary consumer: {}", e);
        }

        info!("Replayed {} events for tenant/project: {}/{}", 
              events.len(), request.tenant_id, request.project_id);
        
        Ok(events)
    }

    /// Get stream information and statistics
    pub async fn get_stream_info(&self) -> Result<HashMap<String, serde_json::Value>> {
        let mut stream = self.jetstream.get_stream(&self.stream_name).await?;
        let info = stream.info().await?;
        
        let mut stats = HashMap::new();
        stats.insert("name".to_string(), serde_json::Value::String(info.config.name.clone()));
        stats.insert("messages".to_string(), serde_json::Value::Number(info.state.messages.into()));
        stats.insert("bytes".to_string(), serde_json::Value::Number(info.state.bytes.into()));
        stats.insert("first_seq".to_string(), serde_json::Value::Number(info.state.first_sequence.into()));
        stats.insert("last_seq".to_string(), serde_json::Value::Number(info.state.last_sequence.into()));
        
        Ok(stats)
    }

    /// Delete a consumer
    pub async fn delete_consumer(&self, consumer_name: &str) -> Result<()> {
        let stream = self.jetstream.get_stream(&self.stream_name).await?;
        stream.delete_consumer(consumer_name).await?;
        info!("Deleted consumer: {}", consumer_name);
        Ok(())
    }

    /// Check if the client is connected
    pub fn is_connected(&self) -> bool {
        self.client.connection_state() == async_nats::connection::State::Connected
    }

    /// Get the underlying NATS client
    pub fn client(&self) -> &async_nats::Client {
        &self.client
    }

    /// Get the JetStream context
    pub fn jetstream(&self) -> &JetStreamContext {
        &self.jetstream
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Event};
    use chrono::Utc;

    #[tokio::test]
    async fn test_event_cursor_serialization() {
        let cursor = EventCursor {
            sequence: 12345,
            timestamp: Utc::now(),
        };

        let serialized = serde_json::to_string(&cursor).unwrap();
        let deserialized: EventCursor = serde_json::from_str(&serialized).unwrap();

        assert_eq!(cursor.sequence, deserialized.sequence);
        assert_eq!(cursor.timestamp, deserialized.timestamp);
    }

    #[test]
    fn test_subscription_config() {
        let config = SubscriptionConfig {
            tenant_id: "tenant_123".to_string(),
            project_id: "project_456".to_string(),
            topics: vec!["user.created".to_string(), "user.updated".to_string()],
            consumer_name: "websocket_consumer".to_string(),
            durable: true,
        };

        assert_eq!(config.tenant_id, "tenant_123");
        assert_eq!(config.topics.len(), 2);
        assert!(config.durable);
    }

    #[test]
    fn test_replay_request() {
        let cursor = EventCursor {
            sequence: 100,
            timestamp: Utc::now(),
        };

        let request = ReplayRequest {
            tenant_id: "tenant_123".to_string(),
            project_id: "project_456".to_string(),
            topic: Some("user.created".to_string()),
            cursor: Some(cursor.clone()),
            limit: Some(50),
        };

        assert_eq!(request.tenant_id, "tenant_123");
        assert_eq!(request.topic, Some("user.created".to_string()));
        assert_eq!(request.cursor.unwrap().sequence, 100);
        assert_eq!(request.limit, Some(50));
    }
}