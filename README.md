# Realtime SaaS Platform

A high-performance, multi-tenant real-time communication service built with Rust.

## Quick Start

### Prerequisites

- Rust 1.70+ 
- Docker and Docker Compose
- PostgreSQL client (optional, for direct database access)

### Development Setup

1. **Clone and setup environment**:
   ```bash
   cp .env.example .env
   # Edit .env with your preferred settings
   ```

2. **Start development services**:
   ```bash
   docker-compose up -d
   ```

3. **Build and run the application**:
   ```bash
   cargo build
   cargo run
   ```

### Services

When running `docker-compose up -d`, the following services will be available:

- **PostgreSQL**: `localhost:5432`
  - Database: `realtime_platform`
  - User: `postgres`
  - Password: `password`

- **NATS JetStream**: `localhost:4222`
  - Monitoring: `http://localhost:8222`

- **Jaeger Tracing**: `http://localhost:16686`
  - OTLP gRPC: `localhost:4317`
  - OTLP HTTP: `localhost:4318`

### Testing

```bash
# Run all tests
cargo test

# Run property-based tests
cargo test --test property_tests
```

## Architecture

The platform is built with:

- **Axum**: Web framework for REST/WebSocket/SSE endpoints
- **SQLx**: Type-safe PostgreSQL integration
- **NATS JetStream**: Durable event streaming
- **OpenTelemetry**: Distributed tracing and observability
- **Tokio**: Async runtime

## Development

This project follows the spec-driven development methodology. See `.kiro/specs/realtime-saas-platform/` for:

- `requirements.md`: Detailed requirements and acceptance criteria
- `design.md`: System architecture and correctness properties  
- `tasks.md`: Implementation plan and task list