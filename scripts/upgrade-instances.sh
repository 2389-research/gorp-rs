#!/bin/bash
# ABOUTME: Upgrades existing app-data-* instances with new MCP volume mounts
# ABOUTME: Run after adding new MCP tools to ensure all instances have required directories

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

# MCP directories that should exist (path inside container : host subdir)
# Format: "container_path:host_subdir:is_config"
# is_config=1 means it goes under .config, otherwise .local/share
REQUIRED_MCP_DIRS=(
    "chronicle:chronicle:0"
    "memory:memory:0"
    "toki:toki:0"
    "pagen:pagen:0"
    "gsuite-mcp:gsuite-mcp:1"
    "digest:digest:0"
)

usage() {
    echo "Usage: $0 [options] [instance-num...]"
    echo ""
    echo "Upgrades app-data-* instances with new MCP volume mounts."
    echo ""
    echo "Options:"
    echo "  -r, --restart    Restart containers after upgrading"
    echo "  -n, --dry-run    Show what would be done without making changes"
    echo "  -h, --help       Show this help"
    echo ""
    echo "Examples:"
    echo "  $0               # Upgrade all instances"
    echo "  $0 8 9           # Upgrade only instances 8 and 9"
    echo "  $0 -r            # Upgrade all and restart"
    echo "  $0 -n            # Dry run - show what would change"
}

RESTART=false
DRY_RUN=false
INSTANCES=()

while [[ $# -gt 0 ]]; do
    case $1 in
        -r|--restart)
            RESTART=true
            shift
            ;;
        -n|--dry-run)
            DRY_RUN=true
            shift
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            INSTANCES+=("$1")
            shift
            ;;
    esac
done

# Find instances to upgrade
if [ ${#INSTANCES[@]} -eq 0 ]; then
    # Find all app-data-* directories
    for dir in "$PROJECT_DIR"/app-data-*; do
        if [ -d "$dir" ]; then
            num=$(basename "$dir" | sed 's/app-data-//')
            INSTANCES+=("$num")
        fi
    done
fi

if [ ${#INSTANCES[@]} -eq 0 ]; then
    echo "No app-data-* instances found."
    exit 0
fi

echo "=== Upgrade Instances ==="
echo ""

for num in "${INSTANCES[@]}"; do
    dir="$PROJECT_DIR/app-data-$num"

    if [ ! -d "$dir" ]; then
        echo "⚠️  Instance $num: app-data-$num not found, skipping"
        continue
    fi

    compose_file="$dir/docker-compose.yml"
    if [ ! -f "$compose_file" ]; then
        echo "⚠️  Instance $num: docker-compose.yml not found, skipping"
        continue
    fi

    echo "📦 Instance $num:"

    dirs_created=0
    mounts_added=0

    for entry in "${REQUIRED_MCP_DIRS[@]}"; do
        IFS=':' read -r name subdir is_config <<< "$entry"

        host_dir="$dir/mcp-data/$subdir"

        if [ "$is_config" = "1" ]; then
            container_path="/home/gorp/.config/$name"
        else
            container_path="/home/gorp/.local/share/$name"
        fi

        # Create host directory if missing
        if [ ! -d "$host_dir" ]; then
            if [ "$DRY_RUN" = true ]; then
                echo "  Would create: mcp-data/$subdir/"
            else
                mkdir -p "$host_dir"
                echo "  ✅ Created: mcp-data/$subdir/"
            fi
            ((dirs_created++))
        fi

        # Check if volume mount exists in docker-compose.yml
        mount_line="./mcp-data/$subdir:$container_path"
        if ! grep -q "mcp-data/$subdir" "$compose_file"; then
            if [ "$DRY_RUN" = true ]; then
                echo "  Would add mount: $mount_line"
            else
                # Add the volume mount after the last mcp-data line
                # Using awk for more reliable insertion
                awk -v mount="      - $mount_line" '
                    /mcp-data\// { last_mcp = NR; line = $0 }
                    { lines[NR] = $0 }
                    END {
                        for (i = 1; i <= NR; i++) {
                            print lines[i]
                            if (i == last_mcp) print mount
                        }
                    }
                ' "$compose_file" > "$compose_file.tmp" && mv "$compose_file.tmp" "$compose_file"
                echo "  ✅ Added mount: $mount_line"
            fi
            ((mounts_added++))
        fi
    done

    if [ $dirs_created -eq 0 ] && [ $mounts_added -eq 0 ]; then
        echo "  ✓ Already up to date"
    fi

    # Restart if requested and changes were made
    if [ "$RESTART" = true ] && [ $mounts_added -gt 0 ]; then
        if [ "$DRY_RUN" = true ]; then
            echo "  Would restart container"
        else
            echo "  🔄 Restarting container..."
            (cd "$dir" && docker compose down && docker compose up -d) 2>/dev/null || echo "  ⚠️  Failed to restart (container may not be running)"
        fi
    fi

    echo ""
done

echo "Done!"
if [ "$DRY_RUN" = true ]; then
    echo "(Dry run - no changes made)"
fi
