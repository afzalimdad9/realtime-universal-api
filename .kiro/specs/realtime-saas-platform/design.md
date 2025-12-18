# Design Document

## Overview

The Realtime SaaS Platform is a high-performance, multi-tenant real-time communication service built with Rust and designed to handle 100k+ concurrent connections while maintaining low operational costs. The platform provides REST, WebSocket, and SSE endpoints with comprehensive billing, authentication, and administrative capabilities.

The system architecture prioritizes horizontal scalability, cost efficiency, and developer experience through well-designed SDKs and infrastructure-as-code deployment.

## Architecture

### High-Level Architecture

```
┌─────────────────┐    ┌─────────────────┐    ┌─────────────────┐
│   Client SDKs   │    │   Load Balancer │    │   Admin Portal  │
│  (JS/Rust/Py)  │    │     (Nginx)     │    │   (Future)      │
└─────────┬───────┘    └─────────┬───────┘    └─────────┬───────┘
          │                      │                      │
          └──────────────────────┼──────────────────────┘
                                 │
                    ┌─────────────▼─────────────┐
                    │     Axum API Gateway      │
                    │   (REST/WS/SSE/Admin)     │
                    └─────────────┬─────────────┘
                                  │
          ┌───────────────────────┼───────────────────────┐
          │                       │                       │
    ┌─────▼─────┐        ┌────────▼────────┐     ┌───────▼───────┐
    │PostgreSQL │        │ NATS JetStream  │     │  Observability│
    │(Metadata) │        │ (Event Stream)  │     │ (Prometheus)  │
    └───────────┘        └─────────────────┘     └───────────────┘
```

### Component Architecture

The system follows a modular architecture with clear separation of concerns:

- **API Gateway Layer**: Handles protocol termination (HTTP/WS/SSE) and routing
- **Authentication Layer**: Validates API keys, JWT tokens, and enforces scopes
- **Business Logic Layer**: Implements tenant isolation, billing, and usage tracking
- **Persistence Layer**: PostgreSQL for metadata, NATS JetStream for events
- **Observability Layer**: OpenTelemetry tracing and Prometheus metrics

## Components and Interfaces

### Core Services

#### 1. API Gateway Service
- **Responsibility**: Protocol handling, request routing, rate limiting
- **Interfaces**: 
  - REST endpoints (`/events`, `/admin`, `/billing`)
  - GraphQL endpoint (`/graphql`) with queries, mutations, and subscriptions
  - WebSocket upgrade handler (`/ws`)
  - SSE endpoint (`/sse`)
- **Dependencies**: Authentication service, usage tracker

#### 2. Authentication Service
- **Responsibility**: API key validation, JWT verification, scope enforcement
- **Interfaces**:
  - `verify_api_key(key: &str) -> Result<ApiKey, AuthError>`
  - `verify_jwt(token: &str) -> Result<Claims, AuthError>`
  - `check_scope(key: &ApiKey, scope: Scope) -> bool`
- **Dependencies**: PostgreSQL for key storage

#### 3. Event Publishing Service
- **Responsibility**: Event validation, tenant scoping, NATS publishing
- **Interfaces**:
  - `publish_event(tenant_id: &str, topic: &str, payload: &[u8]) -> Result<(), PublishError>`
  - `validate_event(event: &Event) -> Result<(), ValidationError>`
- **Dependencies**: NATS JetStream, usage tracker

#### 4. Subscription Management Service
- **Responsibility**: Managing WebSocket/SSE connections, event delivery
- **Interfaces**:
  - `subscribe(connection: Connection, topics: Vec<String>) -> Result<(), SubscribeError>`
  - `unsubscribe(connection_id: &str, topics: Vec<String>) -> Result<(), SubscribeError>`
- **Dependencies**: NATS JetStream consumers

#### 5. GraphQL Service
- **Responsibility**: GraphQL schema definition, query/mutation/subscription handling
- **Interfaces**:
  - **Queries**: `events(filter: EventFilter) -> [Event]`, `tenants() -> [Tenant]`, `projects(tenant_id: ID) -> [Project]`
  - **Mutations**: `publishEvent(input: EventInput) -> Event`, `createApiKey(input: ApiKeyInput) -> ApiKey`
  - **Subscriptions**: `eventStream(topics: [String]) -> Event`, `usageUpdates(tenant_id: ID) -> UsageMetric`
- **Dependencies**: Event service, authentication service, database

#### 6. Billing Service
- **Responsibility**: Usage tracking, Stripe integration, limit enforcement
- **Interfaces**:
  - `track_usage(tenant_id: &str, metric: UsageMetric, quantity: u64)`
  - `check_limits(tenant_id: &str) -> Result<(), LimitExceededError>`
  - `report_to_stripe(tenant_id: &str, usage: Usage) -> Result<(), BillingError>`
- **Dependencies**: PostgreSQL, Stripe API

#### 6. Admin Service
- **Responsibility**: Tenant management, API key operations, audit logging
- **Interfaces**:
  - `create_api_key(tenant_id: &str, scopes: Vec<Scope>) -> Result<ApiKey, AdminError>`
  - `rotate_api_key(key_id: &str) -> Result<ApiKey, AdminError>`
  - `suspend_tenant(tenant_id: &str, reason: &str) -> Result<(), AdminError>`
- **Dependencies**: PostgreSQL, audit logger

### External Integrations

#### NATS JetStream
- **Purpose**: Durable event streaming and persistence
- **Configuration**: 
  - Stream: `EVENTS` with subjects `events.{tenant_id}.{project_id}.{topic}`
  - Retention: Size-based with configurable limits per tenant
  - Consumers: Durable consumers for WebSocket/SSE delivery

#### PostgreSQL
- **Purpose**: Metadata storage for tenants, API keys, billing, audit logs
- **Schema**: Multi-tenant with proper indexing for performance
- **Connection**: Connection pooling with SQLx

#### Stripe API
- **Purpose**: Metered billing and subscription management
- **Integration**: Webhook handling for payment events, usage reporting
- **Security**: Webhook signature verification

## Data Models

### Core Entities

```rust
#[derive(Debug, Clone)]
pub struct Tenant {
    pub id: String,
    pub name: String,
    pub plan: BillingPlan,
    pub status: TenantStatus,
    pub stripe_customer_id: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct Project {
    pub id: String,
    pub tenant_id: String,
    pub name: String,
    pub limits: ProjectLimits,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct ApiKey {
    pub id: String,
    pub tenant_id: String,
    pub project_id: String,
    pub key_hash: String,
    pub scopes: Vec<Scope>,
    pub rate_limit_per_sec: u32,
    pub is_active: bool,
    pub expires_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct Event {
    pub id: String,
    pub tenant_id: String,
    pub project_id: String,
    pub topic: String,
    pub payload: serde_json::Value,
    pub published_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct UsageRecord {
    pub tenant_id: String,
    pub project_id: String,
    pub metric: UsageMetric,
    pub quantity: u64,
    pub window_start: DateTime<Utc>,
}
```

### Enumerations

```rust
#[derive(Debug, Clone, PartialEq)]
pub enum TenantStatus {
    Active,
    Trial,
    PastDue,
    Suspended,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Scope {
    EventsPublish,
    EventsSubscribe,
    AdminRead,
    AdminWrite,
    BillingRead,
}

#[derive(Debug, Clone, PartialEq)]
pub enum UsageMetric {
    EventsPublished,
    EventsDelivered,
    WebSocketMinutes,
    ApiRequests,
}

#[derive(Debug, Clone)]
pub enum BillingPlan {
    Free { monthly_events: u64 },
    Pro { monthly_events: u64, price_per_event: f64 },
    Enterprise { unlimited: bool },
}
```

## Correctness Properties

*A property is a characteristic or behavior that should hold true across all valid executions of a system—essentially, a formal statement about what the system should do. Properties serve as the bridge between human-readable specifications and machine-verifiable correctness guarantees.*

### Event Publishing Properties

**Property 1: Authenticated event acceptance**
*For any* valid authentication credentials and event payload, the REST API should accept the event and return a success response
**Validates: Requirements 1.1**

**Property 2: Event validation consistency**
*For any* event published via REST, the system should validate the payload against the topic schema before acceptance
**Validates: Requirements 1.2**

**Property 3: Tenant isolation enforcement**
*For any* published event, the system should enforce tenant and project scoping such that events never leak across tenant boundaries
**Validates: Requirements 1.3**

**Property 4: Permission-based rejection**
*For any* API key lacking publish permissions, the system should reject requests with appropriate error codes
**Validates: Requirements 1.4**

**Property 5: Rate limiting enforcement**
*For any* request burst exceeding rate limits, the system should throttle requests and return proper rate limit headers
**Validates: Requirements 1.5**

### WebSocket Connection Properties

**Property 6: WebSocket connection establishment**
*For any* valid authentication credentials, WebSocket connections should be accepted and enable event streaming
**Validates: Requirements 2.1**

**Property 7: Real-time event delivery**
*For any* event published to subscribed topics, all connected WebSocket clients should receive the event in real-time
**Validates: Requirements 2.2**

**Property 8: Connection limit enforcement**
*For any* tenant and project, WebSocket connections should be limited according to configured quotas
**Validates: Requirements 2.3**

**Property 9: Graceful reconnection handling**
*For any* network-induced connection drop, the system should handle reconnection gracefully with proper error messaging
**Validates: Requirements 2.4**

**Property 10: Tenant suspension termination**
*For any* suspended tenant, all WebSocket connections for that tenant should be immediately terminated
**Validates: Requirements 2.5**

### SSE Connection Properties

**Property 11: SSE connection establishment**
*For any* valid authentication credentials, SSE connections should be established as persistent HTTP connections for event streaming
**Validates: Requirements 3.1**

**Property 12: SSE event delivery formatting**
*For any* event published to subscribed topics, SSE delivery should use proper Server-Sent Events formatting
**Validates: Requirements 3.2**

**Property 13: SSE connection quotas**
*For any* tenant, SSE connections should be limited according to per-tenant connection quotas
**Validates: Requirements 3.3**

**Property 14: SSE resource cleanup**
*For any* SSE client disconnection, the system should clean up resources and update connection counts accurately
**Validates: Requirements 3.4**

### Authentication and Authorization Properties

**Property 15: API key generation security**
*For any* API key creation request, the system should generate cryptographically secure keys with configurable scopes
**Validates: Requirements 4.1**

**Property 16: API key validation and scope enforcement**
*For any* API key usage, the system should validate the key hash and enforce scope-based permissions correctly
**Validates: Requirements 4.2**

**Property 17: API key rotation grace period**
*For any* API key rotation, the system should provide a grace period before invalidating the old key
**Validates: Requirements 4.3**

**Property 18: API key revocation immediacy**
*For any* API key revocation, all access using that key should be immediately invalidated
**Validates: Requirements 4.4**

### Billing and Usage Properties

**Property 19: Usage tracking accuracy**
*For any* event published or delivered, usage metrics should be accurately tracked per tenant and project
**Validates: Requirements 5.1**

**Property 20: Stripe billing integration**
*For any* collected usage data, metered usage should be correctly reported to Stripe for billing
**Validates: Requirements 5.2**

**Property 21: Hard limit enforcement**
*For any* tenant exceeding plan limits, the system should enforce hard limits and prevent further usage
**Validates: Requirements 5.3**

**Property 22: Kill switch activation**
*For any* payment failure, tenant access should be suspended using the kill switch mechanism
**Validates: Requirements 5.4**

### Role-Based Access Control Properties

**Property 23: Role-based permission enforcement**
*For any* user with assigned roles, the system should enforce role-based permissions for all operations
**Validates: Requirements 6.1**

**Property 24: Admin function access validation**
*For any* admin function access attempt, the system should validate role permissions before allowing actions
**Validates: Requirements 6.2**

**Property 25: Role change propagation**
*For any* role change, access permissions should be immediately updated across all active sessions
**Validates: Requirements 6.3**

### Observability Properties

**Property 26: OpenTelemetry trace emission**
*For any* system operation, structured traces should be emitted via OpenTelemetry
**Validates: Requirements 7.1**

**Property 27: Prometheus metrics exposure**
*For any* collected metrics, the system should expose Prometheus-compatible metrics for monitoring
**Validates: Requirements 7.2**

**Property 28: Alert generation on errors**
*For any* error or anomaly, the system should generate alerts with appropriate severity levels
**Validates: Requirements 7.3**

### Event Persistence Properties

**Property 29: NATS JetStream persistence**
*For any* published event, the system should persist events using NATS JetStream for durability
**Validates: Requirements 10.1**

**Property 30: Cursor-based event replay**
*For any* event replay request, the system should provide cursor-based replay from specific timestamps or sequences
**Validates: Requirements 10.2**

**Property 31: Retention policy enforcement**
*For any* storage limit reached, the system should implement retention policies and cleanup procedures
**Validates: Requirements 10.3**

**Property 32: Dead letter queue routing**
*For any* failed event processing, events should be routed to dead letter queues
**Validates: Requirements 10.4**

### GraphQL API Properties

**Property 33: GraphQL query tenant isolation**
*For any* GraphQL query requesting tenant-scoped data, the system should enforce tenant isolation and only return data belonging to the authenticated tenant
**Validates: Requirements 1.3**

**Property 34: GraphQL mutation authentication**
*For any* GraphQL mutation operation, the system should validate authentication credentials and enforce scope-based permissions before executing the mutation
**Validates: Requirements 1.1, 1.4**

**Property 35: GraphQL subscription real-time delivery**
*For any* GraphQL subscription to event topics, the system should deliver events in real-time to all active subscribers with proper tenant isolation
**Validates: Requirements 2.2**

## Error Handling

### Error Categories

The system implements comprehensive error handling across all layers:

#### 1. Authentication Errors
- **InvalidApiKey**: API key not found or malformed
- **ExpiredApiKey**: API key has exceeded its expiration time
- **InsufficientScope**: API key lacks required permissions for operation
- **RateLimitExceeded**: Request rate exceeds configured limits

#### 2. Validation Errors
- **InvalidEventPayload**: Event payload fails schema validation
- **InvalidTopic**: Topic name violates naming conventions
- **PayloadTooLarge**: Event payload exceeds size limits

#### 3. Business Logic Errors
- **TenantSuspended**: Tenant account is suspended due to non-payment
- **ProjectLimitExceeded**: Project has exceeded connection or usage limits
- **BillingError**: Stripe integration or billing calculation failures

#### 4. Infrastructure Errors
- **DatabaseConnectionError**: PostgreSQL connection failures
- **NatsConnectionError**: NATS JetStream connectivity issues
- **NetworkError**: General network connectivity problems

### Error Response Format

All API errors follow a consistent JSON structure:

```json
{
  "error": {
    "code": "INVALID_API_KEY",
    "message": "The provided API key is invalid or expired",
    "details": {
      "key_id": "key_123",
      "expired_at": "2024-01-15T10:30:00Z"
    },
    "request_id": "req_abc123"
  }
}
```

### Retry and Circuit Breaker Patterns

- **Exponential Backoff**: Implemented for all external service calls (Stripe, NATS)
- **Circuit Breakers**: Protect against cascading failures in database and NATS connections
- **Dead Letter Queues**: Failed events are routed to DLQs for manual inspection
- **Graceful Degradation**: System continues operating with reduced functionality during partial outages

## Testing Strategy

### Dual Testing Approach

The platform employs both unit testing and property-based testing to ensure comprehensive coverage:

#### Unit Testing
- **Specific Examples**: Test concrete scenarios and edge cases
- **Integration Points**: Verify component interactions work correctly
- **Error Conditions**: Validate error handling and edge cases
- **Framework**: Uses Rust's built-in `#[cfg(test)]` and `tokio-test` for async testing

#### Property-Based Testing
- **Universal Properties**: Verify correctness properties hold across all inputs
- **Framework**: Uses `proptest` crate for Rust property-based testing
- **Minimum Iterations**: Each property test runs 100+ iterations with random inputs
- **Property Tagging**: Each test explicitly references design document properties

**Property-Based Testing Requirements**:
- Each correctness property must be implemented by a single property-based test
- Tests must be tagged with format: `**Feature: realtime-saas-platform, Property {number}: {property_text}**`
- Minimum 100 iterations per property test to ensure statistical confidence
- Smart generators that constrain input space to valid domains

#### Testing Libraries and Tools

**Core Testing Stack**:
- `proptest`: Property-based testing framework
- `tokio-test`: Async testing utilities
- `testcontainers`: Integration testing with real PostgreSQL and NATS
- `wiremock`: HTTP mocking for Stripe API testing
- `criterion`: Performance benchmarking

**Load Testing**:
- `k6`: WebSocket and HTTP load testing scripts
- Target: 100k+ concurrent connections
- Metrics: Latency, throughput, error rates, memory usage

### Test Organization

```
tests/
├── unit/                 # Unit tests co-located with source
├── integration/          # Integration tests with real services
├── property/            # Property-based tests for correctness properties
├── load/                # Load testing scripts and scenarios
└── fixtures/            # Test data and utilities
```

## Technology Stack

### Backend Core
- **Language**: Rust (latest stable)
- **Framework**: Axum 0.7+ for HTTP/WebSocket/SSE handling
- **GraphQL**: async-graphql 7.0+ for GraphQL API with subscriptions support
- **Runtime**: Tokio for async execution
- **Database ORM**: SQLx for type-safe PostgreSQL queries
- **Authentication**: `jsonwebtoken` crate for JWT handling

### External Services
- **Database**: PostgreSQL 15+ with connection pooling
- **Event Streaming**: NATS JetStream for durable event persistence
- **Billing**: Stripe API integration via `stripe-rs` crate
- **Observability**: OpenTelemetry + Prometheus metrics

### Client SDKs
- **JavaScript/TypeScript**: REST (axios), WebSocket (ws), SSE (eventsource)
- **Rust**: `reqwest` for HTTP, `tokio-tungstenite` for WebSocket
- **Python**: `aiohttp` for async HTTP, `websockets` for WebSocket

### Infrastructure
- **Containerization**: Docker with multi-stage builds
- **Orchestration**: Kubernetes with Helm charts
- **Infrastructure as Code**: Terraform modules for AWS/GCP/Azure
- **CI/CD**: GitHub Actions for build, test, and deployment

### Monitoring and Observability
- **Tracing**: OpenTelemetry with Jaeger backend
- **Metrics**: Prometheus with Grafana dashboards
- **Logging**: Structured JSON logs with correlation IDs
- **Alerting**: Grafana alerting for SLA violations and billing issues

## Deployment Architecture

### Kubernetes Components

```yaml
# Core application components
- Deployment: realtime-api (Axum application)
- StatefulSet: postgresql (database)
- StatefulSet: nats-jetstream (event streaming)
- Service: Load balancer and service discovery
- ConfigMap: Environment-specific configuration
- Secret: API keys, database credentials, Stripe keys
```

### Scaling Strategy

- **Horizontal Pod Autoscaler**: Scale API pods based on CPU/memory/connection count
- **Vertical Scaling**: PostgreSQL and NATS can scale vertically for increased throughput
- **Multi-Region**: Support for active-active deployment across regions
- **Connection Affinity**: WebSocket connections use session affinity for reliability

### Security Considerations

- **Network Policies**: Restrict pod-to-pod communication
- **RBAC**: Kubernetes role-based access control
- **Secret Management**: Kubernetes secrets with optional Vault integration
- **TLS Termination**: Nginx ingress with automatic certificate management
- **Container Security**: Non-root containers with minimal attack surface