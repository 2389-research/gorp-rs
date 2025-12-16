#!/bin/bash
# ABOUTME: Checks GitHub for new releases of tools defined in install-tools.sh
# ABOUTME: Compares current versions against latest releases and reports updates

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Tool definitions extracted from install-tools.sh
# Format: name|repo|version
TOOLS=(
    "chronicle|harperreed/chronicle|v1.1.4"
    "memory|harperreed/memory|v0.3.4"
    "toki|harperreed/toki|v0.3.6"
    "pagen|harperreed/pagen|v0.4.4"
    "gsuite-mcp|2389-research/gsuite-mcp|v1.2.1"
    "digest|harperreed/digest|v0.6.0"
    "memo|harperreed/memo|v0.2.0"
    "pop|charmbracelet/pop|v0.2.0"
    "push|harperreed/push-cli|v0.0.2"
    "position|harperreed/position|v0.5.0"
    "sweet|harperreed/sweet|v0.2.5"
)

# Check if gh CLI is available
use_gh=false
if command -v gh &> /dev/null; then
    use_gh=true
fi

get_latest_release() {
    local repo="$1"
    local latest=""

    if $use_gh; then
        # Use gh CLI (handles auth automatically)
        latest=$(gh release view --repo "$repo" --json tagName -q '.tagName' 2>/dev/null || echo "")
    else
        # Fallback to curl with GitHub API
        latest=$(curl -s "https://api.github.com/repos/$repo/releases/latest" | grep -o '"tag_name": *"[^"]*"' | head -1 | cut -d'"' -f4)
    fi

    echo "$latest"
}

# Compare semantic versions (returns 0 if v1 < v2, 1 if v1 >= v2)
version_lt() {
    local v1="${1#v}"
    local v2="${2#v}"

    # Simple numeric comparison for semver
    if [[ "$v1" == "$v2" ]]; then
        return 1
    fi

    # Use sort -V for version comparison
    local smaller=$(printf '%s\n%s' "$v1" "$v2" | sort -V | head -1)
    if [[ "$smaller" == "$v1" ]]; then
        return 0
    else
        return 1
    fi
}

echo "🔍 Checking for tool updates..."
echo ""

updates_available=0
errors=0

for tool in "${TOOLS[@]}"; do
    IFS='|' read -r name repo current_version <<< "$tool"

    printf "%-15s %-30s " "$name" "$repo"

    latest=$(get_latest_release "$repo")

    if [[ -z "$latest" ]]; then
        echo -e "${RED}✗ Failed to fetch${NC}"
        ((errors++)) || true
        continue
    fi

    if [[ "$latest" == "$current_version" ]]; then
        echo -e "${GREEN}✓ $current_version (up to date)${NC}"
    elif version_lt "$current_version" "$latest"; then
        echo -e "${YELLOW}⬆ $current_version → $latest${NC}"
        ((updates_available++)) || true
    else
        echo -e "${BLUE}? $current_version (latest: $latest)${NC}"
    fi
done

echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

if [[ $updates_available -gt 0 ]]; then
    echo -e "${YELLOW}📦 $updates_available update(s) available${NC}"
    echo ""
    echo "To update, edit scripts/install-tools.sh and change the version numbers,"
    echo "then rebuild the Docker image with: docker build --no-cache -t gorp:latest ."
else
    echo -e "${GREEN}✅ All tools are up to date!${NC}"
fi

if [[ $errors -gt 0 ]]; then
    echo -e "${RED}⚠️  $errors tool(s) failed to check${NC}"
fi

exit 0
