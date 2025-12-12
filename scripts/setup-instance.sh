#!/bin/bash
# ABOUTME: Interactive setup script for launching a new gorp instance
# ABOUTME: Prompts for Matrix credentials and Anthropic API key, then starts container

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

echo "========================================="
echo "  gorp Instance Setup"
echo "========================================="
echo ""

# Find next available instance number
INSTANCE_NUM=1
while [ -d "$PROJECT_DIR/app-data-$INSTANCE_NUM" ]; do
    INSTANCE_NUM=$((INSTANCE_NUM + 1))
done

read -p "Instance number [$INSTANCE_NUM]: " INPUT_NUM
INSTANCE_NUM=${INPUT_NUM:-$INSTANCE_NUM}

APP_DIR="$PROJECT_DIR/app-data-$INSTANCE_NUM"
PORT=$((13000 + INSTANCE_NUM))

if [ -d "$APP_DIR" ]; then
    echo ""
    echo "Warning: app-data-$INSTANCE_NUM already exists!"
    read -p "Overwrite? (y/N): " OVERWRITE
    if [ "$OVERWRITE" != "y" ] && [ "$OVERWRITE" != "Y" ]; then
        echo "Aborted."
        exit 1
    fi
    rm -rf "$APP_DIR"
fi

echo ""
echo "--- Matrix Bot Configuration ---"
echo ""

read -p "Bot Matrix User ID (e.g., @gorp-bot:matrix.org): " BOT_USER_ID
if [ -z "$BOT_USER_ID" ]; then
    echo "Error: Bot user ID is required"
    exit 1
fi

# Extract homeserver from user ID
HOMESERVER=$(echo "$BOT_USER_ID" | sed 's/.*:\(.*\)/\1/')
read -p "Matrix Homeserver [https://$HOMESERVER]: " INPUT_HOMESERVER
HOMESERVER_URL=${INPUT_HOMESERVER:-"https://$HOMESERVER"}

read -sp "Bot Matrix Password: " BOT_PASSWORD
echo ""
if [ -z "$BOT_PASSWORD" ]; then
    echo "Error: Bot password is required"
    exit 1
fi

read -sp "Bot Recovery Key (optional, press Enter to skip): " RECOVERY_KEY
echo ""

read -p "Room Prefix (e.g., GORP): " ROOM_PREFIX
if [ -z "$ROOM_PREFIX" ]; then
    echo "Error: Room prefix is required"
    exit 1
fi

echo ""
echo "--- User Configuration ---"
echo ""

read -p "Allowed Matrix User ID (e.g., @you:matrix.org): " ALLOWED_USER
if [ -z "$ALLOWED_USER" ]; then
    echo "Error: Allowed user is required"
    exit 1
fi

echo ""
echo "--- API Configuration ---"
echo ""

read -sp "Anthropic API Key (sk-ant-...): " ANTHROPIC_KEY
echo ""
if [ -z "$ANTHROPIC_KEY" ]; then
    echo "Error: Anthropic API key is required"
    exit 1
fi

echo ""
echo "--- Creating Instance ---"
echo ""

# Create directory structure
mkdir -p "$APP_DIR/config"
mkdir -p "$APP_DIR/claude-config"
mkdir -p "$APP_DIR/data"
mkdir -p "$APP_DIR/workspace"
mkdir -p "$APP_DIR/mcp-data/chronicle"
mkdir -p "$APP_DIR/mcp-data/memory"
mkdir -p "$APP_DIR/mcp-data/toki"
mkdir -p "$APP_DIR/mcp-data/pagen"

echo "  Created directory: app-data-$INSTANCE_NUM/"

# Create config.toml
cat > "$APP_DIR/config/config.toml" << EOF
# gorp configuration for instance $INSTANCE_NUM

[matrix]
home_server = "$HOMESERVER_URL"
user_id = "$BOT_USER_ID"
password = "$BOT_PASSWORD"
device_name = "gorp-$INSTANCE_NUM"
room_prefix = "$ROOM_PREFIX"
allowed_users = ["$ALLOWED_USER"]
EOF

# Add recovery key if provided
if [ -n "$RECOVERY_KEY" ]; then
    echo "recovery_key = \"$RECOVERY_KEY\"" >> "$APP_DIR/config/config.toml"
fi

cat >> "$APP_DIR/config/config.toml" << EOF

[webhook]
port = 13000
host = "0.0.0.0"

[workspace]
path = "/home/gorp/workspace"

[scheduler]
timezone = "America/Chicago"
EOF

echo "  Created config.toml"

# Create .env file
cat > "$APP_DIR/.env" << EOF
ANTHROPIC_API_KEY=$ANTHROPIC_KEY
EOF

echo "  Created .env"

# Create docker-compose override for this instance
cat > "$APP_DIR/docker-compose.yml" << EOF
# Docker Compose for gorp instance $INSTANCE_NUM
# Run with: docker compose -f app-data-$INSTANCE_NUM/docker-compose.yml up -d

services:
  gorp-$INSTANCE_NUM:
    build: $PROJECT_DIR
    image: gorp:latest
    container_name: gorp-$INSTANCE_NUM
    restart: unless-stopped
    env_file:
      - .env
    ports:
      - "$PORT:13000"
    volumes:
      - ./config:/home/gorp/.config/gorp
      - ./claude-config:/home/gorp/.config/claude
      - ./data:/home/gorp/.local/share/gorp
      - ./workspace:/home/gorp/workspace
      - ./mcp-data/chronicle:/home/gorp/.local/share/chronicle
      - ./mcp-data/memory:/home/gorp/.local/share/memory
      - ./mcp-data/toki:/home/gorp/.local/share/toki
      - ./mcp-data/pagen:/home/gorp/.local/share/pagen
    logging:
      driver: "json-file"
      options:
        max-size: "10m"
        max-file: "3"
EOF

echo "  Created docker-compose.yml"

echo ""
echo "========================================="
echo "  Instance $INSTANCE_NUM Ready!"
echo "========================================="
echo ""
echo "  Directory:  app-data-$INSTANCE_NUM/"
echo "  Port:       $PORT"
echo "  Bot:        $BOT_USER_ID"
echo "  User:       $ALLOWED_USER"
echo "  Prefix:     $ROOM_PREFIX"
echo ""
echo "To start:"
echo "  cd $APP_DIR && docker compose up -d"
echo ""
echo "Or from project root:"
echo "  docker compose -f app-data-$INSTANCE_NUM/docker-compose.yml up -d"
echo ""

read -p "Start instance now? (Y/n): " START_NOW
if [ "$START_NOW" != "n" ] && [ "$START_NOW" != "N" ]; then
    echo ""
    echo "Building and starting gorp-$INSTANCE_NUM..."
    cd "$APP_DIR"
    docker compose up -d --build
    echo ""
    echo "Instance started! Check logs with:"
    echo "  docker logs -f gorp-$INSTANCE_NUM"
fi
