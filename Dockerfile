# ABOUTME: Multi-stage Dockerfile for gorp Matrix-Claude bridge
# ABOUTME: Uses dependency caching for fast rebuilds, creates minimal runtime image

# Build stage - cache dependencies separately from source
FROM rustlang/rust:nightly-bookworm AS builder

WORKDIR /app

# Copy manifests first (changes rarely)
COPY Cargo.toml Cargo.lock ./

# Create dummy source to build dependencies only
RUN mkdir src && \
    echo 'fn main() { println!("dummy"); }' > src/main.rs && \
    mkdir templates && \
    echo '' > templates/.gitkeep

# Build dependencies (this layer is cached unless Cargo.toml/lock changes)
RUN cargo build --release && \
    rm -rf src templates

# Now copy real source code, templates, and docs
COPY src ./src
COPY templates ./templates
COPY docs ./docs
COPY config.toml.example ./config.toml.example

# Touch main.rs to ensure rebuild (cargo sometimes skips if timestamp is old)
RUN touch src/main.rs

# Build the actual binary (deps are already cached)
RUN cargo build --release

# Runtime stage
FROM debian:bookworm-slim

# Install runtime dependencies, Node.js, and Chromium for Claude Code
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    curl \
    git \
    chromium \
    fonts-liberation \
    libnss3 \
    libatk-bridge2.0-0 \
    libgtk-3-0 \
    && curl -fsSL https://deb.nodesource.com/setup_22.x | bash - \
    && apt-get install -y nodejs \
    && rm -rf /var/lib/apt/lists/*

# Configure Chromium for headless Docker environment
ENV CHROME_BIN=/usr/bin/chromium
ENV CHROMIUM_FLAGS="--no-sandbox --disable-dev-shm-usage --headless"

# Install uv (Python package manager)
RUN curl -LsSf https://astral.sh/uv/install.sh | sh && \
    mv /root/.local/bin/uv /usr/local/bin/uv && \
    mv /root/.local/bin/uvx /usr/local/bin/uvx

# Install mise (runtime version manager)
RUN curl https://mise.run | sh && \
    mv /root/.local/bin/mise /usr/local/bin/mise

# Install Claude Code CLI globally
RUN npm install -g @anthropic-ai/claude-code

# Install MCP tools (chronicle, memory, toki, pagen)
RUN curl -fsSL https://github.com/harperreed/chronicle/releases/download/v1.1.4/chronicle-linux-amd64.tar.gz | tar -xz -C /tmp && \
    mv /tmp/chronicle-linux-amd64 /usr/local/bin/chronicle && \
    curl -fsSL https://github.com/harperreed/memory/releases/download/v0.3.3/memory_v0.3.3_Linux_x86_64.tar.gz | tar -xz -C /tmp && \
    mv /tmp/memory-linux-amd64 /usr/local/bin/memory && \
    curl -fsSL https://github.com/harperreed/toki/releases/download/v0.3.6/toki_0.3.6_Linux_x86_64.tar.gz | tar -xz -C /tmp && \
    mv /tmp/toki_0.3.6_Linux_x86_64/toki /usr/local/bin/toki && \
    rm -rf /tmp/toki_0.3.6_Linux_x86_64 && \
    curl -fsSL https://github.com/harperreed/pagen/releases/download/v0.4.4/pagen_v0.4.4_linux_amd64.tar.gz | tar -xz -C /tmp && \
    mv /tmp/pagen /usr/local/bin/pagen && \
    chmod +x /usr/local/bin/chronicle /usr/local/bin/memory /usr/local/bin/toki /usr/local/bin/pagen

# Create non-root user with home directory
RUN useradd --create-home --shell /bin/bash gorp

# Create Claude API key helper script (reads from env var, no secrets on disk)
RUN echo '#!/bin/sh\necho "$ANTHROPIC_API_KEY"' > /usr/local/bin/claude-api-key-helper && \
    chmod +x /usr/local/bin/claude-api-key-helper

# Copy binary from builder
COPY --from=builder /app/target/release/gorp /usr/local/bin/gorp

# Copy example config and entrypoint
COPY config.toml.example /app/config.toml.example
COPY docker-entrypoint.sh /app/docker-entrypoint.sh
RUN chmod +x /app/docker-entrypoint.sh

# Set up XDG directory structure for gorp user (including Claude config and MCP tools)
RUN mkdir -p /home/gorp/.config/gorp \
             /home/gorp/.config/claude \
             /home/gorp/.claude \
             /home/gorp/.local/share/gorp/crypto_store \
             /home/gorp/.local/share/gorp/logs \
             /home/gorp/.local/share/chronicle \
             /home/gorp/.local/share/memory \
             /home/gorp/.local/share/toki \
             /home/gorp/.local/share/pagen \
             /home/gorp/workspace && \
    chown -R gorp:gorp /home/gorp /app

# Switch to non-root user
USER gorp
WORKDIR /home/gorp

# Environment variables
ENV HOME=/home/gorp

# Claude Code uses ANTHROPIC_API_KEY for authentication (no OAuth needed)
# Set this when running the container: docker run -e ANTHROPIC_API_KEY=sk-ant-...
# Or in docker-compose.yml / .env file

# Volumes for persistent data (XDG-compliant paths)
# Mount .config/claude to persist Claude Code auth across container restarts
# Mount .claude for Claude CLI settings (API key)
# MCP tool data: chronicle, memory, toki, pagen
VOLUME ["/home/gorp/.config/gorp", "/home/gorp/.config/claude", "/home/gorp/.claude", "/home/gorp/.local/share/gorp", "/home/gorp/.local/share/chronicle", "/home/gorp/.local/share/memory", "/home/gorp/.local/share/toki", "/home/gorp/.local/share/pagen", "/home/gorp/workspace"]

# Expose webhook port
EXPOSE 13000

# Use entrypoint script for setup
ENTRYPOINT ["/app/docker-entrypoint.sh"]
CMD ["start"]
