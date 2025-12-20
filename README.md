# Realtime Universal API

[![Rust](https://img.shields.io/badge/rust-1.70+-orange.svg)](https://www.rust-lang.org)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Docker](https://img.shields.io/badge/docker-ready-blue.svg)](https://www.docker.com)
[![GraphQL](https://img.shields.io/badge/GraphQL-ready-e10098.svg)](https://graphql.org)
[![WebSocket](https://img.shields.io/badge/WebSocket-supported-green.svg)](https://developer.mozilla.org/en-US/docs/Web/API/WebSockets_API)

🚀 **Universal Real-time Communication Platform** - A high-performance, multi-tenant real-time communication service built with Rust, designed to support 40+ protocols and handle 100k+ concurrent connections.

## ✨ Features

- **🌐 Universal Protocol Support**: REST, GraphQL, WebSocket, SSE, gRPC, MQTT, AMQP, Kafka, and 30+ more protocols
- **⚡ High Performance**: Built with Rust and Tokio for maximum throughput and minimal latency
- **🏢 Multi-Tenant SaaS**: Complete tenant isolation, billing integration, and admin capabilities
- **📊 Real-time Streaming**: WebSocket, SSE, and GraphQL subscriptions with NATS JetStream persistence
- **🔐 Enterprise Security**: JWT, API keys, OAuth 2.0, SAML, mTLS authentication
- **📈 Observability**: OpenTelemetry tracing, Prometheus metrics, structured logging
- **🐳 Cloud Native**: Docker, Kubernetes, Helm charts, and infrastructure-as-code ready

## 🚀 Protocol Support Matrix

| Category | Protocol | Status | Category | Protocol | Status |
|----------|----------|--------|----------|----------|--------|
| **HTTP** | REST API | ✅ | **Messaging** | NATS JetStream | ✅ |
| | GraphQL | ✅ | | MQTT | ⏳ |
| | WebSocket | ✅ | | AMQP | ⏳ |
| | SSE | ✅ | | Kafka | ⏳ |
| | HTTP/2 | ⏳ | **RPC** | gRPC | ⏳ |
| | HTTP/3 | ⏳ | | JSON-RPC | ⏳ |
| **Auth** | JWT | ✅ | | Apache Thrift | ⏳ |
| | OAuth 2.0 | ⏳ | **File** | SFTP/SCP | ⏳ |
| | SAML | ⏳ | | WebDAV | ⏳ |

*✅ = Implemented, ⏳ = Planned - [View complete protocol roadmap →](.kiro/specs/realtime-saas-platform/design.md#protocol-support-roadmap)*

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

## 🏗️ Architecture

The platform is built with:

- **🌐 Axum**: Web framework for REST/WebSocket/SSE endpoints
- **🗄️ SQLx**: Type-safe PostgreSQL integration with connection pooling
- **📡 NATS JetStream**: Durable event streaming and message persistence
- **📊 OpenTelemetry**: Distributed tracing and observability
- **⚡ Tokio**: High-performance async runtime
- **🔍 GraphQL**: async-graphql with subscriptions support

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
                    │   (REST/WS/SSE/GraphQL)   │
                    └─────────────┬─────────────┘
                                  │
          ┌───────────────────────┼───────────────────────┐
          │                       │                       │
    ┌─────▼─────┐        ┌────────▼────────┐     ┌───────▼───────┐
    │PostgreSQL │        │ NATS JetStream  │     │  Observability│
    │(Metadata) │        │ (Event Stream)  │     │ (Prometheus)  │
    └───────────┘        └─────────────────┘     └───────────────┘
```

## 📚 Development

This project follows **spec-driven development**. See [`.kiro/specs/realtime-saas-platform/`](.kiro/specs/realtime-saas-platform/) for:

- **[`requirements.md`](.kiro/specs/realtime-saas-platform/requirements.md)**: Detailed requirements and acceptance criteria
- **[`design.md`](.kiro/specs/realtime-saas-platform/design.md)**: System architecture and correctness properties  
- **[`tasks.md`](.kiro/specs/realtime-saas-platform/tasks.md)**: Implementation plan and task list

## 🤝 Contributing

We welcome contributions! Please see our [Contributing Guidelines](CONTRIBUTING.md) for details.

## 📄 License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## 🌟 Star History

If you find this project useful, please consider giving it a star! ⭐