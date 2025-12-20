use async_graphql::{
    Context, Enum, Error, ErrorExtensions, FieldResult, InputObject, Object, Schema, SimpleObject,
    Subscription, Union, ID,
};
use async_graphql_axum::{GraphQLRequest, GraphQLResponse};
use axum::extract::ws::WebSocketUpgrade;
use chrono::{DateTime, Utc};
use futures_util::Stream;
use std::fmt;
use std::pin::Pin;
use tokio_stream::{wrappers::BroadcastStream, StreamExt as TokioStreamExt};
use tracing::{error, info};

use crate::auth::{AuthContext, AuthError, AuthService};
use crate::database::Database;
use crate::event_service::{EventService, PublishResult};
use crate::models::{
    ApiKey, BillingPlan, Event, Project, ProjectLimits, Scope, Tenant, TenantStatus, UsageMetric,
    UsageRecord,
};

/// GraphQL Schema type
pub type ApiSchema = Schema<QueryRoot, MutationRoot, SubscriptionRoot>;

/// GraphQL Error extensions for better error handling
#[derive(Debug)]
pub enum GraphQLError {
    Unauthorized,
    Forbidden,
    NotFound,
    ValidationError(String),
    InternalError(String),
}

impl fmt::Display for GraphQLError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GraphQLError::Unauthorized => write!(f, "Unauthorized"),
            GraphQLError::Forbidden => write!(f, "Forbidden"),
            GraphQLError::NotFound => write!(f, "Not found"),
            GraphQLError::ValidationError(msg) => write!(f, "Validation error: {}", msg),
            GraphQLError::InternalError(msg) => write!(f, "Internal error: {}", msg),
        }
    }
}

impl ErrorExtensions for GraphQLError {
    fn extend(&self) -> Error {
        match self {
            GraphQLError::Unauthorized => Error::new("Unauthorized").extend_with(|_, e| {
                e.set("code", "UNAUTHORIZED");
            }),
            GraphQLError::Forbidden => Error::new("Forbidden").extend_with(|_, e| {
                e.set("code", "FORBIDDEN");
            }),
            GraphQLError::NotFound => Error::new("Not found").extend_with(|_, e| {
                e.set("code", "NOT_FOUND");
            }),
            GraphQLError::ValidationError(msg) => {
                Error::new(format!("Validation error: {}", msg)).extend_with(|_, e| {
                    e.set("code", "VALIDATION_ERROR");
                })
            }
            GraphQLError::InternalError(msg) => {
                Error::new(format!("Internal error: {}", msg)).extend_with(|_, e| {
                    e.set("code", "INTERNAL_ERROR");
                })
            }
        }
    }
}

impl From<AuthError> for GraphQLError {
    fn from(err: AuthError) -> Self {
        match err {
            AuthError::InvalidApiKey | AuthError::ExpiredApiKey | AuthError::InvalidJwt => {
                GraphQLError::Unauthorized
            }
            AuthError::InsufficientScope { .. } | AuthError::TenantSuspended => {
                GraphQLError::Forbidden
            }
            AuthError::RateLimitExceeded => GraphQLError::Forbidden,
            _ => GraphQLError::InternalError(err.to_string()),
        }
    }
}

impl From<anyhow::Error> for GraphQLError {
    fn from(err: anyhow::Error) -> Self {
        GraphQLError::InternalError(err.to_string())
    }
}

/// GraphQL representation of Tenant
#[derive(SimpleObject, Clone)]
pub struct GqlTenant {
    pub id: ID,
    pub name: String,
    pub plan: GqlBillingPlan,
    pub status: GqlTenantStatus,
    pub stripe_customer_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<Tenant> for GqlTenant {
    fn from(tenant: Tenant) -> Self {
        Self {
            id: ID(tenant.id),
            name: tenant.name,
            plan: tenant.plan.into(),
            status: tenant.status.into(),
            stripe_customer_id: tenant.stripe_customer_id,
            created_at: tenant.created_at,
            updated_at: tenant.updated_at,
        }
    }
}

/// GraphQL representation of Project
#[derive(SimpleObject, Clone)]
pub struct GqlProject {
    pub id: ID,
    pub tenant_id: ID,
    pub name: String,
    pub limits: GqlProjectLimits,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<Project> for GqlProject {
    fn from(project: Project) -> Self {
        Self {
            id: ID(project.id),
            tenant_id: ID(project.tenant_id),
            name: project.name,
            limits: project.limits.into(),
            created_at: project.created_at,
            updated_at: project.updated_at,
        }
    }
}

/// GraphQL representation of Event
#[derive(SimpleObject, Clone)]
pub struct GqlEvent {
    pub id: ID,
    pub tenant_id: ID,
    pub project_id: ID,
    pub topic: String,
    pub payload: String, // JSON as string for GraphQL
    pub published_at: DateTime<Utc>,
}

impl From<Event> for GqlEvent {
    fn from(event: Event) -> Self {
        Self {
            id: ID(event.id),
            tenant_id: ID(event.tenant_id),
            project_id: ID(event.project_id),
            topic: event.topic,
            payload: event.payload.to_string(),
            published_at: event.published_at,
        }
    }
}

/// GraphQL representation of API Key (without sensitive data)
#[derive(SimpleObject, Clone)]
pub struct GqlApiKey {
    pub id: ID,
    pub tenant_id: ID,
    pub project_id: ID,
    pub scopes: Vec<GqlScope>,
    pub rate_limit_per_sec: i32,
    pub is_active: bool,
    pub expires_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<ApiKey> for GqlApiKey {
    fn from(api_key: ApiKey) -> Self {
        Self {
            id: ID(api_key.id),
            tenant_id: ID(api_key.tenant_id),
            project_id: ID(api_key.project_id),
            scopes: api_key.scopes.into_iter().map(Into::into).collect(),
            rate_limit_per_sec: api_key.rate_limit_per_sec,
            is_active: api_key.is_active,
            expires_at: api_key.expires_at,
            created_at: api_key.created_at,
            updated_at: api_key.updated_at,
        }
    }
}

/// GraphQL representation of Usage Record
#[derive(SimpleObject, Clone)]
pub struct GqlUsageRecord {
    pub id: ID,
    pub tenant_id: ID,
    pub project_id: ID,
    pub metric: GqlUsageMetric,
    pub quantity: i64,
    pub window_start: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

impl From<UsageRecord> for GqlUsageRecord {
    fn from(record: UsageRecord) -> Self {
        Self {
            id: ID(record.id),
            tenant_id: ID(record.tenant_id),
            project_id: ID(record.project_id),
            metric: record.metric.into(),
            quantity: record.quantity,
            window_start: record.window_start,
            created_at: record.created_at,
        }
    }
}

/// GraphQL enums
#[derive(Enum, Copy, Clone, Eq, PartialEq)]
#[derive(Debug)]
pub enum GqlTenantStatus {
    Active,
    Trial,
    PastDue,
    Suspended,
}

impl From<TenantStatus> for GqlTenantStatus {
    fn from(status: TenantStatus) -> Self {
        match status {
            TenantStatus::Active => GqlTenantStatus::Active,
            TenantStatus::Trial => GqlTenantStatus::Trial,
            TenantStatus::PastDue => GqlTenantStatus::PastDue,
            TenantStatus::Suspended => GqlTenantStatus::Suspended,
        }
    }
}

#[derive(Enum, Copy, Clone, Eq, PartialEq)]
pub enum GqlScope {
    EventsPublish,
    EventsSubscribe,
    AdminRead,
    AdminWrite,
    BillingRead,
}

impl From<Scope> for GqlScope {
    fn from(scope: Scope) -> Self {
        match scope {
            Scope::EventsPublish => GqlScope::EventsPublish,
            Scope::EventsSubscribe => GqlScope::EventsSubscribe,
            Scope::AdminRead => GqlScope::AdminRead,
            Scope::AdminWrite => GqlScope::AdminWrite,
            Scope::BillingRead => GqlScope::BillingRead,
        }
    }
}

impl From<GqlScope> for Scope {
    fn from(scope: GqlScope) -> Self {
        match scope {
            GqlScope::EventsPublish => Scope::EventsPublish,
            GqlScope::EventsSubscribe => Scope::EventsSubscribe,
            GqlScope::AdminRead => Scope::AdminRead,
            GqlScope::AdminWrite => Scope::AdminWrite,
            GqlScope::BillingRead => Scope::BillingRead,
        }
    }
}

#[derive(Enum, Copy, Clone, Eq, PartialEq)]
pub enum GqlUsageMetric {
    EventsPublished,
    EventsDelivered,
    WebSocketMinutes,
    ApiRequests,
}

impl From<UsageMetric> for GqlUsageMetric {
    fn from(metric: UsageMetric) -> Self {
        match metric {
            UsageMetric::EventsPublished => GqlUsageMetric::EventsPublished,
            UsageMetric::EventsDelivered => GqlUsageMetric::EventsDelivered,
            UsageMetric::WebSocketMinutes => GqlUsageMetric::WebSocketMinutes,
            UsageMetric::ApiRequests => GqlUsageMetric::ApiRequests,
        }
    }
}

/// GraphQL complex types
#[derive(SimpleObject, Clone)]
pub struct GqlProjectLimits {
    pub max_connections: i32,
    pub max_events_per_sec: i32,
    pub max_payload_size: i32,
}

impl From<ProjectLimits> for GqlProjectLimits {
    fn from(limits: ProjectLimits) -> Self {
        Self {
            max_connections: limits.max_connections,
            max_events_per_sec: limits.max_events_per_sec,
            max_payload_size: limits.max_payload_size,
        }
    }
}

#[derive(Union, Clone)]
pub enum GqlBillingPlan {
    Free(GqlFreePlan),
    Pro(GqlProPlan),
    Enterprise(GqlEnterprisePlan),
}

impl From<BillingPlan> for GqlBillingPlan {
    fn from(plan: BillingPlan) -> Self {
        match plan {
            BillingPlan::Free { monthly_events } => {
                GqlBillingPlan::Free(GqlFreePlan { monthly_events })
            }
            BillingPlan::Pro {
                monthly_events,
                price_per_event,
            } => GqlBillingPlan::Pro(GqlProPlan {
                monthly_events,
                price_per_event,
            }),
            BillingPlan::Enterprise { unlimited } => {
                GqlBillingPlan::Enterprise(GqlEnterprisePlan { unlimited })
            }
        }
    }
}

#[derive(SimpleObject, Clone)]
pub struct GqlFreePlan {
    pub monthly_events: i64,
}

#[derive(SimpleObject, Clone)]
pub struct GqlProPlan {
    pub monthly_events: i64,
    pub price_per_event: f64,
}

#[derive(SimpleObject, Clone)]
pub struct GqlEnterprisePlan {
    pub unlimited: bool,
}

/// Input types for mutations
#[derive(InputObject)]
pub struct EventInput {
    pub topic: String,
    pub payload: String, // JSON as string
}

#[derive(InputObject)]
pub struct CreateApiKeyInput {
    pub project_id: ID,
    pub scopes: Vec<GqlScope>,
    pub rate_limit_per_sec: i32,
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(InputObject)]
pub struct CreateTenantInput {
    pub name: String,
    pub plan: CreateBillingPlanInput,
}

#[derive(InputObject)]
pub struct CreateBillingPlanInput {
    pub plan_type: String, // "free", "pro", "enterprise"
    pub monthly_events: Option<i64>,
    pub price_per_event: Option<f64>,
    pub unlimited: Option<bool>,
}

#[derive(InputObject)]
pub struct CreateProjectInput {
    pub name: String,
    pub limits: Option<CreateProjectLimitsInput>,
}

#[derive(InputObject)]
pub struct CreateProjectLimitsInput {
    pub max_connections: i32,
    pub max_events_per_sec: i32,
    pub max_payload_size: i32,
}

/// Filter types for queries
#[derive(InputObject)]
pub struct EventFilter {
    pub topic: Option<String>,
    pub from_date: Option<DateTime<Utc>>,
    pub to_date: Option<DateTime<Utc>>,
    pub limit: Option<i32>,
}

/// Query root
pub struct QueryRoot;

#[Object]
impl QueryRoot {
    /// Get events with optional filtering and tenant isolation
    async fn events(
        &self,
        ctx: &Context<'_>,
        filter: Option<EventFilter>,
    ) -> FieldResult<Vec<GqlEvent>> {
        let auth = get_auth_context(ctx)?;
        let database = ctx.data::<Database>()?;

        // Enforce tenant isolation - only return events for the authenticated tenant
        let limit = filter.as_ref().and_then(|f| f.limit).unwrap_or(100) as i64;
        let events = database
            .get_events_for_tenant(&auth.tenant_id, limit)
            .await
            .map_err(GraphQLError::from)?;

        Ok(events.into_iter().map(Into::into).collect())
    }

    /// Get tenants (admin only)
    async fn tenants(&self, ctx: &Context<'_>) -> FieldResult<Vec<GqlTenant>> {
        let auth = get_auth_context(ctx)?;
        check_scope(&auth, &Scope::AdminRead)?;

        let database = ctx.data::<Database>()?;

        // Only return the authenticated tenant (tenant isolation)
        if let Some(tenant) = database.get_tenant(&auth.tenant_id).await.map_err(GraphQLError::from)? {
            Ok(vec![tenant.into()])
        } else {
            Ok(vec![])
        }
    }

    /// Get projects for a tenant
    async fn projects(&self, ctx: &Context<'_>, tenant_id: Option<ID>) -> FieldResult<Vec<GqlProject>> {
        let auth = get_auth_context(ctx)?;
        let database = ctx.data::<Database>()?;

        // Enforce tenant isolation - use authenticated tenant_id if not provided or if different
        let target_tenant_id = tenant_id
            .map(|id| id.to_string())
            .unwrap_or_else(|| auth.tenant_id.clone());

        if target_tenant_id != auth.tenant_id {
            return Err(GraphQLError::Forbidden.extend());
        }

        let projects = database
            .get_projects_for_tenant(&target_tenant_id)
            .await
            .map_err(GraphQLError::from)?;

        Ok(projects.into_iter().map(Into::into).collect())
    }

    /// Get API keys for a project (admin read required)
    async fn api_keys(&self, ctx: &Context<'_>, project_id: ID) -> FieldResult<Vec<GqlApiKey>> {
        let auth = get_auth_context(ctx)?;
        check_scope(&auth, &Scope::AdminRead)?;

        let database = ctx.data::<Database>()?;

        // Verify project belongs to authenticated tenant
        let project = database
            .get_project(&project_id.to_string())
            .await
            .map_err(GraphQLError::from)?
            .ok_or_else(|| GraphQLError::NotFound)?;

        if project.tenant_id != auth.tenant_id {
            return Err(GraphQLError::Forbidden.extend());
        }

        let api_keys = database
            .get_api_keys_for_project(&project_id.to_string())
            .await
            .map_err(GraphQLError::from)?;

        Ok(api_keys.into_iter().map(Into::into).collect())
    }

    /// Get usage records for a project (billing read required)
    async fn usage_records(
        &self,
        ctx: &Context<'_>,
        project_id: ID,
        from_date: Option<DateTime<Utc>>,
        to_date: Option<DateTime<Utc>>,
    ) -> FieldResult<Vec<GqlUsageRecord>> {
        let auth = get_auth_context(ctx)?;
        check_scope(&auth, &Scope::BillingRead)?;

        let database = ctx.data::<Database>()?;

        // Verify project belongs to authenticated tenant
        let project = database
            .get_project(&project_id.to_string())
            .await
            .map_err(GraphQLError::from)?
            .ok_or_else(|| GraphQLError::NotFound)?;

        if project.tenant_id != auth.tenant_id {
            return Err(GraphQLError::Forbidden.extend());
        }

        let usage_records = database
            .get_usage_records(&project_id.to_string(), from_date, to_date)
            .await
            .map_err(GraphQLError::from)?;

        Ok(usage_records.into_iter().map(Into::into).collect())
    }
}

/// Mutation root
pub struct MutationRoot;

#[Object]
impl MutationRoot {
    /// Publish an event
    async fn publish_event(
        &self,
        ctx: &Context<'_>,
        input: EventInput,
    ) -> FieldResult<GqlEvent> {
        let auth = get_auth_context(ctx)?;
        check_scope(&auth, &Scope::EventsPublish)?;

        let event_service = ctx.data::<EventService>()?;

        // Parse JSON payload
        let payload: serde_json::Value = serde_json::from_str(&input.payload)
            .map_err(|e| GraphQLError::ValidationError(format!("Invalid JSON payload: {}", e)))?;

        // Create and publish event
        let event = Event::new(
            auth.tenant_id.clone(),
            auth.project_id.clone(),
            input.topic,
            payload,
        );

        match event_service.publish_event(&event).await {
            Ok(PublishResult::Success) => {
                info!("Event published via GraphQL: {}", event.id);
                Ok(event.into())
            }
            Ok(PublishResult::ValidationFailed(msg)) => {
                Err(GraphQLError::ValidationError(msg).extend())
            }
            Err(e) => Err(GraphQLError::InternalError(e.to_string()).extend()),
        }
    }

    /// Create a new API key (admin write required)
    async fn create_api_key(
        &self,
        ctx: &Context<'_>,
        input: CreateApiKeyInput,
    ) -> FieldResult<GqlApiKey> {
        let auth = get_auth_context(ctx)?;
        check_scope(&auth, &Scope::AdminWrite)?;

        let auth_service = ctx.data::<AuthService>()?;
        let database = ctx.data::<Database>()?;

        // Verify project belongs to authenticated tenant
        let project = database
            .get_project(&input.project_id.to_string())
            .await
            .map_err(GraphQLError::from)?
            .ok_or_else(|| GraphQLError::NotFound)?;

        if project.tenant_id != auth.tenant_id {
            return Err(GraphQLError::Forbidden.extend());
        }

        let scopes: Vec<Scope> = input.scopes.into_iter().map(Into::into).collect();

        let (_, api_key) = auth_service
            .create_api_key(
                auth.tenant_id.clone(),
                input.project_id.to_string(),
                scopes,
                input.rate_limit_per_sec,
                input.expires_at,
            )
            .await
            .map_err(GraphQLError::from)?;

        Ok(api_key.into())
    }

    /// Create a new tenant (admin write required)
    async fn create_tenant(
        &self,
        ctx: &Context<'_>,
        input: CreateTenantInput,
    ) -> FieldResult<GqlTenant> {
        let auth = get_auth_context(ctx)?;
        check_scope(&auth, &Scope::AdminWrite)?;

        let database = ctx.data::<Database>()?;

        // Convert input to BillingPlan
        let plan = match input.plan.plan_type.as_str() {
            "free" => BillingPlan::Free {
                monthly_events: input.plan.monthly_events.unwrap_or(10000),
            },
            "pro" => BillingPlan::Pro {
                monthly_events: input.plan.monthly_events.unwrap_or(100000),
                price_per_event: input.plan.price_per_event.unwrap_or(0.001),
            },
            "enterprise" => BillingPlan::Enterprise {
                unlimited: input.plan.unlimited.unwrap_or(true),
            },
            _ => return Err(GraphQLError::ValidationError("Invalid plan type".to_string()).extend()),
        };

        let tenant = Tenant::new(input.name, plan);
        database.create_tenant(&tenant).await.map_err(GraphQLError::from)?;

        Ok(tenant.into())
    }

    /// Create a new project
    async fn create_project(
        &self,
        ctx: &Context<'_>,
        input: CreateProjectInput,
    ) -> FieldResult<GqlProject> {
        let auth = get_auth_context(ctx)?;
        check_scope(&auth, &Scope::AdminWrite)?;

        let database = ctx.data::<Database>()?;

        let mut project = Project::new(auth.tenant_id.clone(), input.name);

        // Apply custom limits if provided
        if let Some(limits_input) = input.limits {
            project.limits = ProjectLimits {
                max_connections: limits_input.max_connections,
                max_events_per_sec: limits_input.max_events_per_sec,
                max_payload_size: limits_input.max_payload_size,
            };
        }

        database.create_project(&project).await.map_err(GraphQLError::from)?;

        Ok(project.into())
    }

    /// Revoke an API key (admin write required)
    async fn revoke_api_key(
        &self,
        ctx: &Context<'_>,
        key_id: ID,
    ) -> FieldResult<bool> {
        let auth = get_auth_context(ctx)?;
        check_scope(&auth, &Scope::AdminWrite)?;

        let auth_service = ctx.data::<AuthService>()?;

        auth_service
            .revoke_api_key(&auth.tenant_id, &key_id.to_string())
            .await
            .map_err(GraphQLError::from)?;

        Ok(true)
    }
}

/// Subscription root
pub struct SubscriptionRoot;

#[Subscription]
impl SubscriptionRoot {
    /// Subscribe to real-time events for specific topics
    async fn event_stream(
        &self,
        ctx: &Context<'_>,
        topics: Vec<String>,
    ) -> FieldResult<Pin<Box<dyn Stream<Item = GqlEvent> + Send>>> {
        let auth = get_auth_context(ctx)?;
        check_scope(&auth, &Scope::EventsSubscribe)?;

        let event_service = ctx.data::<EventService>()?;

        // Create subscription with tenant isolation
        let subscription = event_service
            .subscribe_to_topics(&auth.tenant_id, &auth.project_id, topics)
            .await
            .map_err(GraphQLError::from)?;

        // For now, return a simple empty stream since we don't have real event streaming yet
        let stream = tokio_stream::empty::<GqlEvent>();
        Ok(Box::pin(stream))
    }

    /// Subscribe to usage updates for a tenant
    async fn usage_updates(
        &self,
        ctx: &Context<'_>,
        tenant_id: Option<ID>,
    ) -> FieldResult<Pin<Box<dyn Stream<Item = GqlUsageRecord> + Send>>> {
        let auth = get_auth_context(ctx)?;
        check_scope(&auth, &Scope::BillingRead)?;

        // Enforce tenant isolation
        let target_tenant_id = tenant_id
            .map(|id| id.to_string())
            .unwrap_or_else(|| auth.tenant_id.clone());

        if target_tenant_id != auth.tenant_id {
            return Err(GraphQLError::Forbidden.extend());
        }

        // For now, return an empty stream as usage updates would require
        // a separate event system for billing events
        let stream = tokio_stream::empty::<GqlUsageRecord>();
        Ok(Box::pin(stream))
    }
}

/// Helper functions
fn get_auth_context(ctx: &Context<'_>) -> FieldResult<AuthContext> {
    ctx.data::<AuthContext>()
        .map(|auth| auth.clone())
        .map_err(|_| GraphQLError::Unauthorized.extend())
}

fn check_scope(auth: &AuthContext, required_scope: &Scope) -> FieldResult<()> {
    if auth.scopes.contains(required_scope) {
        Ok(())
    } else {
        Err(GraphQLError::Forbidden.extend())
    }
}

/// Create the GraphQL schema
pub fn create_schema(
    database: Database,
    event_service: EventService,
    auth_service: AuthService,
) -> ApiSchema {
    Schema::build(QueryRoot, MutationRoot, SubscriptionRoot)
        .data(database)
        .data(event_service)
        .data(auth_service)
        .finish()
}

/// GraphQL request handler with authentication
pub async fn graphql_handler(
    auth_context: AuthContext,
    schema: axum::extract::State<ApiSchema>,
    req: GraphQLRequest,
) -> GraphQLResponse {
    let request = req.into_inner().data(auth_context);
    schema.execute(request).await.into()
}

/// GraphQL subscription handler
pub async fn graphql_subscription_handler(
    auth_context: AuthContext,
    _schema: axum::extract::State<ApiSchema>,
    ws: WebSocketUpgrade,
) -> axum::response::Response {
    ws.on_upgrade(move |_socket| async move {
        // For now, just log the connection
        // In a real implementation, this would handle GraphQL subscriptions over WebSocket
        info!("GraphQL WebSocket connection established for tenant: {}", auth_context.tenant_id);
    })
}

/// GraphQL playground handler (development only)
pub async fn graphql_playground() -> axum::response::Html<&'static str> {
    axum::response::Html(
        r#"
        <!DOCTYPE html>
        <html>
        <head>
            <title>GraphQL Playground</title>
            <link rel="stylesheet" href="https://cdn.jsdelivr.net/npm/graphql-playground-react@1.7.26/build/static/css/index.css" />
        </head>
        <body>
            <div id="root"></div>
            <script src="https://cdn.jsdelivr.net/npm/graphql-playground-react@1.7.26/build/static/js/middleware.js"></script>
            <script>
                window.addEventListener('load', function (event) {
                    GraphQLPlayground.init(document.getElementById('root'), {
                        endpoint: '/graphql',
                        subscriptionEndpoint: '/graphql/ws',
                        settings: {
                            'request.credentials': 'include',
                        }
                    })
                })
            </script>
        </body>
        </html>
        "#,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_graphql_type_conversions() {
        // Test enum conversions
        let scope = Scope::EventsPublish;
        let gql_scope: GqlScope = scope.clone().into();
        let back_to_scope: Scope = gql_scope.into();
        assert_eq!(scope, back_to_scope);

        // Test status conversions
        let status = TenantStatus::Active;
        let gql_status: GqlTenantStatus = status.into();
        assert_eq!(gql_status, GqlTenantStatus::Active);
    }

    #[test]
    fn test_billing_plan_conversion() {
        let plan = BillingPlan::Pro {
            monthly_events: 100000,
            price_per_event: 0.001,
        };
        let gql_plan: GqlBillingPlan = plan.into();
        
        match gql_plan {
            GqlBillingPlan::Pro(pro_plan) => {
                assert_eq!(pro_plan.monthly_events, 100000);
                assert_eq!(pro_plan.price_per_event, 0.001);
            }
            _ => panic!("Expected Pro plan"),
        }
    }
}