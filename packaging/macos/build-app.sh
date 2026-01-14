#!/bin/bash
# ABOUTME: Build macOS .app bundle for gorp
# ABOUTME: Creates gorp.app with proper structure for distribution

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
BUILD_DIR="$PROJECT_ROOT/target/release"
APP_NAME="gorp"
APP_BUNDLE="$BUILD_DIR/$APP_NAME.app"

# Get version from Cargo.toml
VERSION=$(grep '^version' "$PROJECT_ROOT/Cargo.toml" | head -1 | sed 's/.*"\(.*\)".*/\1/')

echo "Building gorp v$VERSION for macOS..."

# Build release binary
echo "Building release binary..."
cd "$PROJECT_ROOT"
cargo build --release

# Check if binary exists
if [[ ! -f "$BUILD_DIR/gorp" ]]; then
    echo "Error: Binary not found at $BUILD_DIR/gorp"
    exit 1
fi

# Remove old bundle if exists
rm -rf "$APP_BUNDLE"

# Create bundle structure
echo "Creating app bundle structure..."
mkdir -p "$APP_BUNDLE/Contents/MacOS"
mkdir -p "$APP_BUNDLE/Contents/Resources"

# Copy binary
cp "$BUILD_DIR/gorp" "$APP_BUNDLE/Contents/MacOS/gorp"

# Copy and update Info.plist
sed "s/VERSION_PLACEHOLDER/$VERSION/g" "$SCRIPT_DIR/Info.plist" > "$APP_BUNDLE/Contents/Info.plist"

# Copy icon if exists
if [[ -f "$SCRIPT_DIR/gorp.icns" ]]; then
    cp "$SCRIPT_DIR/gorp.icns" "$APP_BUNDLE/Contents/Resources/"
else
    echo "Warning: No icon file found at $SCRIPT_DIR/gorp.icns"
fi

# Create PkgInfo
echo -n "APPL????" > "$APP_BUNDLE/Contents/PkgInfo"

# Strip binary to reduce size (optional)
if command -v strip &> /dev/null; then
    echo "Stripping binary..."
    strip "$APP_BUNDLE/Contents/MacOS/gorp" 2>/dev/null || true
fi

# Get bundle size
BUNDLE_SIZE=$(du -sh "$APP_BUNDLE" | cut -f1)

echo ""
echo "App bundle created successfully!"
echo "  Location: $APP_BUNDLE"
echo "  Version:  $VERSION"
echo "  Size:     $BUNDLE_SIZE"
echo ""
echo "To test: open $APP_BUNDLE"
echo "To sign: codesign --deep --sign \"Developer ID Application: YOUR_NAME\" $APP_BUNDLE"
