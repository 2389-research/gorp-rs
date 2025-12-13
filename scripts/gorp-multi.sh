#!/bin/bash
# ABOUTME: Management script for multi-instance gorp deployments
# ABOUTME: Provides start, stop, restart, update, status, and logs commands

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

# Find all app-data-* directories
get_instances() {
    find "$PROJECT_DIR" -maxdepth 1 -type d -name 'app-data-*' | sort -V
}

# Get instance number from path
get_instance_num() {
    echo "$1" | sed 's/.*app-data-//'
}

# Check if instance is running
is_running() {
    local num=$1
    docker ps --format '{{.Names}}' 2>/dev/null | grep -q "^gorp-$num$"
}

usage() {
    echo "Usage: $0 <command> [instance-num]"
    echo ""
    echo "Commands:"
    echo "  start [N]    Start all instances (or instance N)"
    echo "  stop [N]     Stop all instances (or instance N)"
    echo "  restart [N]  Restart all instances (or instance N)"
    echo "  update [N]   Rebuild and restart all instances (or instance N)"
    echo "  status       Show status of all instances"
    echo "  logs N       Follow logs for instance N"
    echo "  shell N      Open shell in instance N"
    echo "  remove N     Remove instance N (stops container, optionally deletes data)"
    echo "  list         List all configured instances"
    echo ""
    echo "Examples:"
    echo "  $0 start           # Start all instances"
    echo "  $0 start 3         # Start only instance 3"
    echo "  $0 update          # Rebuild image and restart all"
    echo "  $0 logs 1          # Follow logs for instance 1"
    echo "  $0 remove 2        # Remove instance 2"
    echo "  $0 status          # Show status of all instances"
}

cmd_start() {
    local target=$1
    local instances=$(get_instances)

    if [ -z "$instances" ]; then
        echo "No instances found. Run ./scripts/setup-instance.sh first."
        exit 1
    fi

    for dir in $instances; do
        local num=$(get_instance_num "$dir")
        if [ -n "$target" ] && [ "$num" != "$target" ]; then
            continue
        fi

        if is_running "$num"; then
            echo "gorp-$num: already running"
        else
            echo "gorp-$num: starting..."
            (cd "$dir" && docker compose up -d)
        fi
    done
}

cmd_stop() {
    local target=$1
    local instances=$(get_instances)

    for dir in $instances; do
        local num=$(get_instance_num "$dir")
        if [ -n "$target" ] && [ "$num" != "$target" ]; then
            continue
        fi

        if is_running "$num"; then
            echo "gorp-$num: stopping..."
            (cd "$dir" && docker compose down)
        else
            echo "gorp-$num: not running"
        fi
    done
}

cmd_restart() {
    local target=$1
    local instances=$(get_instances)

    for dir in $instances; do
        local num=$(get_instance_num "$dir")
        if [ -n "$target" ] && [ "$num" != "$target" ]; then
            continue
        fi

        echo "gorp-$num: restarting..."
        (cd "$dir" && docker compose restart)
    done
}

cmd_update() {
    local target=$1
    local instances=$(get_instances)

    echo "Building gorp image..."
    docker build -t gorp:latest "$PROJECT_DIR"
    echo ""

    for dir in $instances; do
        local num=$(get_instance_num "$dir")
        if [ -n "$target" ] && [ "$num" != "$target" ]; then
            continue
        fi

        echo "gorp-$num: updating..."
        (cd "$dir" && docker compose up -d --force-recreate)
    done
}

cmd_status() {
    local instances=$(get_instances)

    if [ -z "$instances" ]; then
        echo "No instances configured."
        exit 0
    fi

    printf "%-12s %-10s %-8s %s\n" "INSTANCE" "STATUS" "PORT" "UPTIME"
    printf "%-12s %-10s %-8s %s\n" "--------" "------" "----" "------"

    for dir in $instances; do
        local num=$(get_instance_num "$dir")
        local port=$((13000 + num))

        if is_running "$num"; then
            local uptime=$(docker ps --filter "name=gorp-$num" --format '{{.Status}}' | sed 's/Up //')
            printf "%-12s %-10s %-8s %s\n" "gorp-$num" "running" "$port" "$uptime"
        else
            printf "%-12s %-10s %-8s %s\n" "gorp-$num" "stopped" "$port" "-"
        fi
    done
}

cmd_logs() {
    local num=$1
    if [ -z "$num" ]; then
        echo "Error: Instance number required"
        echo "Usage: $0 logs <instance-num>"
        exit 1
    fi

    local dir="$PROJECT_DIR/app-data-$num"
    if [ ! -d "$dir" ]; then
        echo "Error: Instance $num not found"
        exit 1
    fi

    docker logs -f "gorp-$num"
}

cmd_shell() {
    local num=$1
    if [ -z "$num" ]; then
        echo "Error: Instance number required"
        echo "Usage: $0 shell <instance-num>"
        exit 1
    fi

    if ! is_running "$num"; then
        echo "Error: Instance $num is not running"
        exit 1
    fi

    docker exec -it "gorp-$num" /bin/bash
}

cmd_remove() {
    local num=$1
    if [ -z "$num" ]; then
        echo "Error: Instance number required"
        echo "Usage: $0 remove <instance-num>"
        exit 1
    fi

    local dir="$PROJECT_DIR/app-data-$num"
    if [ ! -d "$dir" ]; then
        echo "Error: Instance $num not found (no app-data-$num directory)"
        exit 1
    fi

    echo "=== Remove Instance gorp-$num ==="
    echo ""

    # Stop and remove container if running
    if is_running "$num"; then
        echo "Stopping gorp-$num..."
        (cd "$dir" && docker compose down)
    else
        # Try to remove any stopped container
        if docker ps -a --format '{{.Names}}' 2>/dev/null | grep -q "^gorp-$num$"; then
            echo "Removing stopped container gorp-$num..."
            docker rm "gorp-$num" 2>/dev/null || true
        fi
    fi

    echo ""
    echo "Container removed."
    echo ""
    echo "Data directory: $dir"
    echo ""
    read -p "Also delete app-data-$num directory? (y/N): " DELETE_DATA
    if [ "$DELETE_DATA" = "y" ] || [ "$DELETE_DATA" = "Y" ]; then
        echo "Deleting $dir..."
        rm -rf "$dir"
        echo "Instance $num completely removed."
    else
        echo "Data preserved. To fully remove later: rm -rf $dir"
    fi
}

cmd_list() {
    local instances=$(get_instances)

    if [ -z "$instances" ]; then
        echo "No instances configured."
        echo "Run ./scripts/setup-instance.sh to create one."
        exit 0
    fi

    echo "Configured instances:"
    for dir in $instances; do
        local num=$(get_instance_num "$dir")
        local port=$((13000 + num))

        # Try to read bot user from config
        local bot_user="-"
        if [ -f "$dir/config/config.toml" ]; then
            bot_user=$(grep 'user_id' "$dir/config/config.toml" 2>/dev/null | head -1 | sed 's/.*= *"\(.*\)"/\1/' || echo "-")
        fi

        echo "  gorp-$num: port $port, bot: $bot_user"
    done
}

# Main
case "${1:-}" in
    start)
        cmd_start "$2"
        ;;
    stop)
        cmd_stop "$2"
        ;;
    restart)
        cmd_restart "$2"
        ;;
    update)
        cmd_update "$2"
        ;;
    status)
        cmd_status
        ;;
    logs)
        cmd_logs "$2"
        ;;
    shell)
        cmd_shell "$2"
        ;;
    remove)
        cmd_remove "$2"
        ;;
    list)
        cmd_list
        ;;
    -h|--help|help|"")
        usage
        ;;
    *)
        echo "Unknown command: $1"
        usage
        exit 1
        ;;
esac
