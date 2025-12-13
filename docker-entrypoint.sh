#!/bin/bash
# ABOUTME: Docker entrypoint script for gorp
# ABOUTME: Creates directories, configures Claude MCP servers, and copies example config on first run

set -e

CONFIG_DIR="/home/gorp/.config/gorp"
DATA_DIR="/home/gorp/.local/share/gorp"
WORKSPACE_DIR="/home/gorp/workspace"
CLAUDE_CONFIG="/home/gorp/.claude.json"
CLAUDE_SETTINGS_DIR="/home/gorp/.claude"

# Create directories if they don't exist
mkdir -p "$CONFIG_DIR"
mkdir -p "$DATA_DIR/crypto_store"
mkdir -p "$DATA_DIR/logs"
mkdir -p "$WORKSPACE_DIR"
mkdir -p "$CLAUDE_SETTINGS_DIR"
mkdir -p "/home/gorp/.local/share/chronicle"
mkdir -p "/home/gorp/.local/share/memory"
mkdir -p "/home/gorp/.local/share/toki"
mkdir -p "/home/gorp/.local/share/pagen"

# Configure Claude CLI settings with default plugins
CLAUDE_SETTINGS="$CLAUDE_SETTINGS_DIR/settings.json"
CLAUDE_SETTINGS_TARBALL="/app/claude-settings.clean.tgz"
if [ ! -f "$CLAUDE_SETTINGS" ]; then
    if [ -w "$CLAUDE_SETTINGS_DIR" ]; then
        # Extract default claude-settings with plugins if tarball exists
        if [ -f "$CLAUDE_SETTINGS_TARBALL" ]; then
            echo "Extracting default Claude settings with plugins..."
            tar -xzf "$CLAUDE_SETTINGS_TARBALL" -C /tmp/
            cp -r /tmp/claude-settings.clean/* "$CLAUDE_SETTINGS_DIR/"
            rm -rf /tmp/claude-settings.clean
            echo "Claude settings with plugins extracted."
        else
            # Fallback to minimal settings (tarball includes apiKeyHelper)
            echo "Configuring Claude CLI (minimal)..."
            cat > "$CLAUDE_SETTINGS" << 'EOF'
{
    "apiKeyHelper": "/usr/local/bin/claude-api-key-helper"
}
EOF
            echo "Claude CLI configured."
        fi
    else
        echo "Warning: Cannot write to $CLAUDE_SETTINGS_DIR (permission denied)"
    fi
fi

# Set up Claude Code MCP servers if not already configured
if [ ! -f "$CLAUDE_CONFIG" ]; then
    echo "Setting up Claude Code MCP servers..."
    cat > "$CLAUDE_CONFIG" << 'EOF'
{
  "mcpServers": {
    "chronicle": {
      "type": "stdio",
      "command": "chronicle",
      "args": ["mcp"],
      "env": {}
    },
    "memory": {
      "type": "stdio",
      "command": "memory",
      "args": ["mcp"],
      "env": {}
    },
    "toki": {
      "type": "stdio",
      "command": "toki",
      "args": ["mcp"],
      "env": {}
    },
    "pagen": {
      "type": "stdio",
      "command": "pagen",
      "args": ["mcp"],
      "env": {}
    },
    "gsuite": {
      "type": "stdio",
      "command": "gsuite-mcp",
      "args": ["mcp"],
      "env": {}
    },
    "gorp": {
      "type": "http",
      "url": "http://localhost:13000/mcp"
    }
  }
}
EOF
    echo "Claude Code MCP servers configured."
fi

# Copy example config if no config exists
if [ ! -f "$CONFIG_DIR/config.toml" ]; then
    if [ -f "/app/config.toml.example" ]; then
        echo "No config found. Copying example config to $CONFIG_DIR/config.toml"
        echo "Please edit this file with your Matrix credentials."
        cp /app/config.toml.example "$CONFIG_DIR/config.toml"
    else
        echo "Warning: No config.toml found in $CONFIG_DIR"
        echo "You can either:"
        echo "  1. Mount a config file: -v ./config:/home/gorp/.config/gorp"
        echo "  2. Use environment variables:"
        echo "     MATRIX_HOME_SERVER, MATRIX_USER_ID, MATRIX_PASSWORD,"
        echo "     ALLOWED_USERS (comma-separated Matrix user IDs)"
    fi
fi

# If first argument is a flag, assume we're running gorp
if [ "${1#-}" != "$1" ]; then
    set -- gorp "$@"
fi

# If no arguments or "start", run gorp start
if [ $# -eq 0 ] || [ "$1" = "start" ]; then
    exec gorp start
fi

# Otherwise, pass through to gorp or the command
if [ "$1" = "gorp" ]; then
    exec "$@"
else
    exec gorp "$@"
fi
