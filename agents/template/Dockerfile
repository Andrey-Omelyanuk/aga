# Stage 1: Build
FROM rust:1.75-bookworm AS builder

WORKDIR /build

# Install dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    curl \
    && rm -rf /var/lib/apt/lists/*

# Copy Cargo files
COPY Cargo.toml ./
COPY Cargo.lock ./

# Copy source code
COPY src ./src

# Build release binary
RUN cargo build --release

# Stage 2: Runtime
FROM debian:bookworm-slim

WORKDIR /app

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    curl \
    && rm -rf /var/lib/apt/lists/*

# Copy built binary from builder
COPY --from=builder /build/target/release/agent-template /app/agent-template

# Create necessary directories
RUN mkdir -p /models /shared-files

# Set environment variables for model configuration
ENV RUST_LOG=info
ENV NATS_URL=nats://nats:4222
ENV DB_HOST=postgres
ENV DB_PORT=5432
ENV DB_USER=agent_user
ENV DB_PASSWORD=agent_pass
ENV DB_NAME=agents_db

# Expose port for health checks (optional)
EXPOSE 8082

# Run the agent
CMD ["./agent-template"]
