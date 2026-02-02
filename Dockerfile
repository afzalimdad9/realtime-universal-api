# Build stage
FROM rust:1.93-slim as builder

# Install system dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Create app directory
WORKDIR /app

# Copy manifests
COPY Cargo.toml Cargo.lock ./
COPY realtime-api/Cargo.toml ./realtime-api/

# Create dummy source files to cache dependencies
RUN mkdir -p realtime-api/src && \
    echo "fn main() {}" > realtime-api/src/main.rs && \
    echo "pub fn dummy() {}" > realtime-api/src/lib.rs

# Build dependencies (this layer will be cached)
RUN cargo build --release && \
    rm -rf realtime-api/src

# Copy source code
COPY realtime-api/src ./realtime-api/src
COPY realtime-api/migrations ./realtime-api/migrations

# Build application
RUN touch realtime-api/src/main.rs && \
    cargo build --release

# Runtime stage
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

# Create app user
RUN useradd -r -s /bin/false -m -d /app appuser

# Copy binary from builder stage
COPY --from=builder /app/target/release/realtime-api /usr/local/bin/realtime-api

# Set ownership and permissions
RUN chown appuser:appuser /usr/local/bin/realtime-api && \
    chmod +x /usr/local/bin/realtime-api

# Switch to app user
USER appuser
WORKDIR /app

# Expose port
EXPOSE 3000

# Health check
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD curl -f http://localhost:3000/health || exit 1

# Run the application
CMD ["realtime-api"]