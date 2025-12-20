use anyhow::Result;
use sqlx::{PgPool, Row};
use std::time::Duration;
use tracing::{info, warn};

use crate::models::*;

/// Database connection pool and operations
#[derive(Debug, Clone)]
pub struct Database {
    pool: PgPool,
}

impl Database {
    /// Create a new database connection with the given URL
    pub async fn new(database_url: &str) -> Result<Self> {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(20)
            .min_connections(5)
            .acquire_timeout(Duration::from_secs(30))
            .idle_timeout(Duration::from_secs(600))
            .max_lifetime(Duration::from_secs(1800))
            .connect(database_url)
            .await?;

        info!("Database connection pool established");
        Ok(Self { pool })
    }

    /// Get a reference to the connection pool
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Run database migrations
    pub async fn migrate(&self) -> Result<()> {
        sqlx::migrate!("./migrations").run(&self.pool).await?;
        info!("Database migrations completed");
        Ok(())
    }

    /// Validate tenant isolation by checking if tenant_id exists in query
    pub fn validate_tenant_isolation(tenant_id: &str, query: &str) -> bool {
        // Simple validation that tenant_id is included in WHERE clause
        // In production, this would be more sophisticated
        query.contains(&format!("tenant_id = '{}'", tenant_id))
            || query.contains("tenant_id = $")
    }

    // Tenant CRUD operations
    pub async fn create_tenant(&self, tenant: &Tenant) -> Result<()> {
        let status_str = match &tenant.status {
            TenantStatus::Active => "active",
            TenantStatus::Trial => "trial",
            TenantStatus::PastDue => "past_due",
            TenantStatus::Suspended => "suspended",
        };

        sqlx::query(
            r#"
            INSERT INTO tenants (id, name, plan, status, stripe_customer_id, created_at, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            "#,
        )
        .bind(&tenant.id)
        .bind(&tenant.name)
        .bind(serde_json::to_value(&tenant.plan)?)
        .bind(status_str)
        .bind(&tenant.stripe_customer_id)
        .bind(tenant.created_at)
        .bind(tenant.updated_at)
        .execute(&self.pool)
        .await?;

        info!("Created tenant: {}", tenant.id);
        Ok(())
    }

    pub async fn get_tenant(&self, tenant_id: &str) -> Result<Option<Tenant>> {
        let row = sqlx::query(
            "SELECT id, name, plan, status, stripe_customer_id, created_at, updated_at FROM tenants WHERE id = $1"
        )
        .bind(tenant_id)
        .fetch_optional(&self.pool)
        .await?;

        if let Some(row) = row {
            let plan: BillingPlan = serde_json::from_value(row.get("plan"))?;
            let status_str: String = row.get("status");
            let status = match status_str.as_str() {
                "active" => TenantStatus::Active,
                "trial" => TenantStatus::Trial,
                "past_due" => TenantStatus::PastDue,
                "suspended" => TenantStatus::Suspended,
                _ => TenantStatus::Trial,
            };

            Ok(Some(Tenant {
                id: row.get("id"),
                name: row.get("name"),
                plan,
                status,
                stripe_customer_id: row.get("stripe_customer_id"),
                created_at: row.get("created_at"),
                updated_at: row.get("updated_at"),
            }))
        } else {
            Ok(None)
        }
    }

    pub async fn update_tenant_status(&self, tenant_id: &str, status: TenantStatus) -> Result<()> {
        let status_str = match status {
            TenantStatus::Active => "active",
            TenantStatus::Trial => "trial",
            TenantStatus::PastDue => "past_due",
            TenantStatus::Suspended => "suspended",
        };

        let result =
            sqlx::query("UPDATE tenants SET status = $1, updated_at = NOW() WHERE id = $2")
                .bind(status_str)
                .bind(tenant_id)
                .execute(&self.pool)
                .await?;

        let rows_affected = result.rows_affected();

        if rows_affected == 0 {
            warn!("No tenant found with id: {}", tenant_id);
        } else {
            info!("Updated tenant {} status to {:?}", tenant_id, status);
        }

        Ok(())
    }

    // Project CRUD operations
    pub async fn create_project(&self, project: &Project) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO projects (id, tenant_id, name, limits, created_at, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6)
            "#,
        )
        .bind(&project.id)
        .bind(&project.tenant_id)
        .bind(&project.name)
        .bind(serde_json::to_value(&project.limits)?)
        .bind(project.created_at)
        .bind(project.updated_at)
        .execute(&self.pool)
        .await?;

        info!(
            "Created project: {} for tenant: {}",
            project.id, project.tenant_id
        );
        Ok(())
    }

    pub async fn get_project(&self, project_id: &str) -> Result<Option<Project>> {
        let row = sqlx::query(
            "SELECT id, tenant_id, name, limits, created_at, updated_at FROM projects WHERE id = $1"
        )
        .bind(project_id)
        .fetch_optional(&self.pool)
        .await?;

        if let Some(row) = row {
            let limits: ProjectLimits = serde_json::from_value(row.get("limits"))?;
            Ok(Some(Project {
                id: row.get("id"),
                tenant_id: row.get("tenant_id"),
                name: row.get("name"),
                limits,
                created_at: row.get("created_at"),
                updated_at: row.get("updated_at"),
            }))
        } else {
            Ok(None)
        }
    }

    pub async fn get_project_with_tenant(
        &self,
        tenant_id: &str,
        project_id: &str,
    ) -> Result<Option<Project>> {
        let row = sqlx::query(
            "SELECT id, tenant_id, name, limits, created_at, updated_at FROM projects WHERE id = $1 AND tenant_id = $2"
        )
        .bind(project_id)
        .bind(tenant_id)
        .fetch_optional(&self.pool)
        .await?;

        if let Some(row) = row {
            let limits: ProjectLimits = serde_json::from_value(row.get("limits"))?;
            Ok(Some(Project {
                id: row.get("id"),
                tenant_id: row.get("tenant_id"),
                name: row.get("name"),
                limits,
                created_at: row.get("created_at"),
                updated_at: row.get("updated_at"),
            }))
        } else {
            Ok(None)
        }
    }

    pub async fn get_projects_for_tenant(&self, tenant_id: &str) -> Result<Vec<Project>> {
        self.list_projects_for_tenant(tenant_id).await
    }

    pub async fn list_projects_for_tenant(&self, tenant_id: &str) -> Result<Vec<Project>> {
        let rows = sqlx::query(
            "SELECT id, tenant_id, name, limits, created_at, updated_at FROM projects WHERE tenant_id = $1 ORDER BY created_at"
        )
        .bind(tenant_id)
        .fetch_all(&self.pool)
        .await?;

        let mut projects = Vec::new();
        for row in rows {
            let limits: ProjectLimits = serde_json::from_value(row.get("limits"))?;
            projects.push(Project {
                id: row.get("id"),
                tenant_id: row.get("tenant_id"),
                name: row.get("name"),
                limits,
                created_at: row.get("created_at"),
                updated_at: row.get("updated_at"),
            });
        }

        Ok(projects)
    }

    // API Key CRUD operations
    pub async fn create_api_key(&self, api_key: &ApiKey) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO api_keys (id, tenant_id, project_id, key_hash, scopes, rate_limit_per_sec, is_active, expires_at, created_at, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            "#
        )
        .bind(&api_key.id)
        .bind(&api_key.tenant_id)
        .bind(&api_key.project_id)
        .bind(&api_key.key_hash)
        .bind(serde_json::to_value(&api_key.scopes)?)
        .bind(api_key.rate_limit_per_sec)
        .bind(api_key.is_active)
        .bind(api_key.expires_at)
        .bind(api_key.created_at)
        .bind(api_key.updated_at)
        .execute(&self.pool)
        .await?;

        info!(
            "Created API key: {} for tenant: {}",
            api_key.id, api_key.tenant_id
        );
        Ok(())
    }

    pub async fn get_api_key_by_hash(&self, key_hash: &str) -> Result<Option<ApiKey>> {
        let row = sqlx::query(
            r#"
            SELECT id, tenant_id, project_id, key_hash, scopes, rate_limit_per_sec, is_active, expires_at, created_at, updated_at
            FROM api_keys 
            WHERE key_hash = $1 AND is_active = true
            "#
        )
        .bind(key_hash)
        .fetch_optional(&self.pool)
        .await?;

        if let Some(row) = row {
            let scopes: Vec<Scope> = serde_json::from_value(row.get("scopes"))?;
            Ok(Some(ApiKey {
                id: row.get("id"),
                tenant_id: row.get("tenant_id"),
                project_id: row.get("project_id"),
                key_hash: row.get("key_hash"),
                scopes,
                rate_limit_per_sec: row.get("rate_limit_per_sec"),
                is_active: row.get("is_active"),
                expires_at: row.get("expires_at"),
                created_at: row.get("created_at"),
                updated_at: row.get("updated_at"),
            }))
        } else {
            Ok(None)
        }
    }

    pub async fn revoke_api_key(&self, tenant_id: &str, key_id: &str) -> Result<()> {
        let result = sqlx::query(
            "UPDATE api_keys SET is_active = false, updated_at = NOW() WHERE id = $1 AND tenant_id = $2"
        )
        .bind(key_id)
        .bind(tenant_id)
        .execute(&self.pool)
        .await?;

        let rows_affected = result.rows_affected();

        if rows_affected == 0 {
            warn!(
                "No API key found with id: {} for tenant: {}",
                key_id, tenant_id
            );
        } else {
            info!("Revoked API key: {} for tenant: {}", key_id, tenant_id);
        }

        Ok(())
    }

    // Event operations
    pub async fn create_event(&self, event: &Event) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO events (id, tenant_id, project_id, topic, payload, published_at)
            VALUES ($1, $2, $3, $4, $5, $6)
            "#,
        )
        .bind(&event.id)
        .bind(&event.tenant_id)
        .bind(&event.project_id)
        .bind(&event.topic)
        .bind(&event.payload)
        .bind(event.published_at)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn get_events_for_tenant(&self, tenant_id: &str, limit: i64) -> Result<Vec<Event>> {
        let rows = sqlx::query(
            "SELECT id, tenant_id, project_id, topic, payload, published_at FROM events WHERE tenant_id = $1 ORDER BY published_at DESC LIMIT $2"
        )
        .bind(tenant_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        let mut events = Vec::new();
        for row in rows {
            events.push(Event {
                id: row.get("id"),
                tenant_id: row.get("tenant_id"),
                project_id: row.get("project_id"),
                topic: row.get("topic"),
                payload: row.get("payload"),
                published_at: row.get("published_at"),
            });
        }

        Ok(events)
    }

    pub async fn get_api_keys_for_project(&self, project_id: &str) -> Result<Vec<ApiKey>> {
        let rows = sqlx::query(
            r#"
            SELECT id, tenant_id, project_id, key_hash, scopes, rate_limit_per_sec, is_active, expires_at, created_at, updated_at
            FROM api_keys 
            WHERE project_id = $1
            ORDER BY created_at DESC
            "#
        )
        .bind(project_id)
        .fetch_all(&self.pool)
        .await?;

        let mut api_keys = Vec::new();
        for row in rows {
            let scopes: Vec<Scope> = serde_json::from_value(row.get("scopes"))?;
            api_keys.push(ApiKey {
                id: row.get("id"),
                tenant_id: row.get("tenant_id"),
                project_id: row.get("project_id"),
                key_hash: row.get("key_hash"),
                scopes,
                rate_limit_per_sec: row.get("rate_limit_per_sec"),
                is_active: row.get("is_active"),
                expires_at: row.get("expires_at"),
                created_at: row.get("created_at"),
                updated_at: row.get("updated_at"),
            });
        }

        Ok(api_keys)
    }

    pub async fn get_usage_records(
        &self,
        project_id: &str,
        from_date: Option<chrono::DateTime<chrono::Utc>>,
        to_date: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Result<Vec<UsageRecord>> {
        let mut query = String::from(
            "SELECT id, tenant_id, project_id, metric, quantity, window_start, created_at FROM usage_records WHERE project_id = $1"
        );

        let mut bind_count = 2;
        if from_date.is_some() {
            query.push_str(&format!(" AND window_start >= ${}", bind_count));
            bind_count += 1;
        }
        if to_date.is_some() {
            query.push_str(&format!(" AND window_start <= ${}", bind_count));
        }
        query.push_str(" ORDER BY window_start DESC");

        let mut query_builder = sqlx::query(&query).bind(project_id);

        if let Some(from) = from_date {
            query_builder = query_builder.bind(from);
        }
        if let Some(to) = to_date {
            query_builder = query_builder.bind(to);
        }

        let rows = query_builder.fetch_all(&self.pool).await?;

        let mut usage_records = Vec::new();
        for row in rows {
            let metric_str: String = row.get("metric");
            let metric = match metric_str.as_str() {
                "events_published" => UsageMetric::EventsPublished,
                "events_delivered" => UsageMetric::EventsDelivered,
                "web_socket_minutes" => UsageMetric::WebSocketMinutes,
                "api_requests" => UsageMetric::ApiRequests,
                _ => UsageMetric::ApiRequests,
            };

            usage_records.push(UsageRecord {
                id: row.get("id"),
                tenant_id: row.get("tenant_id"),
                project_id: row.get("project_id"),
                metric,
                quantity: row.get("quantity"),
                window_start: row.get("window_start"),
                created_at: row.get("created_at"),
            });
        }

        Ok(usage_records)
    }

    // Usage tracking operations
    pub async fn create_usage_record(&self, usage: &UsageRecord) -> Result<()> {
        let metric_str = match &usage.metric {
            UsageMetric::EventsPublished => "events_published",
            UsageMetric::EventsDelivered => "events_delivered",
            UsageMetric::WebSocketMinutes => "web_socket_minutes",
            UsageMetric::ApiRequests => "api_requests",
        };

        sqlx::query(
            r#"
            INSERT INTO usage_records (id, tenant_id, project_id, metric, quantity, window_start, created_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            "#
        )
        .bind(&usage.id)
        .bind(&usage.tenant_id)
        .bind(&usage.project_id)
        .bind(metric_str)
        .bind(usage.quantity)
        .bind(usage.window_start)
        .bind(usage.created_at)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn get_usage_for_tenant(&self, tenant_id: &str, metric: UsageMetric) -> Result<i64> {
        let metric_str = match metric {
            UsageMetric::EventsPublished => "events_published",
            UsageMetric::EventsDelivered => "events_delivered",
            UsageMetric::WebSocketMinutes => "web_socket_minutes",
            UsageMetric::ApiRequests => "api_requests",
        };

        let row = sqlx::query(
            "SELECT COALESCE(SUM(quantity), 0) as total FROM usage_records WHERE tenant_id = $1 AND metric = $2"
        )
        .bind(tenant_id)
        .bind(metric_str)
        .fetch_one(&self.pool)
        .await?;

        let total: i64 = row.get("total");
        Ok(total)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_tenant_isolation() {
        let tenant_id = "tenant_123";

        // Valid queries with tenant isolation
        assert!(Database::validate_tenant_isolation(
            tenant_id,
            "SELECT * FROM events WHERE tenant_id = 'tenant_123'"
        ));

        assert!(Database::validate_tenant_isolation(
            tenant_id,
            "SELECT * FROM events WHERE tenant_id = $1"
        ));

        // Invalid query without tenant isolation
        assert!(!Database::validate_tenant_isolation(
            tenant_id,
            "SELECT * FROM events"
        ));
    }
}
