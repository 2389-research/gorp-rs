#!/bin/bash
# ABOUTME: Build DMG installer for gorp
# ABOUTME: Creates a distributable disk image with Applications symlink

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
BUILD_DIR="$PROJECT_ROOT/target/release"
APP_NAME="gorp"
APP_BUNDLE="$BUILD_DIR/$APP_NAME.app"

# Get version from Cargo.toml
VERSION=$(grep '^version' "$PROJECT_ROOT/Cargo.toml" | head -1 | sed 's/.*"\(.*\)".*/\1/')

DMG_NAME="gorp-$VERSION-macos"
DMG_PATH="$BUILD_DIR/$DMG_NAME.dmg"
STAGING_DIR="$BUILD_DIR/dmg-staging"

echo "Building DMG for gorp v$VERSION..."

# Check if app bundle exists
if [[ ! -d "$APP_BUNDLE" ]]; then
    echo "Error: App bundle not found at $APP_BUNDLE"
    echo "Run build-app.sh first"
    exit 1
fi

# Clean up old staging/dmg
rm -rf "$STAGING_DIR"
rm -f "$DMG_PATH"

# Create staging directory
echo "Setting up DMG staging..."
mkdir -p "$STAGING_DIR"

# Copy app bundle
cp -R "$APP_BUNDLE" "$STAGING_DIR/"

# Create Applications symlink
ln -s /Applications "$STAGING_DIR/Applications"

# Create DMG
echo "Creating DMG..."
hdiutil create \
    -volname "gorp $VERSION" \
    -srcfolder "$STAGING_DIR" \
    -ov \
    -format UDZO \
    "$DMG_PATH"

# Clean up staging
rm -rf "$STAGING_DIR"

# Get DMG size
DMG_SIZE=$(du -sh "$DMG_PATH" | cut -f1)

echo ""
echo "DMG created successfully!"
echo "  Location: $DMG_PATH"
echo "  Size:     $DMG_SIZE"
echo ""
echo "To notarize:"
echo "  xcrun notarytool submit $DMG_PATH --apple-id YOUR_APPLE_ID --team-id YOUR_TEAM_ID --password YOUR_APP_SPECIFIC_PASSWORD --wait"
echo ""
echo "To staple after notarization:"
echo "  xcrun stapler staple $DMG_PATH"
