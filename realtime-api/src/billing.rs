use crate::models::{BillingPlan, Tenant, TenantStatus, UsageMetric};
use anyhow::{anyhow, Result};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{error, info, warn};

/// Stripe integration for metered billing
#[derive(Clone)]
pub struct BillingService {
    db: PgPool,
    stripe_api_key: String,
    usage_cache: Arc<RwLock<HashMap<String, UsageCache>>>,
    http_client: reqwest::Client,
}

/// Cached usage data for a tenant
#[derive(Debug, Clone)]
struct UsageCache {
    events_published: i64,
    events_delivered: i64,
    websocket_minutes: i64,
    api_requests: i64,
    window_start: DateTime<Utc>,
    last_reported: DateTime<Utc>,
}

/// Usage limits for a billing plan
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageLimits {
    pub max_events_per_month: Option<i64>,
    pub max_connections: Option<i32>,
    pub max_api_requests_per_day: Option<i64>,
}

/// Stripe metered usage report
#[derive(Debug, Serialize)]
struct StripeUsageReport {
    quantity: i64,
    timestamp: i64,
    action: String,
}

impl BillingService {
    /// Create a new billing service
    pub fn new(db: PgPool, stripe_api_key: String) -> Self {
        Self {
            db,
            stripe_api_key,
            usage_cache: Arc::new(RwLock::new(HashMap::new())),
            http_client: reqwest::Client::new(),
        }
    }

    /// Track usage metric for a tenant
    pub async fn track_usage(
        &self,
        tenant_id: &str,
        project_id: &str,
        metric: UsageMetric,
        quantity: i64,
    ) -> Result<()> {
        // Update cache
        let mut cache = self.usage_cache.write().await;
        let entry = cache.entry(tenant_id.to_string()).or_insert_with(|| {
            let now = Utc::now();
            UsageCache {
                events_published: 0,
                events_delivered: 0,
                websocket_minutes: 0,
                api_requests: 0,
                window_start: now,
                last_reported: now,
            }
        });

        match metric {
            UsageMetric::EventsPublished => entry.events_published += quantity,
            UsageMetric::EventsDelivered => entry.events_delivered += quantity,
            UsageMetric::WebSocketMinutes => entry.websocket_minutes += quantity,
            UsageMetric::ApiRequests => entry.api_requests += quantity,
        }

        // Persist to database
        let window_start = entry.window_start;
        drop(cache);

        sqlx::query(
            r#"
            INSERT INTO usage_records (id, tenant_id, project_id, metric, quantity, window_start, created_at)
            VALUES (gen_random_uuid()::text, $1, $2, $3, $4, $5, NOW())
            ON CONFLICT (tenant_id, project_id, metric, window_start)
            DO UPDATE SET quantity = usage_records.quantity + EXCLUDED.quantity
            "#,
        )
        .bind(tenant_id)
        .bind(project_id)
        .bind(&metric)
        .bind(quantity)
        .bind(window_start)
        .execute(&self.db)
        .await?;

        Ok(())
    }

    /// Get current usage for a tenant
    pub async fn get_usage(&self, tenant_id: &str) -> Result<UsageCache> {
        let cache = self.usage_cache.read().await;
        cache
            .get(tenant_id)
            .cloned()
            .ok_or_else(|| anyhow!("No usage data found for tenant"))
    }

    /// Check if tenant has exceeded usage limits
    pub async fn check_limits(&self, tenant_id: &str) -> Result<bool> {
        let tenant = self.get_tenant(tenant_id).await?;
        let usage = self.get_usage(tenant_id).await?;

        let limits = self.get_limits_for_plan(&tenant.plan);

        // Check event limits
        if let Some(max_events) = limits.max_events_per_month {
            if usage.events_published >= max_events {
                warn!(
                    tenant_id = tenant_id,
                    usage = usage.events_published,
                    limit = max_events,
                    "Tenant exceeded event limit"
                );
                return Ok(false);
            }
        }

        Ok(true)
    }

    /// Enforce hard limits by suspending tenant
    pub async fn enforce_hard_limit(&self, tenant_id: &str) -> Result<()> {
        info!(tenant_id = tenant_id, "Enforcing hard limit on tenant");

        sqlx::query(
            r#"
            UPDATE tenants
            SET status = 'suspended', updated_at = NOW()
            WHERE id = $1
            "#,
        )
        .bind(tenant_id)
        .execute(&self.db)
        .await?;

        Ok(())
    }

    /// Report usage to Stripe for metered billing
    pub async fn report_to_stripe(&self, tenant_id: &str) -> Result<()> {
        let tenant = self.get_tenant(tenant_id).await?;
        let usage = self.get_usage(tenant_id).await?;

        if let Some(stripe_customer_id) = &tenant.stripe_customer_id {
            let report = StripeUsageReport {
                quantity: usage.events_published,
                timestamp: Utc::now().timestamp(),
                action: "increment".to_string(),
            };

            // Send to Stripe API
            let response = self
                .http_client
                .post(format!(
                    "https://api.stripe.com/v1/subscription_items/{}/usage_records",
                    stripe_customer_id
                ))
                .bearer_auth(&self.stripe_api_key)
                .json(&report)
                .send()
                .await?;

            if !response.status().is_success() {
                error!(
                    tenant_id = tenant_id,
                    status = ?response.status(),
                    "Failed to report usage to Stripe"
                );
                return Err(anyhow!("Failed to report usage to Stripe"));
            }

            info!(
                tenant_id = tenant_id,
                quantity = usage.events_published,
                "Reported usage to Stripe"
            );

            // Update last reported timestamp
            let mut cache = self.usage_cache.write().await;
            if let Some(entry) = cache.get_mut(tenant_id) {
                entry.last_reported = Utc::now();
            }
        }

        Ok(())
    }

    /// Activate kill switch for non-payment
    pub async fn activate_kill_switch(&self, tenant_id: &str, reason: &str) -> Result<()> {
        info!(
            tenant_id = tenant_id,
            reason = reason,
            "Activating kill switch"
        );

        sqlx::query(
            r#"
            UPDATE tenants
            SET status = 'suspended', updated_at = NOW()
            WHERE id = $1
            "#,
        )
        .bind(tenant_id)
        .execute(&self.db)
        .await?;

        // Log the kill switch activation
        sqlx::query(
            r#"
            INSERT INTO audit_logs (id, tenant_id, action, details, created_at)
            VALUES (gen_random_uuid()::text, $1, 'kill_switch_activated', $2, NOW())
            "#,
        )
        .bind(tenant_id)
        .bind(serde_json::json!({ "reason": reason }))
        .execute(&self.db)
        .await?;

        Ok(())
    }

    /// Handle free trial expiration
    pub async fn handle_trial_expiration(&self, tenant_id: &str) -> Result<()> {
        let tenant = self.get_tenant(tenant_id).await?;

        if tenant.status != TenantStatus::Trial {
            return Ok(());
        }

        // Check if trial has expired (14 days from creation)
        let trial_duration = Duration::days(14);
        let trial_end = tenant.created_at + trial_duration;

        if Utc::now() > trial_end {
            info!(tenant_id = tenant_id, "Trial expired, converting to paid");

            // Check if Stripe customer exists
            if tenant.stripe_customer_id.is_some() {
                // Convert to active paid plan
                sqlx::query(
                    r#"
                    UPDATE tenants
                    SET status = 'active', updated_at = NOW()
                    WHERE id = $1
                    "#,
                )
                .bind(tenant_id)
                .execute(&self.db)
                .await?;
            } else {
                // No payment method, suspend
                self.activate_kill_switch(tenant_id, "Trial expired without payment method")
                    .await?;
            }
        }

        Ok(())
    }

    /// Get tenant from database
    async fn get_tenant(&self, tenant_id: &str) -> Result<Tenant> {
        let row = sqlx::query(
            r#"
            SELECT id, name, plan, status, stripe_customer_id, created_at, updated_at
            FROM tenants
            WHERE id = $1
            "#,
        )
        .bind(tenant_id)
        .fetch_one(&self.db)
        .await
        .map_err(|e| anyhow!("Failed to fetch tenant: {}", e))?;

        let tenant = Tenant {
            id: row.get("id"),
            name: row.get("name"),
            plan: serde_json::from_value(row.get("plan"))
                .map_err(|e| anyhow!("Failed to deserialize plan: {}", e))?,
            status: row.get("status"),
            stripe_customer_id: row.get("stripe_customer_id"),
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
        };

        Ok(tenant)
    }

    /// Get usage limits for a billing plan
    fn get_limits_for_plan(&self, plan: &BillingPlan) -> UsageLimits {
        match plan {
            BillingPlan::Free { monthly_events } => UsageLimits {
                max_events_per_month: Some(*monthly_events),
                max_connections: Some(100),
                max_api_requests_per_day: Some(10000),
            },
            BillingPlan::Pro { monthly_events, .. } => UsageLimits {
                max_events_per_month: Some(*monthly_events),
                max_connections: Some(1000),
                max_api_requests_per_day: Some(100000),
            },
            BillingPlan::Enterprise { .. } => UsageLimits {
                max_events_per_month: None,
                max_connections: None,
                max_api_requests_per_day: None,
            },
        }
    }

    /// Reset usage cache for a new billing period
    pub async fn reset_usage_cache(&self, tenant_id: &str) -> Result<()> {
        let mut cache = self.usage_cache.write().await;
        if let Some(entry) = cache.get_mut(tenant_id) {
            entry.events_published = 0;
            entry.events_delivered = 0;
            entry.websocket_minutes = 0;
            entry.api_requests = 0;
            entry.window_start = Utc::now();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_usage_limits_free_plan() {
        let service = BillingService::new(
            PgPool::connect_lazy("postgresql://localhost/test").unwrap(),
            "test_key".to_string(),
        );

        let plan = BillingPlan::Free {
            monthly_events: 10000,
        };
        let limits = service.get_limits_for_plan(&plan);

        assert_eq!(limits.max_events_per_month, Some(10000));
        assert_eq!(limits.max_connections, Some(100));
    }

    #[test]
    fn test_usage_limits_enterprise_plan() {
        let service = BillingService::new(
            PgPool::connect_lazy("postgresql://localhost/test").unwrap(),
            "test_key".to_string(),
        );

        let plan = BillingPlan::Enterprise { unlimited: true };
        let limits = service.get_limits_for_plan(&plan);

        assert_eq!(limits.max_events_per_month, None);
        assert_eq!(limits.max_connections, None);
    }
}
