# ABOUTME: Multi-stage Dockerfile for Matrix-Claude bridge
# ABOUTME: Builds Rust binary and creates minimal runtime image with proper user permissions

# Build stage
FROM rustlang/rust:nightly-bookworm as builder

WORKDIR /app

# Copy manifests
COPY Cargo.toml Cargo.lock ./

# Copy source code
COPY src ./src

# Build release binary
RUN cargo build --release

# Runtime stage
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

# Create non-root user
RUN useradd --create-home --shell /bin/bash claude

WORKDIR /app

# Copy binary from builder
COPY --from=builder /app/target/release/gorp /app/gorp

# Create directories for persistent data
RUN mkdir -p /app/sessions_db /app/crypto_store && \
    chown -R claude:claude /app

# Switch to non-root user
USER claude

# Volumes for persistent data
VOLUME ["/app/sessions_db", "/app/crypto_store"]

# Run the bot
CMD ["/app/gorp"]
