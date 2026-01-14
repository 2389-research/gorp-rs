# ABOUTME: Homebrew cask formula for gorp desktop app
# ABOUTME: Install via: brew install --cask 2389-research/tap/gorp

cask "gorp" do
  version "0.3.1"
  sha256 "PLACEHOLDER_SHA256"

  url "https://github.com/2389-research/gorp-rs/releases/download/v#{version}/gorp-#{version}-macos.dmg"
  name "gorp"
  desc "Personal AI agent desktop for Matrix-Claude bridge"
  homepage "https://github.com/2389-research/gorp-rs"

  livecheck do
    url :url
    strategy :github_latest
  end

  depends_on macos: ">= :big_sur"

  app "gorp.app"

  # Also install CLI binary to PATH
  binary "#{appdir}/gorp.app/Contents/MacOS/gorp"

  zap trash: [
    "~/.config/gorp",
    "~/.local/share/gorp",
    "~/Library/Application Support/gorp",
    "~/Library/Caches/gorp",
    "~/Library/Preferences/com.2389.gorp.plist",
  ]

  caveats <<~EOS
    gorp runs as a menu bar app by default.

    To configure, create a config file:
      mkdir -p ~/.config/gorp
      cp /Applications/gorp.app/Contents/Resources/config.toml.example ~/.config/gorp/config.toml

    Or set environment variables:
      export MATRIX_HOME_SERVER="https://matrix.org"
      export MATRIX_USER_ID="@yourbot:matrix.org"
      export MATRIX_PASSWORD="your-password"
      export ALLOWED_USERS="@you:matrix.org"

    To start at login, enable in System Settings > General > Login Items
    or run: gorp --headless (for daemon mode)
  EOS
end
