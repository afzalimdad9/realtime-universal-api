use anyhow::Result;
use reqwest::Client;
use serde_json::json;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{error, info, warn};

use crate::config::ObservabilityConfig;

/// Alert severity levels
#[derive(Debug, Clone, PartialEq)]
pub enum AlertSeverity {
    Info,
    Warning,
    Error,
    Critical,
}

impl AlertSeverity {
    pub fn as_str(&self) -> &'static str {
        match self {
            AlertSeverity::Info => "info",
            AlertSeverity::Warning => "warning", 
            AlertSeverity::Error => "error",
            AlertSeverity::Critical => "critical",
        }
    }
}

/// Alert message structure
#[derive(Debug, Clone)]
pub struct Alert {
    pub severity: AlertSeverity,
    pub title: String,
    pub message: String,
    pub context: serde_json::Value,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

/// Alerting service for sending notifications about errors and performance issues
#[derive(Clone)]
pub struct AlertingService {
    config: ObservabilityConfig,
    client: Client,
    alert_count: Arc<RwLock<u64>>,
}

impl AlertingService {
    pub fn new(config: ObservabilityConfig) -> Self {
        Self {
            config,
            client: Client::new(),
            alert_count: Arc::new(RwLock::new(0)),
        }
    }

    /// Send an alert if alerting is enabled
    pub async fn send_alert(&self, alert: Alert) -> Result<()> {
        if !self.config.enable_alerts {
            return Ok(());
        }

        // Increment alert count
        {
            let mut count = self.alert_count.write().await;
            *count += 1;
        }

        // Log the alert locally
        match alert.severity {
            AlertSeverity::Info => info!(
                title = alert.title,
                message = alert.message,
                context = ?alert.context,
                "Alert generated"
            ),
            AlertSeverity::Warning => warn!(
                title = alert.title,
                message = alert.message,
                context = ?alert.context,
                "Alert generated"
            ),
            AlertSeverity::Error | AlertSeverity::Critical => error!(
                title = alert.title,
                message = alert.message,
                context = ?alert.context,
                "Alert generated"
            ),
        }

        // Send to webhook if configured
        if let Some(webhook_url) = &self.config.alert_webhook_url {
            self.send_webhook_alert(webhook_url, &alert).await?;
        }

        Ok(())
    }

    /// Send alert to webhook endpoint
    async fn send_webhook_alert(&self, webhook_url: &str, alert: &Alert) -> Result<()> {
        let payload = json!({
            "severity": alert.severity.as_str(),
            "title": alert.title,
            "message": alert.message,
            "context": alert.context,
            "timestamp": alert.timestamp.to_rfc3339(),
            "service": self.config.service_name
        });

        let response = self
            .client
            .post(webhook_url)
            .json(&payload)
            .send()
            .await?;

        if !response.status().is_success() {
            warn!(
                status = response.status().as_u16(),
                "Failed to send alert webhook"
            );
        }

        Ok(())
    }

    /// Send error alert
    pub async fn alert_error(&self, title: &str, error: &str, context: serde_json::Value) {
        let alert = Alert {
            severity: AlertSeverity::Error,
            title: title.to_string(),
            message: error.to_string(),
            context,
            timestamp: chrono::Utc::now(),
        };

        if let Err(e) = self.send_alert(alert).await {
            error!(error = %e, "Failed to send error alert");
        }
    }

    /// Send critical alert
    pub async fn alert_critical(&self, title: &str, error: &str, context: serde_json::Value) {
        let alert = Alert {
            severity: AlertSeverity::Critical,
            title: title.to_string(),
            message: error.to_string(),
            context,
            timestamp: chrono::Utc::now(),
        };

        if let Err(e) = self.send_alert(alert).await {
            error!(error = %e, "Failed to send critical alert");
        }
    }

    /// Send performance degradation alert
    pub async fn alert_performance(&self, metric: &str, value: f64, threshold: f64) {
        let alert = Alert {
            severity: AlertSeverity::Warning,
            title: "Performance Degradation".to_string(),
            message: format!("{} exceeded threshold", metric),
            context: json!({
                "metric": metric,
                "value": value,
                "threshold": threshold
            }),
            timestamp: chrono::Utc::now(),
        };

        if let Err(e) = self.send_alert(alert).await {
            error!(error = %e, "Failed to send performance alert");
        }
    }

    /// Send billing alert
    pub async fn alert_billing(&self, tenant_id: &str, issue: &str, context: serde_json::Value) {
        let alert = Alert {
            severity: AlertSeverity::Warning,
            title: "Billing Issue".to_string(),
            message: format!("Billing issue for tenant {}: {}", tenant_id, issue),
            context,
            timestamp: chrono::Utc::now(),
        };

        if let Err(e) = self.send_alert(alert).await {
            error!(error = %e, "Failed to send billing alert");
        }
    }

    /// Get total alert count (for testing)
    pub async fn get_alert_count(&self) -> u64 {
        *self.alert_count.read().await
    }
}