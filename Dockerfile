# ABOUTME: Multi-stage Dockerfile for gorp Matrix-Claude bridge
# ABOUTME: Builds Rust binary and creates minimal runtime image with XDG directory structure

# Build stage
FROM rustlang/rust:nightly-bookworm as builder

WORKDIR /app

# Copy manifests
COPY Cargo.toml Cargo.lock ./

# Copy source code and templates (Askama compiles templates into binary)
COPY src ./src
COPY templates ./templates

# Copy config.toml.example (needed for include_str! at compile time)
COPY config.toml.example ./config.toml.example

# Build release binary
RUN cargo build --release

# Runtime stage
FROM debian:bookworm-slim

# Install runtime dependencies and Node.js for Claude Code
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    curl \
    && curl -fsSL https://deb.nodesource.com/setup_22.x | bash - \
    && apt-get install -y nodejs \
    && rm -rf /var/lib/apt/lists/*

# Install Claude Code CLI globally
RUN npm install -g @anthropic-ai/claude-code

# Create non-root user with home directory
RUN useradd --create-home --shell /bin/bash gorp

# Copy binary from builder
COPY --from=builder /app/target/release/gorp /usr/local/bin/gorp

# Copy example config and entrypoint
COPY config.toml.example /app/config.toml.example
COPY docker-entrypoint.sh /app/docker-entrypoint.sh
RUN chmod +x /app/docker-entrypoint.sh

# Set up XDG directory structure for gorp user (including Claude config)
RUN mkdir -p /home/gorp/.config/gorp \
             /home/gorp/.config/claude \
             /home/gorp/.local/share/gorp/crypto_store \
             /home/gorp/.local/share/gorp/logs \
             /home/gorp/workspace && \
    chown -R gorp:gorp /home/gorp /app

# Switch to non-root user
USER gorp
WORKDIR /home/gorp

# Environment variables
ENV HOME=/home/gorp
# Claude Code auth: either set ANTHROPIC_API_KEY env var, or shell in and run `claude` to login
# ENV ANTHROPIC_API_KEY=your-key-here

# Volumes for persistent data (XDG-compliant paths)
# Mount .config/claude to persist Claude Code auth across container restarts
VOLUME ["/home/gorp/.config/gorp", "/home/gorp/.config/claude", "/home/gorp/.local/share/gorp", "/home/gorp/workspace"]

# Expose webhook port
EXPOSE 13000

# Use entrypoint script for setup
ENTRYPOINT ["/app/docker-entrypoint.sh"]
CMD ["start"]
