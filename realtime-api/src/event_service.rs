use anyhow::{anyhow, Result};
use std::sync::Arc;
use tracing::{error, info, warn};

use crate::database::Database;
use crate::models::{Event, UsageMetric, UsageRecord};
use crate::nats::{NatsClient, ReplayRequest, SubscriptionConfig};
use crate::schema_validator::SchemaValidator;

/// Event publishing service with tenant/project scoping
#[derive(Debug, Clone)]
pub struct EventService {
    database: Database,
    nats_client: NatsClient,
    schema_validator: Arc<SchemaValidator>,
}

/// Event publishing result
#[derive(Debug)]
pub struct PublishResult {
    pub event_id: String,
    pub sequence: u64,
    pub published_at: chrono::DateTime<chrono::Utc>,
}

/// Event subscription handle
#[derive(Debug)]
pub struct EventSubscription {
    pub consumer_name: String,
    pub tenant_id: String,
    pub project_id: String,
    pub topics: Vec<String>,
}

impl EventService {
    /// Create a new event service
    pub fn new(
        database: Database,
        nats_client: NatsClient,
        schema_validator: SchemaValidator,
    ) -> Self {
        Self {
            database,
            nats_client,
            schema_validator: Arc::new(schema_validator),
        }
    }

    /// Publish an event with validation and persistence
    pub async fn publish_event(
        &self,
        tenant_id: &str,
        project_id: &str,
        topic: &str,
        payload: serde_json::Value,
    ) -> Result<PublishResult> {
        // Validate tenant and project exist and are active
        let tenant = self.database.get_tenant(tenant_id).await?
            .ok_or_else(|| anyhow!("Tenant not found: {}", tenant_id))?;

        if !tenant.is_active() {
            return Err(anyhow!("Tenant is not active: {}", tenant_id));
        }

        let _project = self.database.get_project(tenant_id, project_id).await?
            .ok_or_else(|| anyhow!("Project not found: {}", project_id))?;

        // Validate event payload against topic schema
        if let Err(e) = self.schema_validator.validate_event_payload(topic, &payload) {
            warn!("Event validation failed for topic {}: {}", topic, e);
            return Err(anyhow!("Event validation failed: {}", e));
        }

        // Create the event
        let event = Event::new(
            tenant_id.to_string(),
            project_id.to_string(),
            topic.to_string(),
            payload,
        );

        // Publish to NATS JetStream first (for durability)
        let sequence = self.nats_client.publish_event(&event).await?;

        // Store event metadata in PostgreSQL
        if let Err(e) = self.database.create_event(&event).await {
            error!("Failed to store event metadata in database: {}", e);
            // Note: Event is already in NATS, so we don't fail the publish
            // but we log the error for monitoring
        }

        // Track usage metrics
        let usage_record = UsageRecord::new(
            tenant_id.to_string(),
            project_id.to_string(),
            UsageMetric::EventsPublished,
            1,
            chrono::Utc::now().date_naive().and_hms_opt(0, 0, 0).unwrap().and_utc(),
        );

        if let Err(e) = self.database.create_usage_record(&usage_record).await {
            error!("Failed to track usage metrics: {}", e);
            // Don't fail the publish for usage tracking errors
        }

        info!(
            "Published event {} to topic {} for tenant/project: {}/{}",
            event.id, topic, tenant_id, project_id
        );

        Ok(PublishResult {
            event_id: event.id,
            sequence,
            published_at: event.published_at,
        })
    }

    /// Create a subscription for WebSocket/SSE delivery
    pub async fn create_subscription(
        &self,
        tenant_id: &str,
        project_id: &str,
        topics: Vec<String>,
        consumer_name: String,
        durable: bool,
    ) -> Result<EventSubscription> {
        // Validate tenant and project
        let tenant = self.database.get_tenant(tenant_id).await?
            .ok_or_else(|| anyhow!("Tenant not found: {}", tenant_id))?;

        if !tenant.is_active() {
            return Err(anyhow!("Tenant is not active: {}", tenant_id));
        }

        let _project = self.database.get_project(tenant_id, project_id).await?
            .ok_or_else(|| anyhow!("Project not found: {}", project_id))?;

        // Create subscription configuration
        let config = SubscriptionConfig {
            tenant_id: tenant_id.to_string(),
            project_id: project_id.to_string(),
            topics: topics.clone(),
            consumer_name: consumer_name.clone(),
            durable,
        };

        // Create the consumer in NATS
        self.nats_client.create_consumer(&config).await?;

        info!(
            "Created subscription '{}' for tenant/project: {}/{} with topics: {:?}",
            consumer_name, tenant_id, project_id, topics
        );

        Ok(EventSubscription {
            consumer_name,
            tenant_id: tenant_id.to_string(),
            project_id: project_id.to_string(),
            topics,
        })
    }

    /// Replay events with cursor support
    pub async fn replay_events(
        &self,
        tenant_id: &str,
        project_id: &str,
        topic: Option<String>,
        cursor: Option<crate::nats::EventCursor>,
        limit: Option<usize>,
    ) -> Result<Vec<(Event, crate::nats::EventCursor)>> {
        // Validate tenant and project
        let tenant = self.database.get_tenant(tenant_id).await?
            .ok_or_else(|| anyhow!("Tenant not found: {}", tenant_id))?;

        if !tenant.is_active() {
            return Err(anyhow!("Tenant is not active: {}", tenant_id));
        }

        let _project = self.database.get_project(tenant_id, project_id).await?
            .ok_or_else(|| anyhow!("Project not found: {}", project_id))?;

        // Create replay request
        let request = ReplayRequest {
            tenant_id: tenant_id.to_string(),
            project_id: project_id.to_string(),
            topic,
            cursor,
            limit,
        };

        // Get events from NATS
        let events = self.nats_client.replay_events(&request).await?;

        info!(
            "Replayed {} events for tenant/project: {}/{}",
            events.len(), tenant_id, project_id
        );

        Ok(events)
    }

    /// Delete a subscription
    pub async fn delete_subscription(&self, consumer_name: &str) -> Result<()> {
        self.nats_client.delete_consumer(consumer_name).await?;
        info!("Deleted subscription: {}", consumer_name);
        Ok(())
    }

    /// Get stream statistics
    pub async fn get_stream_stats(&self) -> Result<std::collections::HashMap<String, serde_json::Value>> {
        self.nats_client.get_stream_info().await
    }

    /// Check if the service is healthy
    pub fn is_healthy(&self) -> bool {
        self.nats_client.is_connected()
    }

    /// Get the underlying NATS client
    pub fn nats_client(&self) -> &NatsClient {
        &self.nats_client
    }

    /// Get the database connection
    pub fn database(&self) -> &Database {
        &self.database
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{BillingPlan, Project, Tenant};

    #[test]
    fn test_publish_result() {
        let result = PublishResult {
            event_id: "event_123".to_string(),
            sequence: 42,
            published_at: chrono::Utc::now(),
        };

        assert_eq!(result.event_id, "event_123");
        assert_eq!(result.sequence, 42);
    }

    #[test]
    fn test_event_subscription() {
        let subscription = EventSubscription {
            consumer_name: "websocket_consumer".to_string(),
            tenant_id: "tenant_123".to_string(),
            project_id: "project_456".to_string(),
            topics: vec!["user.created".to_string(), "user.updated".to_string()],
        };

        assert_eq!(subscription.consumer_name, "websocket_consumer");
        assert_eq!(subscription.topics.len(), 2);
    }
}