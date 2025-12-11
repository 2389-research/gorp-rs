#!/bin/bash
# ABOUTME: Docker entrypoint script for gorp
# ABOUTME: Creates directories and copies example config on first run

set -e

CONFIG_DIR="/home/gorp/.config/gorp"
DATA_DIR="/home/gorp/.local/share/gorp"
WORKSPACE_DIR="/home/gorp/workspace"

# Create directories if they don't exist
mkdir -p "$CONFIG_DIR"
mkdir -p "$DATA_DIR/crypto_store"
mkdir -p "$DATA_DIR/logs"
mkdir -p "$WORKSPACE_DIR"

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
