#!/bin/bash
# ABOUTME: Sets up directory structure for multi-user gorp deployment
# ABOUTME: Creates app-data-{1..10} directories with config templates

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

NUM_USERS=${1:-10}

echo "Setting up $NUM_USERS gorp instances..."

for i in $(seq 1 $NUM_USERS); do
    DIR="$PROJECT_DIR/app-data-$i"

    if [ -d "$DIR" ]; then
        echo "  app-data-$i: already exists, skipping"
        continue
    fi

    echo "  Creating app-data-$i..."

    mkdir -p "$DIR/config"
    mkdir -p "$DIR/claude-config"
    mkdir -p "$DIR/data"
    mkdir -p "$DIR/workspace"
    mkdir -p "$DIR/mcp-data/chronicle"
    mkdir -p "$DIR/mcp-data/memory"
    mkdir -p "$DIR/mcp-data/toki"
    mkdir -p "$DIR/mcp-data/pagen"

    # Copy example config if it exists
    if [ -f "$PROJECT_DIR/config.toml.example" ]; then
        cp "$PROJECT_DIR/config.toml.example" "$DIR/config/config.toml"
    fi

    # Create .env file with placeholder
    cat > "$DIR/.env" << EOF
# Environment for gorp instance $i
# ANTHROPIC_API_KEY=sk-ant-...
EOF

    echo "    - Created directory structure"
    echo "    - Edit $DIR/config/config.toml with Matrix credentials"
    echo "    - Edit $DIR/.env with ANTHROPIC_API_KEY"
done

echo ""
echo "Done! To start all instances:"
echo "  docker compose -f docker-compose.multi.yml up -d"
echo ""
echo "To start specific instances:"
echo "  docker compose -f docker-compose.multi.yml up -d gorp-1 gorp-2"
echo ""
echo "Ports:"
for i in $(seq 1 $NUM_USERS); do
    echo "  gorp-$i: localhost:1300$i"
done
