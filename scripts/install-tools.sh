#!/bin/bash
# ABOUTME: Installs MCP and CLI tools into /usr/local/bin
# ABOUTME: Called during Docker build to install all required tools

set -e

INSTALL_DIR="${INSTALL_DIR:-/usr/local/bin}"
TEMP_DIR="${TEMP_DIR:-/tmp/tool-install}"

mkdir -p "$INSTALL_DIR"
mkdir -p "$TEMP_DIR"
cd "$TEMP_DIR"

# Tool definitions: name|repo|version|url_pattern|binary_in_archive
# url_pattern uses {version} and {version_num} placeholders
# binary_in_archive supports {version_num} placeholder for versioned paths
TOOLS=(
    # MCP servers
    "chronicle|harperreed/chronicle|v1.1.4|https://github.com/{repo}/releases/download/{version}/chronicle-linux-amd64.tar.gz|chronicle-linux-amd64"
    "memory|harperreed/memory|v0.3.4|https://github.com/{repo}/releases/download/{version}/memory_{version}_Linux_x86_64.tar.gz|memory-linux-amd64"
    "toki|harperreed/toki|v0.3.6|https://github.com/{repo}/releases/download/{version}/toki_{version_num}_Linux_x86_64.tar.gz|toki_{version_num}_Linux_x86_64/toki"
    "pagen|harperreed/pagen|v0.4.4|https://github.com/{repo}/releases/download/{version}/pagen_{version}_linux_amd64.tar.gz|pagen"
    "gsuite-mcp|2389-research/gsuite-mcp|v1.1.0|https://github.com/{repo}/releases/download/{version}/gsuite-mcp_{version_num}_linux_amd64.tar.gz|gsuite-mcp"
    "digest|harperreed/digest|v0.6.0|https://github.com/{repo}/releases/download/{version}/digest_{version_num}_Linux_x86_64.tar.gz|digest"
    "memo|harperreed/memo|v0.2.0|https://github.com/{repo}/releases/download/{version}/memo_{version_num}_Linux_x86_64.tar.gz|memo_{version_num}_Linux_x86_64/memo"
    # CLI tools
    "pop|charmbracelet/pop|v0.2.0|https://github.com/{repo}/releases/download/{version}/pop_{version_num}_Linux_x86_64.tar.gz|pop"
    "push|harperreed/push-cli|v0.0.2|https://github.com/{repo}/releases/download/{version}/push_{version_num}_Linux_x86_64.tar.gz|push_{version_num}_Linux_x86_64/push"
    "position|harperreed/position|v0.3.0|https://github.com/{repo}/releases/download/{version}/position_{version_num}_Linux_x86_64.tar.gz|position_{version_num}_Linux_x86_64/position"
)

install_tool() {
    local spec="$1"
    IFS='|' read -r name repo version url_pattern binary_path <<< "$spec"

    # Strip 'v' prefix for version_num
    local version_num="${version#v}"

    # Build URL with substitutions
    local url="$url_pattern"
    url="${url//\{repo\}/$repo}"
    url="${url//\{version\}/$version}"
    url="${url//\{version_num\}/$version_num}"

    # Substitute version_num in binary_path too (for versioned subdirectories)
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
echo "Install dir: $INSTALL_DIR"
echo ""

failed=0
for tool in "${TOOLS[@]}"; do
    if ! install_tool "$tool"; then
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
