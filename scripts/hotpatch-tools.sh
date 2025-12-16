#!/bin/bash
# ABOUTME: Hot-patches tools in running containers for testing without rebuild
# ABOUTME: Usage: ./hotpatch-tools.sh [container] [tool] or ./hotpatch-tools.sh --all

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

# Tool definitions: name|repo|url_pattern|binary_in_archive
# Mirrors install-tools.sh but fetches latest from GitHub
TOOLS=(
    "chronicle|harperreed/chronicle|https://github.com/{repo}/releases/download/{version}/chronicle-linux-amd64.tar.gz|chronicle-linux-amd64"
    "memory|harperreed/memory|https://github.com/{repo}/releases/download/{version}/memory_{version}_Linux_x86_64.tar.gz|memory-linux-amd64"
    "toki|harperreed/toki|https://github.com/{repo}/releases/download/{version}/toki_{version_num}_Linux_x86_64.tar.gz|toki"
    "pagen|harperreed/pagen|https://github.com/{repo}/releases/download/{version}/pagen_{version}_linux_amd64.tar.gz|pagen"
    "gsuite-mcp|2389-research/gsuite-mcp|https://github.com/{repo}/releases/download/{version}/gsuite-mcp_{version_num}_linux_amd64.tar.gz|gsuite-mcp"
    "digest|harperreed/digest|https://github.com/{repo}/releases/download/{version}/digest_{version_num}_Linux_x86_64.tar.gz|digest"
    "memo|harperreed/memo|https://github.com/{repo}/releases/download/{version}/memo_{version_num}_Linux_x86_64.tar.gz|memo"
    "pop|charmbracelet/pop|https://github.com/{repo}/releases/download/{version}/pop_{version_num}_Linux_x86_64.tar.gz|pop"
    "push|harperreed/push-cli|https://github.com/{repo}/releases/download/{version}/push_{version_num}_Linux_x86_64.tar.gz|push"
    "position|harperreed/position|https://github.com/{repo}/releases/download/{version}/position_{version_num}_Linux_x86_64.tar.gz|position"
    "sweet|harperreed/sweet|https://github.com/{repo}/releases/download/{version}/sweet_{version_num}_Linux_x86_64.tar.gz|sweet"
    "bbs|harperreed/bbs-mcp|https://github.com/{repo}/releases/download/{version}/bbs_{version_num}_Linux_x86_64.tar.gz|bbs"
)

get_latest_version() {
    local repo="$1"
    curl -s "https://api.github.com/repos/$repo/releases/latest" | grep -o '"tag_name": *"[^"]*"' | head -1 | cut -d'"' -f4
}

get_tool_spec() {
    local tool_name="$1"
    for spec in "${TOOLS[@]}"; do
        IFS='|' read -r name repo url_pattern binary_path <<< "$spec"
        if [[ "$name" == "$tool_name" ]]; then
            echo "$spec"
            return 0
        fi
    done
    return 1
}

patch_tool_in_container() {
    local container="$1"
    local tool_name="$2"
    local version="$3"  # optional, uses latest if empty

    local spec=$(get_tool_spec "$tool_name")
    if [[ -z "$spec" ]]; then
        echo -e "${RED}Unknown tool: $tool_name${NC}"
        return 1
    fi

    IFS='|' read -r name repo url_pattern binary_path <<< "$spec"

    # Get version
    if [[ -z "$version" ]]; then
        version=$(get_latest_version "$repo")
        if [[ -z "$version" ]]; then
            echo -e "${RED}Failed to get latest version for $tool_name${NC}"
            return 1
        fi
    fi

    local version_num="${version#v}"

    # Build URL
    local url="$url_pattern"
    url="${url//\{repo\}/$repo}"
    url="${url//\{version\}/$version}"
    url="${url//\{version_num\}/$version_num}"

    echo -e "${YELLOW}Patching $tool_name to $version in $container${NC}"
    echo "  URL: $url"

    # Download and extract in container
    docker exec "$container" bash -c "
        cd /tmp && \
        rm -rf patch-$name && \
        mkdir -p patch-$name && \
        curl -fsSL '$url' | tar -xz -C patch-$name && \
        find patch-$name -type f -executable -name '$name*' | head -1 | xargs -I{} cp {} /usr/local/bin/$name && \
        chmod +x /usr/local/bin/$name && \
        rm -rf patch-$name && \
        echo 'Installed:' && /usr/local/bin/$name --version 2>/dev/null || /usr/local/bin/$name version 2>/dev/null || echo '(version check not supported)'
    "

    if [[ $? -eq 0 ]]; then
        echo -e "${GREEN}✓ $tool_name patched to $version${NC}"
    else
        echo -e "${RED}✗ Failed to patch $tool_name${NC}"
        return 1
    fi
}

list_containers() {
    docker ps --format '{{.Names}}' | grep -E '^gorp-[0-9]+$' | sort -V
}

show_usage() {
    echo "Usage: $0 [OPTIONS] [CONTAINER] [TOOL] [VERSION]"
    echo ""
    echo "Hot-patch tools in running gorp containers for testing."
    echo ""
    echo "Options:"
    echo "  --all              Patch all tools to latest in all containers"
    echo "  --list             List available tools"
    echo "  --check            Check for updates (like check-tool-updates.sh)"
    echo ""
    echo "Examples:"
    echo "  $0 gorp-1 gsuite-mcp           # Patch gsuite-mcp to latest in gorp-1"
    echo "  $0 gorp-1 gsuite-mcp v1.2.3    # Patch gsuite-mcp to specific version"
    echo "  $0 --all gsuite-mcp            # Patch gsuite-mcp in all containers"
    echo "  $0 --all                       # Patch ALL tools to latest in all containers"
    echo ""
    echo "Available tools:"
    for spec in "${TOOLS[@]}"; do
        IFS='|' read -r name repo _ _ <<< "$spec"
        echo "  $name ($repo)"
    done
}

# Parse arguments
if [[ $# -eq 0 ]] || [[ "$1" == "-h" ]] || [[ "$1" == "--help" ]]; then
    show_usage
    exit 0
fi

if [[ "$1" == "--list" ]]; then
    echo "Available tools:"
    for spec in "${TOOLS[@]}"; do
        IFS='|' read -r name repo _ _ <<< "$spec"
        latest=$(get_latest_version "$repo")
        echo "  $name: $latest ($repo)"
    done
    exit 0
fi

if [[ "$1" == "--check" ]]; then
    exec "$SCRIPT_DIR/check-tool-updates.sh"
fi

if [[ "$1" == "--all" ]]; then
    shift
    containers=$(list_containers)

    if [[ -z "$containers" ]]; then
        echo -e "${RED}No gorp containers running${NC}"
        exit 1
    fi

    if [[ $# -eq 0 ]]; then
        # Patch all tools
        echo "Patching ALL tools to latest in all containers..."
        for container in $containers; do
            echo ""
            echo -e "${YELLOW}=== $container ===${NC}"
            for spec in "${TOOLS[@]}"; do
                IFS='|' read -r name _ _ _ <<< "$spec"
                patch_tool_in_container "$container" "$name" "" || true
            done
        done
    else
        # Patch specific tool in all containers
        tool_name="$1"
        version="${2:-}"
        for container in $containers; do
            patch_tool_in_container "$container" "$tool_name" "$version" || true
        done
    fi
else
    # Single container mode
    container="$1"
    tool_name="$2"
    version="${3:-}"

    if [[ -z "$tool_name" ]]; then
        echo -e "${RED}Missing tool name${NC}"
        show_usage
        exit 1
    fi

    patch_tool_in_container "$container" "$tool_name" "$version"
fi

echo ""
echo -e "${GREEN}Done! Remember: these patches are temporary and will be lost on container restart.${NC}"
echo "Once testing is complete, update scripts/install-tools.sh and rebuild the image."
