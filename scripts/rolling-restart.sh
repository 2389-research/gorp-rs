#!/bin/bash
# ABOUTME: Performs rolling restart of gorp containers to minimize downtime
# ABOUTME: Restarts containers one at a time, waiting for health before continuing

set -e

COMPOSE_FILE="${COMPOSE_FILE:-docker-compose.multi.yml}"
DELAY="${DELAY:-10}"  # Seconds to wait between restarts

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

show_usage() {
    echo "Usage: $0 [OPTIONS]"
    echo ""
    echo "Perform rolling restart of gorp containers."
    echo ""
    echo "Options:"
    echo "  --delay N     Wait N seconds between restarts (default: 10)"
    echo "  --dry-run     Show what would be done without doing it"
    echo ""
    echo "Environment:"
    echo "  COMPOSE_FILE  Docker compose file (default: docker-compose.multi.yml)"
    echo "  DELAY         Seconds between restarts (default: 10)"
}

DRY_RUN=false

while [[ $# -gt 0 ]]; do
    case "$1" in
        --delay)
            DELAY="$2"
            shift 2
            ;;
        --dry-run)
            DRY_RUN=true
            shift
            ;;
        -h|--help)
            show_usage
            exit 0
            ;;
        *)
            echo -e "${RED}Unknown option: $1${NC}"
            show_usage
            exit 1
            ;;
    esac
done

# Get list of gorp services from compose file
get_services() {
    docker compose -f "$COMPOSE_FILE" config --services 2>/dev/null | grep -E '^gorp-[0-9]+$' | sort -V
}

# Check if container is running
is_running() {
    local container="$1"
    docker ps --format '{{.Names}}' | grep -q "^${container}$"
}

# Restart a single service
restart_service() {
    local service="$1"

    echo -e "${YELLOW}Restarting $service...${NC}"

    if $DRY_RUN; then
        echo "  [DRY RUN] Would restart $service"
        return 0
    fi

    # Stop and start (not just restart, to pick up new image)
    docker compose -f "$COMPOSE_FILE" stop "$service"
    docker compose -f "$COMPOSE_FILE" up -d "$service"

    # Wait for container to be running
    local attempts=0
    while ! is_running "$service" && [[ $attempts -lt 30 ]]; do
        sleep 1
        ((attempts++))
    done

    if is_running "$service"; then
        echo -e "${GREEN}✓ $service is running${NC}"
    else
        echo -e "${RED}✗ $service failed to start${NC}"
        return 1
    fi
}

echo "Rolling restart of gorp containers"
echo "Compose file: $COMPOSE_FILE"
echo "Delay between restarts: ${DELAY}s"
echo ""

services=$(get_services)

if [[ -z "$services" ]]; then
    echo -e "${RED}No gorp services found in $COMPOSE_FILE${NC}"
    exit 1
fi

service_count=$(echo "$services" | wc -l | tr -d ' ')
echo "Found $service_count services to restart"
echo ""

current=0
for service in $services; do
    ((current++))
    echo -e "${YELLOW}[$current/$service_count]${NC} $service"

    restart_service "$service"

    # Wait before next restart (except for last one)
    if [[ $current -lt $service_count ]]; then
        echo "  Waiting ${DELAY}s before next restart..."
        if ! $DRY_RUN; then
            sleep "$DELAY"
        fi
    fi
    echo ""
done

echo -e "${GREEN}Rolling restart complete!${NC}"
