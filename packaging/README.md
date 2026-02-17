# gorp Packaging

Build and distribution scripts for gorp desktop app.

## macOS

### Quick Build

```bash
# Build .app bundle
./packaging/macos/build-app.sh

# Build DMG installer
./packaging/macos/build-dmg.sh
```

Outputs:
- `target/release/gorp.app` - Application bundle
- `target/release/gorp-X.Y.Z-macos.dmg` - Disk image installer

### Files

- `Info.plist` - App bundle metadata
- `build-app.sh` - Creates .app bundle from release binary
- `build-dmg.sh` - Creates DMG with Applications symlink
- `generate-icon.sh` - Creates placeholder icon (needs Python/Pillow)
- `com.2389.gorp.plist` - LaunchAgent for auto-start at login

### Code Signing & Notarization

For distribution outside the App Store:

```bash
# Sign the app bundle
codesign --deep --sign "Developer ID Application: YOUR_NAME" target/release/gorp.app

# Notarize the DMG
xcrun notarytool submit target/release/gorp-X.Y.Z-macos.dmg \
    --apple-id YOUR_APPLE_ID \
    --team-id YOUR_TEAM_ID \
    --password YOUR_APP_SPECIFIC_PASSWORD \
    --wait

# Staple the notarization ticket
xcrun stapler staple target/release/gorp-X.Y.Z-macos.dmg
```

### Launch at Login

Copy the LaunchAgent plist:

```bash
cp packaging/macos/com.2389.gorp.plist ~/Library/LaunchAgents/
launchctl load ~/Library/LaunchAgents/com.2389.gorp.plist
```

## Homebrew

Formula at `packaging/homebrew/gorp.rb`. To use:

1. Create a tap repository (e.g., `2389-research/homebrew-tap`)
2. Copy `gorp.rb` to the tap
3. Update the SHA256 hash after building the DMG:
   ```bash
   shasum -a 256 target/release/gorp-X.Y.Z-macos.dmg
   ```
4. Users can then install with:
   ```bash
   brew tap 2389-research/tap
   brew install --cask gorp
   ```

## Icon

To generate a proper icon:

1. Create `packaging/macos/icon-source.png` (1024x1024 PNG)
2. Run `./packaging/macos/generate-icon.sh`

The script creates `gorp.icns` with all required sizes.
