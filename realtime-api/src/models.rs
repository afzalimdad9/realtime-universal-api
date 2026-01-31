use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

/// Tenant represents an organization or customer account with isolated resources
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Tenant {
    pub id: String,
    pub name: String,
    pub plan: BillingPlan,
    pub status: TenantStatus,
    pub stripe_customer_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Project represents a subdivision within a tenant for organizing applications
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Project {
    pub id: String,
    pub tenant_id: String,
    pub name: String,
    pub limits: ProjectLimits,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// API key for authentication with specific scopes and rate limits
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ApiKey {
    pub id: String,
    pub tenant_id: String,
    pub project_id: String,
    pub key_hash: String,
    pub scopes: Vec<Scope>,
    pub rate_limit_per_sec: i32,
    pub is_active: bool,
    pub expires_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Event represents a message published to a specific topic
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Event {
    pub id: String,
    pub tenant_id: String,
    pub project_id: String,
    pub topic: String,
    pub payload: serde_json::Value,
    pub published_at: DateTime<Utc>,
}

/// Usage record for tracking resource consumption
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct UsageRecord {
    pub id: String,
    pub tenant_id: String,
    pub project_id: String,
    pub metric: UsageMetric,
    pub quantity: i64,
    pub window_start: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

/// Tenant status enumeration
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "tenant_status", rename_all = "lowercase")]
pub enum TenantStatus {
    Active,
    Trial,
    PastDue,
    Suspended,
}

/// API key scope enumeration
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "scope", rename_all = "snake_case")]
pub enum Scope {
    EventsPublish,
    EventsSubscribe,
    AdminRead,
    AdminWrite,
    BillingRead,
}

/// Usage metric enumeration
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "usage_metric", rename_all = "snake_case")]
pub enum UsageMetric {
    EventsPublished,
    EventsDelivered,
    WebSocketMinutes,
    ApiRequests,
}

/// User role enumeration for RBAC
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "user_role", rename_all = "snake_case")]
pub enum UserRole {
    Owner,
    Admin,
    Developer,
    Viewer,
}

/// Permission enumeration for RBAC
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "permission", rename_all = "snake_case")]
pub enum Permission {
    ManageTenant,
    ManageProjects,
    ManageApiKeys,
    ManageUsers,
    ViewAuditLogs,
    PublishEvents,
    SubscribeEvents,
    ViewBilling,
    ManageBilling,
}

/// User represents a user within a tenant with specific roles
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct User {
    pub id: String,
    pub tenant_id: String,
    pub email: String,
    pub name: String,
    pub role: UserRole,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Role permission mapping
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct RolePermission {
    pub id: String,
    pub role: UserRole,
    pub permission: Permission,
    pub created_at: DateTime<Utc>,
}

/// Billing plan configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BillingPlan {
    Free {
        monthly_events: i64,
    },
    Pro {
        monthly_events: i64,
        price_per_event: f64,
    },
    Enterprise {
        unlimited: bool,
    },
}

/// Project limits configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectLimits {
    pub max_connections: i32,
    pub max_events_per_sec: i32,
    pub max_payload_size: i32,
}

impl Default for ProjectLimits {
    fn default() -> Self {
        Self {
            max_connections: 1000,
            max_events_per_sec: 100,
            max_payload_size: 1024 * 1024, // 1MB
        }
    }
}

impl Tenant {
    /// Create a new tenant with default values
    pub fn new(name: String, plan: BillingPlan) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4().to_string(),
            name,
            plan,
            status: TenantStatus::Trial,
            stripe_customer_id: None,
            created_at: now,
            updated_at: now,
        }
    }

    /// Check if tenant is active and can perform operations
    pub fn is_active(&self) -> bool {
        matches!(self.status, TenantStatus::Active | TenantStatus::Trial)
    }
}

impl Project {
    /// Create a new project with default limits
    pub fn new(tenant_id: String, name: String) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4().to_string(),
            tenant_id,
            name,
            limits: ProjectLimits::default(),
            created_at: now,
            updated_at: now,
        }
    }
}

impl ApiKey {
    /// Create a new API key with specified scopes
    pub fn new(
        tenant_id: String,
        project_id: String,
        key_hash: String,
        scopes: Vec<Scope>,
        rate_limit_per_sec: i32,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4().to_string(),
            tenant_id,
            project_id,
            key_hash,
            scopes,
            rate_limit_per_sec,
            is_active: true,
            expires_at: None,
            created_at: now,
            updated_at: now,
        }
    }

    /// Check if API key has a specific scope
    pub fn has_scope(&self, scope: &Scope) -> bool {
        self.scopes.contains(scope)
    }

    /// Check if API key is valid (active and not expired)
    pub fn is_valid(&self) -> bool {
        self.is_active && self.expires_at.is_none_or(|exp| exp > Utc::now())
    }
}

impl Event {
    /// Create a new event
    pub fn new(
        tenant_id: String,
        project_id: String,
        topic: String,
        payload: serde_json::Value,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            tenant_id,
            project_id,
            topic,
            payload,
            published_at: Utc::now(),
        }
    }
}

impl UsageRecord {
    /// Create a new usage record
    pub fn new(
        tenant_id: String,
        project_id: String,
        metric: UsageMetric,
        quantity: i64,
        window_start: DateTime<Utc>,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            tenant_id,
            project_id,
            metric,
            quantity,
            window_start,
            created_at: Utc::now(),
        }
    }
}

impl User {
    /// Create a new user
    pub fn new(tenant_id: String, email: String, name: String, role: UserRole) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4().to_string(),
            tenant_id,
            email,
            name,
            role,
            is_active: true,
            created_at: now,
            updated_at: now,
        }
    }

    /// Check if user has a specific permission based on their role
    pub fn has_permission(&self, permission: &Permission) -> bool {
        if !self.is_active {
            return false;
        }

        match (&self.role, permission) {
            // Owner has all permissions
            (UserRole::Owner, _) => true,
            
            // Admin permissions
            (UserRole::Admin, Permission::ManageProjects) => true,
            (UserRole::Admin, Permission::ManageApiKeys) => true,
            (UserRole::Admin, Permission::ManageUsers) => true,
            (UserRole::Admin, Permission::ViewAuditLogs) => true,
            (UserRole::Admin, Permission::PublishEvents) => true,
            (UserRole::Admin, Permission::SubscribeEvents) => true,
            (UserRole::Admin, Permission::ViewBilling) => true,
            
            // Developer permissions
            (UserRole::Developer, Permission::ManageApiKeys) => true,
            (UserRole::Developer, Permission::PublishEvents) => true,
            (UserRole::Developer, Permission::SubscribeEvents) => true,
            (UserRole::Developer, Permission::ViewBilling) => true,
            
            // Viewer permissions
            (UserRole::Viewer, Permission::SubscribeEvents) => true,
            (UserRole::Viewer, Permission::ViewBilling) => true,
            
            // Default deny
            _ => false,
        }
    }
}

impl RolePermission {
    /// Create a new role permission mapping
    pub fn new(role: UserRole, permission: Permission) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            role,
            permission,
            created_at: Utc::now(),
        }
    }
}
/// Audit log for tracking administrative operations
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct AuditLog {
    pub id: String,
    pub tenant_id: String,
    pub operation: String,
    pub details: String,
    pub performed_by: String,
    pub created_at: DateTime<Utc>,
}

impl AuditLog {
    /// Create a new audit log entry
    pub fn new(
        tenant_id: String,
        operation: String,
        details: String,
        performed_by: String,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            tenant_id,
            operation,
            details,
            performed_by,
            created_at: Utc::now(),
        }
    }
}