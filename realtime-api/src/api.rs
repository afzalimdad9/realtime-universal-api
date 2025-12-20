use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::Json,
    Extension,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::auth::{AuthContext, AuthService};
use crate::database::Database;
use crate::event_service::{EventService, PublishResult};
use crate::models::{ApiKey, Event, Scope, Tenant, TenantStatus, UsageMetric, UsageRecord};

/// Application state shared across handlers
#[derive(Clone)]
pub struct AppState {
    pub database: Database,
    pub event_service: EventService,
    pub auth_service: AuthService,
}

/// Request payload for publishing events
#[derive(Debug, Deserialize)]
pub struct PublishEventRequest {
    pub topic: String,
    pub payload: Value,
}

/// Response for successful event publishing
#[derive(Debug, Serialize)]
pub struct PublishEventResponse {
    pub event_id: String,
    pub sequence: u64,
    pub published_at: String,
}

/// Request payload for creating API keys
#[derive(Debug, Deserialize)]
pub struct CreateApiKeyRequest {
    pub name: String,
    pub scopes: Vec<String>,
    pub rate_limit_per_sec: Option<i32>,
    pub expires_in_days: Option<i64>,
}

/// Response for API key creation
#[derive(Debug, Serialize)]
pub struct CreateApiKeyResponse {
    pub id: String,
    pub key: String,
    pub scopes: Vec<String>,
    pub rate_limit_per_sec: i32,
    pub expires_at: Option<String>,
}

/// Request payload for creating tenants
#[derive(Debug, Deserialize)]
pub struct CreateTenantRequest {
    pub name: String,
    pub plan: String,
}

/// Response for tenant creation
#[derive(Debug, Serialize)]
pub struct CreateTenantResponse {
    pub id: String,
    pub name: String,
    pub status: String,
    pub created_at: String,
}

/// Query parameters for usage reporting
#[derive(Debug, Deserialize)]
pub struct UsageQuery {
    pub metric: Option<String>,
    pub start_date: Option<String>,
    pub end_date: Option<String>,
}

/// Usage report response
#[derive(Debug, Serialize)]
pub struct UsageReportResponse {
    pub tenant_id: String,
    pub metrics: HashMap<String, i64>,
    pub period: String,
}

/// Error response structure
#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub error: ErrorDetail,
}

#[derive(Debug, Serialize)]
pub struct ErrorDetail {
    pub code: String,
    pub message: String,
    pub details: Option<Value>,
    pub request_id: String,
}

impl ErrorResponse {
    pub fn new(code: &str, message: &str, details: Option<Value>) -> Self {
        Self {
            error: ErrorDetail {
                code: code.to_string(),
                message: message.to_string(),
                details,
                request_id: Uuid::new_v4().to_string(),
            },
        }
    }
}

/// POST /events - Publish an event
pub async fn publish_event(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Json(request): Json<PublishEventRequest>,
) -> Result<Json<PublishEventResponse>, (StatusCode, Json<ErrorResponse>)> {
    // Check if the API key has publish permissions
    if !auth.scopes.contains(&Scope::EventsPublish) {
        warn!(
            "Insufficient permissions for event publishing: tenant={}, scopes={:?}",
            auth.tenant_id, auth.scopes
        );
        return Err((
            StatusCode::FORBIDDEN,
            Json(ErrorResponse::new(
                "INSUFFICIENT_SCOPE",
                "API key lacks events:publish permission",
                Some(json!({
                    "required_scope": "events:publish",
                    "available_scopes": auth.scopes
                })),
            )),
        ));
    }

    // Validate topic name
    if request.topic.is_empty() || request.topic.len() > 255 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse::new(
                "INVALID_TOPIC",
                "Topic name must be between 1 and 255 characters",
                Some(json!({
                    "topic": request.topic,
                    "length": request.topic.len()
                })),
            )),
        ));
    }

    // Validate payload size (1MB limit)
    let payload_size = serde_json::to_string(&request.payload)
        .map_err(|e| {
            error!("Failed to serialize payload: {}", e);
            (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse::new(
                    "INVALID_PAYLOAD",
                    "Payload must be valid JSON",
                    Some(json!({"error": e.to_string()})),
                )),
            )
        })?
        .len();

    if payload_size > 1024 * 1024 {
        return Err((
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(ErrorResponse::new(
                "PAYLOAD_TOO_LARGE",
                "Payload exceeds 1MB limit",
                Some(json!({
                    "size": payload_size,
                    "limit": 1024 * 1024
                })),
            )),
        ));
    }

    // Publish the event
    let event = Event::new(
        auth.tenant_id.clone(),
        auth.project_id.clone(),
        request.topic.clone(),
        request.payload,
    );
    
    match state
        .event_service
        .publish_event(&event)
        .await
    {
        Ok(PublishResult::Success) => {
            info!(
                "Event published successfully: event_id={}, tenant={}, project={}, topic={}",
                event.id, auth.tenant_id, auth.project_id, request.topic
            );

            Ok(Json(PublishEventResponse {
                event_id: event.id,
                sequence: 0, // Placeholder - would come from NATS in real implementation
                published_at: event.published_at.to_rfc3339(),
            }))
        }
        Ok(PublishResult::ValidationFailed(msg)) => {
            warn!("Event validation failed: {}", msg);
            Err((
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse::new(
                    "VALIDATION_FAILED",
                    &msg,
                    None,
                )),
            ))
        }
        Err(e) => {
            error!("Failed to publish event: {}", e);
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new(
                    "PUBLISH_FAILED",
                    "Failed to publish event",
                    Some(json!({"error": e.to_string()})),
                )),
            ))
        }
    }
}

/// POST /admin/tenants - Create a new tenant (admin only)
pub async fn create_tenant(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Json(request): Json<CreateTenantRequest>,
) -> Result<Json<CreateTenantResponse>, (StatusCode, Json<ErrorResponse>)> {
    // Check admin permissions
    if !auth.scopes.contains(&Scope::AdminWrite) {
        return Err((
            StatusCode::FORBIDDEN,
            Json(ErrorResponse::new(
                "INSUFFICIENT_SCOPE",
                "Admin write permission required",
                None,
            )),
        ));
    }

    // Validate tenant name
    if request.name.is_empty() || request.name.len() > 255 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse::new(
                "INVALID_TENANT_NAME",
                "Tenant name must be between 1 and 255 characters",
                None,
            )),
        ));
    }

    // Parse billing plan
    let plan = match request.plan.as_str() {
        "free" => crate::models::BillingPlan::Free { monthly_events: 10000 },
        "pro" => crate::models::BillingPlan::Pro {
            monthly_events: 100000,
            price_per_event: 0.001,
        },
        "enterprise" => crate::models::BillingPlan::Enterprise { unlimited: true },
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse::new(
                    "INVALID_PLAN",
                    "Plan must be one of: free, pro, enterprise",
                    None,
                )),
            ))
        }
    };

    // Create the tenant
    let tenant = Tenant::new(request.name, plan);

    match state.database.create_tenant(&tenant).await {
        Ok(_) => {
            info!("Created tenant: {} ({})", tenant.id, tenant.name);
            Ok(Json(CreateTenantResponse {
                id: tenant.id,
                name: tenant.name,
                status: format!("{:?}", tenant.status).to_lowercase(),
                created_at: tenant.created_at.to_rfc3339(),
            }))
        }
        Err(e) => {
            error!("Failed to create tenant: {}", e);
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new(
                    "TENANT_CREATION_FAILED",
                    "Failed to create tenant",
                    Some(json!({"error": e.to_string()})),
                )),
            ))
        }
    }
}

/// POST /admin/api-keys - Create a new API key
pub async fn create_api_key(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Json(request): Json<CreateApiKeyRequest>,
) -> Result<Json<CreateApiKeyResponse>, (StatusCode, Json<ErrorResponse>)> {
    // Check admin permissions
    if !auth.scopes.contains(&Scope::AdminWrite) {
        return Err((
            StatusCode::FORBIDDEN,
            Json(ErrorResponse::new(
                "INSUFFICIENT_SCOPE",
                "Admin write permission required",
                None,
            )),
        ));
    }

    // Parse scopes
    let mut scopes = Vec::new();
    for scope_str in &request.scopes {
        let scope = match scope_str.as_str() {
            "events:publish" => Scope::EventsPublish,
            "events:subscribe" => Scope::EventsSubscribe,
            "admin:read" => Scope::AdminRead,
            "admin:write" => Scope::AdminWrite,
            "billing:read" => Scope::BillingRead,
            _ => {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(ErrorResponse::new(
                        "INVALID_SCOPE",
                        &format!("Invalid scope: {}", scope_str),
                        Some(json!({
                            "valid_scopes": [
                                "events:publish",
                                "events:subscribe", 
                                "admin:read",
                                "admin:write",
                                "billing:read"
                            ]
                        })),
                    )),
                ))
            }
        };
        scopes.push(scope);
    }

    let rate_limit = request.rate_limit_per_sec.unwrap_or(100);
    let expires_at = request.expires_in_days.map(|days| {
        chrono::Utc::now() + chrono::Duration::days(days)
    });

    // Create the API key
    match state
        .auth_service
        .create_api_key(
            auth.tenant_id.clone(),
            auth.project_id.clone(),
            scopes.clone(),
            rate_limit,
            expires_at,
        )
        .await
    {
        Ok((raw_key, api_key)) => {
            info!("Created API key: {} for tenant: {}", api_key.id, auth.tenant_id);
            Ok(Json(CreateApiKeyResponse {
                id: api_key.id,
                key: raw_key,
                scopes: request.scopes,
                rate_limit_per_sec: rate_limit,
                expires_at: expires_at.map(|dt| dt.to_rfc3339()),
            }))
        }
        Err(e) => {
            error!("Failed to create API key: {}", e);
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new(
                    "API_KEY_CREATION_FAILED",
                    "Failed to create API key",
                    Some(json!({"error": e.to_string()})),
                )),
            ))
        }
    }
}

/// DELETE /admin/api-keys/{key_id} - Revoke an API key
pub async fn revoke_api_key(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(key_id): Path<String>,
) -> Result<StatusCode, (StatusCode, Json<ErrorResponse>)> {
    // Check admin permissions
    if !auth.scopes.contains(&Scope::AdminWrite) {
        return Err((
            StatusCode::FORBIDDEN,
            Json(ErrorResponse::new(
                "INSUFFICIENT_SCOPE",
                "Admin write permission required",
                None,
            )),
        ));
    }

    match state.auth_service.revoke_api_key(&auth.tenant_id, &key_id).await {
        Ok(_) => {
            info!("Revoked API key: {} for tenant: {}", key_id, auth.tenant_id);
            Ok(StatusCode::NO_CONTENT)
        }
        Err(e) => {
            error!("Failed to revoke API key: {}", e);
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::new(
                    "API_KEY_REVOCATION_FAILED",
                    "Failed to revoke API key",
                    Some(json!({"error": e.to_string()})),
                )),
            ))
        }
    }
}

/// GET /billing/usage - Get usage report for tenant
pub async fn get_usage_report(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Query(query): Query<UsageQuery>,
) -> Result<Json<UsageReportResponse>, (StatusCode, Json<ErrorResponse>)> {
    // Check billing read permissions
    if !auth.scopes.contains(&Scope::BillingRead) && !auth.scopes.contains(&Scope::AdminRead) {
        return Err((
            StatusCode::FORBIDDEN,
            Json(ErrorResponse::new(
                "INSUFFICIENT_SCOPE",
                "Billing read or admin read permission required",
                None,
            )),
        ));
    }

    let mut metrics = HashMap::new();

    // Get usage for different metrics
    let usage_metrics = vec![
        UsageMetric::EventsPublished,
        UsageMetric::EventsDelivered,
        UsageMetric::WebSocketMinutes,
        UsageMetric::ApiRequests,
    ];

    for metric in usage_metrics {
        match state.database.get_usage_for_tenant(&auth.tenant_id, metric.clone()).await {
            Ok(usage) => {
                let metric_name = match metric {
                    UsageMetric::EventsPublished => "events_published",
                    UsageMetric::EventsDelivered => "events_delivered",
                    UsageMetric::WebSocketMinutes => "websocket_minutes",
                    UsageMetric::ApiRequests => "api_requests",
                };
                metrics.insert(metric_name.to_string(), usage);
            }
            Err(e) => {
                error!("Failed to get usage for metric {:?}: {}", metric, e);
                // Continue with other metrics
            }
        }
    }

    Ok(Json(UsageReportResponse {
        tenant_id: auth.tenant_id,
        metrics,
        period: "current_month".to_string(),
    }))
}

/// POST /billing/stripe-webhook - Handle Stripe webhooks
pub async fn handle_stripe_webhook(
    State(state): State<AppState>,
    body: String,
) -> Result<StatusCode, (StatusCode, Json<ErrorResponse>)> {
    // TODO: Implement Stripe webhook signature verification
    // TODO: Handle different webhook event types (payment_succeeded, payment_failed, etc.)
    
    info!("Received Stripe webhook: {}", body.len());
    
    // For now, just acknowledge receipt
    // In a real implementation, this would:
    // 1. Verify the webhook signature
    // 2. Parse the webhook payload
    // 3. Handle different event types (payment success/failure, subscription changes)
    // 4. Update tenant status accordingly
    // 5. Trigger kill switch if payment fails
    
    Ok(StatusCode::OK)
}

/// Health check endpoint
pub async fn health_check(
    State(state): State<AppState>,
) -> Result<Json<Value>, (StatusCode, Json<ErrorResponse>)> {
    let is_healthy = state.event_service.is_healthy();
    
    if is_healthy {
        Ok(Json(json!({
            "status": "healthy",
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "services": {
                "database": "healthy",
                "nats": "healthy",
                "event_service": "healthy"
            }
        })))
    } else {
        Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ErrorResponse::new(
                "SERVICE_UNHEALTHY",
                "One or more services are unhealthy",
                None,
            )),
        ))
    }
}