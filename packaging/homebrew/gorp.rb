# ABOUTME: Homebrew formula for gorp CLI (builds from source with cargo)
# ABOUTME: Install via: brew install 2389-research/tap/gorp

class Gorp < Formula
  desc "Multi-platform Claude bridge - connect Claude to Matrix, Telegram, Slack"
  homepage "https://github.com/2389-research/gorp-rs"
  url "https://github.com/2389-research/gorp-rs/archive/refs/tags/v0.3.2.tar.gz"
  sha256 "PLACEHOLDER_SHA256"
  license "MIT"
  head "https://github.com/2389-research/gorp-rs.git", branch: "main"

  livecheck do
    url :stable
    strategy :github_latest
  end

  depends_on "rust" => :build
  depends_on "protobuf" => :build
  depends_on "pkg-config" => :build
  depends_on "openssl@3"

  # Node.js for ACP backend (Claude Code CLI)
  depends_on "node" => :recommended

  def install
    # Build with server-appropriate features (no GUI on headless install)
    features = %w[matrix telegram slack admin coven]
    system "cargo", "build", "--release",
           "--no-default-features",
           "--features", features.join(",")

    bin.install "target/release/gorp"

    # Install example config
    (share/"gorp").install "config.toml.example"
  end

  def post_install
    # Create config directory
    (var/"lib/gorp").mkpath
  end

  def caveats
    <<~EOS
      To get started, copy the example config:
        mkdir -p ~/.config/gorp
        cp #{share}/gorp/config.toml.example ~/.config/gorp/config.toml

      Edit the config with your platform credentials, then run:
        gorp start

      Available features (compiled into this build):
        Matrix, Telegram, Slack, Admin panel, Coven gateway

      For headless server deployment, see:
        https://github.com/2389-research/gorp-rs
    EOS
  end

  test do
    assert_match "gorp", shell_output("#{bin}/gorp --version")
    # config check fails without a config file (exit 1), verify it runs
    output = shell_output("#{bin}/gorp config check 2>&1", 1)
    assert_match(/Invalid|Error|parse/, output)
  end
end
