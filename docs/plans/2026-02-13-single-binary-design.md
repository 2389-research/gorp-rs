# Single Binary Build & Packaging — Design Document

**Date:** 2026-02-13
**Status:** Draft
**Depends on:** All platform, interface, and provider design documents

## Summary

Compile all gorp components — four messaging platforms, three interfaces, and the coven-gateway provider — into a single Rust binary with feature flags. Every component is opt-in at compile time. The WhatsApp/Baileys Node.js sidecar source is embedded in the binary via `rust-embed` and extracted at runtime (requires system Node.js).

## Feature Flags

```toml
[features]
default = ["matrix", "admin"]

# Platforms (inbound)
matrix = ["dep:matrix-sdk"]
telegram = ["dep:teloxide"]
slack = ["dep:slack-morphism"]
whatsapp = ["dep:rust-embed"]

# Interfaces
gui = ["dep:iced", "dep:tray-icon", "dep:global-hotkey"]
tui = ["dep:ratatui", "dep:crossterm"]
admin = ["dep:askama", "dep:tower-sessions", "dep:argon2"]

# Providers (outbound)
coven = ["dep:tonic", "dep:prost"]

# Meta
all = ["matrix", "telegram", "slack", "whatsapp", "gui", "tui", "admin", "coven"]
```

Every module is gated behind `#[cfg(feature = "...")]`. At least one platform or the coven provider must be enabled, or the binary prints a helpful error at startup.

## Subcommands

All subcommands live in one binary. Unavailable features print a message pointing to the right compile flag:

```
gorp start              # Headless daemon (always available)
gorp tui                # Terminal UI        (requires tui feature)
gorp gui                # Desktop GUI        (requires gui feature)
gorp config             # Configuration      (always available)
gorp schedule           # Schedule management (always available)
```

```rust
// src/main.rs
match cli.command {
    Commands::Start => run_headless(config).await,

    #[cfg(feature = "tui")]
    Commands::Tui => gorp::tui::run_tui(config).await,
    #[cfg(not(feature = "tui"))]
    Commands::Tui => {
        eprintln!("TUI not available. Rebuild with: cargo build --features tui");
        std::process::exit(1);
    }

    #[cfg(feature = "gui")]
    Commands::Gui => gorp::gui::run_gui(config).await,
    #[cfg(not(feature = "gui"))]
    Commands::Gui => {
        eprintln!("GUI not available. Rebuild with: cargo build --features gui");
        std::process::exit(1);
    }

    Commands::Config => config_management().await,
    Commands::Schedule => schedule_management().await,
}
```

## Platform Initialization

`main.rs` conditionally initializes platforms based on both feature flags AND config presence:

```rust
let mut registry = PlatformRegistry::new();

#[cfg(feature = "matrix")]
if let Some(ref matrix_cfg) = config.matrix {
    registry.register(Box::new(MatrixPlatform::new(matrix_cfg).await?));
}

#[cfg(feature = "telegram")]
if let Some(ref tg_cfg) = config.telegram {
    registry.register(Box::new(TelegramPlatform::new(tg_cfg).await?));
}

#[cfg(feature = "slack")]
if let Some(ref slack_cfg) = config.slack {
    registry.register(Box::new(SlackPlatform::new(slack_cfg).await?));
}

#[cfg(feature = "whatsapp")]
if let Some(ref wa_cfg) = config.whatsapp {
    let sidecar = WhatsAppSidecar::extract_and_spawn(wa_cfg).await?;
    registry.register(Box::new(WhatsAppPlatform::new(wa_cfg, sidecar).await?));
}

#[cfg(feature = "coven")]
if let Some(ref coven_cfg) = config.coven {
    CovenProvider::start(coven_cfg, &server).await?;
}
```

A platform config being present when the feature is not compiled in produces a warning at startup:

```
WARN: [telegram] config found but telegram feature not compiled in. Rebuild with --features telegram
```

## WhatsApp Embedded Source

The Baileys Node.js sidecar source is embedded in the binary and extracted on first use:

```rust
#[cfg(feature = "whatsapp")]
#[derive(rust_embed::Embed)]
#[folder = "baileys-bridge/"]
struct BaileysBridge;
```

### Extraction Flow

```
First WhatsApp enable:
    │
    ├── Check data/baileys-bridge/package.json exists?
    │   ├── Yes → check version matches embedded version
    │   │   ├── Match → skip extraction
    │   │   └── Mismatch → re-extract (update)
    │   └── No → extract
    │
    ├── Extract all BaileysBridge files to data/baileys-bridge/
    │
    ├── Verify Node.js available:
    │   node --version
    │   ├── Found → continue
    │   └── Not found → error: "WhatsApp requires Node.js. Install from https://nodejs.org"
    │
    ├── Install dependencies:
    │   npm install --production (in data/baileys-bridge/)
    │
    └── Spawn sidecar from data/baileys-bridge/
```

### Version Tracking

A `.gorp-embed-version` file in the extracted directory tracks which binary version extracted it. On upgrade, if the version differs, the source is re-extracted and `npm install` runs again.

```rust
impl WhatsAppSidecar {
    async fn extract_and_spawn(config: &WhatsAppConfig) -> Result<Self> {
        let bridge_dir = PathBuf::from(&config.data_dir).join("baileys-bridge");
        let version_file = bridge_dir.join(".gorp-embed-version");
        let current_version = env!("CARGO_PKG_VERSION");

        let needs_extract = if version_file.exists() {
            fs::read_to_string(&version_file)? != current_version
        } else {
            true
        };

        if needs_extract {
            // Extract embedded files
            for file in BaileysBridge::iter() {
                let path = bridge_dir.join(file.as_ref());
                fs::create_dir_all(path.parent().unwrap())?;
                fs::write(&path, BaileysBridge::get(file.as_ref()).unwrap().data)?;
            }

            // Verify Node.js
            let node = config.node_binary.as_deref().unwrap_or("node");
            Command::new(node).arg("--version").output()
                .map_err(|_| anyhow!("WhatsApp requires Node.js. Install from https://nodejs.org"))?;

            // Install dependencies
            Command::new("npm")
                .args(["install", "--production"])
                .current_dir(&bridge_dir)
                .output()?;

            // Write version marker
            fs::write(&version_file, current_version)?;
        }

        // Spawn sidecar
        Self::spawn(&bridge_dir, config).await
    }
}
```

## Build Profiles

### Common Profiles

```bash
# Default — Matrix + web admin (what most people start with)
cargo build --release

# Full headless server (all platforms, no GUI)
cargo build --release --features matrix,telegram,slack,whatsapp,admin,coven

# Everything
cargo build --release --features all

# Minimal TUI-only with one platform
cargo build --release --features tui,matrix

# Server + coven provider (no user-facing platforms)
cargo build --release --features admin,coven

# Platforms only, no interfaces (headless, no web admin)
cargo build --release --no-default-features --features matrix,telegram
```

### Estimated Binary Sizes (release, stripped)

| Profile | Features | Approx Size |
|---|---|---|
| Default | matrix, admin | ~15-20MB |
| Headless server | matrix, telegram, slack, whatsapp, admin | ~25-35MB |
| Full headless + coven | matrix, telegram, slack, whatsapp, admin, coven | ~30-40MB |
| Everything | all | ~45-60MB |
| Minimal TUI | tui, matrix | ~15-20MB |

iced (GUI) is the heaviest dependency at ~15-20MB. Most server deployments won't include it.

## Cross-Compilation

### Target Matrix

| Target | Platforms | Interfaces | Notes |
|---|---|---|---|
| `x86_64-unknown-linux-gnu` | All | All | Primary server target |
| `aarch64-unknown-linux-gnu` | All | All | ARM servers (Raspberry Pi, AWS Graviton) |
| `x86_64-apple-darwin` | All | All | macOS Intel |
| `aarch64-apple-darwin` | All | All | macOS Apple Silicon |
| `x86_64-unknown-linux-musl` | All except GUI | TUI, admin | Fully static binary (Alpine, scratch containers) |

### Platform-Specific Constraints

| Feature | Constraint |
|---|---|
| `gui` (iced) | Requires windowing system libs (X11/Wayland on Linux, Cocoa on macOS). Not available on musl/Alpine. |
| `whatsapp` | Requires Node.js at runtime (not at build time). |
| `coven` | Requires protobuf compiler (`protoc`) at build time for tonic-build. |
| `matrix` | matrix-sdk requires OpenSSL or rustls. Default to rustls for easier cross-compilation. |

## Docker

### Multi-Feature Dockerfile

```dockerfile
# Build stage
FROM rust:1.83-bookworm AS builder

# Install protoc for coven feature
RUN apt-get update && apt-get install -y protobuf-compiler && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY . .

ARG FEATURES="matrix,telegram,slack,whatsapp,admin,coven"
RUN cargo build --release --features "$FEATURES"

# Runtime stage
FROM debian:bookworm-slim

# Install Node.js for WhatsApp sidecar (only if whatsapp feature enabled)
ARG INSTALL_NODE=true
RUN if [ "$INSTALL_NODE" = "true" ]; then \
      apt-get update && apt-get install -y nodejs npm && rm -rf /var/lib/apt/lists/*; \
    fi

RUN useradd --create-home --shell /bin/bash gorp
WORKDIR /home/gorp

COPY --from=builder /app/target/release/gorp /usr/local/bin/gorp

USER gorp
EXPOSE 13000

CMD ["gorp", "start"]
```

### Build Variants

```bash
# Full server (all platforms)
docker build --build-arg FEATURES="matrix,telegram,slack,whatsapp,admin,coven" -t gorp:full .

# Minimal (matrix only, no WhatsApp/Node.js)
docker build --build-arg FEATURES="matrix,admin" --build-arg INSTALL_NODE=false -t gorp:minimal .

# Coven agent only (no platforms)
docker build --build-arg FEATURES="admin,coven" --build-arg INSTALL_NODE=false -t gorp:coven .
```

## Homebrew

Update the existing formula to support feature selection:

```ruby
# packaging/homebrew/gorp.rb
class Gorp < Formula
  desc "Matrix-to-Claude bridge with pluggable agent backends"
  homepage "https://github.com/2389/gorp-rs"

  depends_on "rust" => :build
  depends_on "protobuf" => :build    # For coven feature

  # Optional runtime dep
  depends_on "node" => :optional     # For WhatsApp

  def install
    features = ["matrix", "telegram", "slack", "admin", "tui"]
    features << "whatsapp" if build.with?("node")
    features << "coven" if build.with?("protobuf")

    system "cargo", "build", "--release", "--features", features.join(",")
    bin.install "target/release/gorp"
  end
end
```

## CI/CD Build Matrix

```yaml
# .github/workflows/release.yml
jobs:
  build:
    strategy:
      matrix:
        include:
          - target: x86_64-unknown-linux-gnu
            features: "matrix,telegram,slack,whatsapp,admin,coven,tui"
            os: ubuntu-latest
          - target: aarch64-unknown-linux-gnu
            features: "matrix,telegram,slack,whatsapp,admin,coven,tui"
            os: ubuntu-latest
          - target: x86_64-apple-darwin
            features: "all"
            os: macos-latest
          - target: aarch64-apple-darwin
            features: "all"
            os: macos-latest

    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          targets: ${{ matrix.target }}
      - name: Install protoc
        uses: arduino/setup-protoc@v3
      - name: Build
        run: cargo build --release --target ${{ matrix.target }} --features "${{ matrix.features }}"
      - name: Strip binary
        run: strip target/${{ matrix.target }}/release/gorp
      - uses: actions/upload-artifact@v4
        with:
          name: gorp-${{ matrix.target }}
          path: target/${{ matrix.target }}/release/gorp
```

## Compile-Time Validation

`build.rs` validates feature combinations and provides helpful errors:

```rust
fn main() {
    // Warn if no platform or provider is enabled
    let has_platform = cfg!(feature = "matrix")
        || cfg!(feature = "telegram")
        || cfg!(feature = "slack")
        || cfg!(feature = "whatsapp");
    let has_provider = cfg!(feature = "coven");

    if !has_platform && !has_provider {
        println!("cargo:warning=No platforms or providers enabled. Enable at least one: matrix, telegram, slack, whatsapp, or coven");
    }

    // tonic-build for coven
    #[cfg(feature = "coven")]
    {
        tonic_build::compile_protos("proto/coven.proto")
            .expect("Failed to compile coven.proto. Is protoc installed?");
    }
}
```

## Summary

| Component | Compile-time | Runtime Dep | Feature Flag |
|---|---|---|---|
| Matrix | Pure Rust | None | `matrix` |
| Telegram | Pure Rust | None | `telegram` |
| Slack | Pure Rust | None | `slack` |
| WhatsApp | Embedded JS source | Node.js | `whatsapp` |
| GUI | Pure Rust | Windowing system | `gui` |
| TUI | Pure Rust | Terminal | `tui` |
| Web Admin | Pure Rust (templates compiled in) | None | `admin` |
| Coven | Pure Rust (protobuf generated) | None | `coven` |

One binary. One `cargo build`. Feature flags control what's included. WhatsApp is the only component with a runtime dependency (Node.js), and even its source code is embedded in the binary.
