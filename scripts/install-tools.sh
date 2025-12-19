#!/bin/bash
# ABOUTME: Installs MCP and CLI tools into /usr/local/bin
# ABOUTME: Reads tool definitions from tools.yaml

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TOOLS_FILE="${TOOLS_FILE:-$SCRIPT_DIR/tools.yaml}"
INSTALL_DIR="${INSTALL_DIR:-/usr/local/bin}"
TEMP_DIR="${TEMP_DIR:-/tmp/tool-install}"

# Check for yq
if ! command -v yq &> /dev/null; then
    echo "Installing yq..."
    curl -sL "https://github.com/mikefarah/yq/releases/download/v4.44.1/yq_linux_amd64" -o /tmp/yq
    chmod +x /tmp/yq
    mv /tmp/yq /usr/local/bin/yq
fi

mkdir -p "$INSTALL_DIR"
mkdir -p "$TEMP_DIR"

get_latest_version() {
    local repo="$1"
    curl -sL "https://api.github.com/repos/$repo/releases/latest" | yq -r '.tag_name'
}

install_tool() {
    local name="$1"
    local repo="$2"
    local version="$3"
    local url_pattern="$4"
    local binary_path="$5"

    # Fetch latest version from GitHub if version is "latest"
    if [ "$version" = "latest" ]; then
        version=$(get_latest_version "$repo")
        echo "   Resolved latest version: $version"
    fi

    # Strip 'v' prefix for version_num
    local version_num="${version#v}"

    # Build URL with substitutions
    local url="$url_pattern"
    url="${url//\{repo\}/$repo}"
    url="${url//\{version\}/$version}"
    url="${url//\{version_num\}/$version_num}"

    # Substitute version_num in binary_path too
    binary_path="${binary_path//\{version_num\}/$version_num}"

    echo "📦 Installing $name $version..."
    echo "   URL: $url"

    # Download and extract
    rm -rf "$TEMP_DIR/$name"
    mkdir -p "$TEMP_DIR/$name"

    if ! curl -fsSL "$url" | tar -xz -C "$TEMP_DIR/$name" 2>/dev/null; then
        echo "   ⚠️  Failed to download/extract $name"
        return 1
    fi

    # Find and move binary
    local src="$TEMP_DIR/$name/$binary_path"

    if [ ! -f "$src" ]; then
        # Try finding the binary by name
        src=$(find "$TEMP_DIR/$name" -name "$name" -type f -executable 2>/dev/null | head -1)
        if [ -z "$src" ]; then
            src=$(find "$TEMP_DIR/$name" -type f -executable 2>/dev/null | head -1)
        fi
    fi

    if [ -f "$src" ]; then
        mv "$src" "$INSTALL_DIR/$name"
        chmod +x "$INSTALL_DIR/$name"
        echo "   ✅ Installed to $INSTALL_DIR/$name"
    else
        echo "   ❌ Binary not found in archive"
        echo "   Contents: $(ls -la "$TEMP_DIR/$name")"
        return 1
    fi
}

echo "=== Installing Tools ==="
echo "Tools file: $TOOLS_FILE"
echo "Install dir: $INSTALL_DIR"
echo ""

if [ ! -f "$TOOLS_FILE" ]; then
    echo "❌ Tools file not found: $TOOLS_FILE"
    exit 1
fi

# Count tools
tool_count=$(yq '. | length' "$TOOLS_FILE")
echo "Found $tool_count tools to install"
echo ""

failed=0
for i in $(seq 0 $((tool_count - 1))); do
    name=$(yq ".[$i].name" "$TOOLS_FILE")
    repo=$(yq ".[$i].repo" "$TOOLS_FILE")
    version=$(yq ".[$i].version" "$TOOLS_FILE")
    url=$(yq ".[$i].url" "$TOOLS_FILE")
    binary=$(yq ".[$i].binary" "$TOOLS_FILE")

    if ! install_tool "$name" "$repo" "$version" "$url" "$binary"; then
        ((failed++)) || true
    fi
done

# Cleanup
rm -rf "$TEMP_DIR"

echo ""
if [ $failed -eq 0 ]; then
    echo "✅ All tools installed successfully!"
else
    echo "⚠️  $failed tool(s) failed to install"
    exit 1
fi
