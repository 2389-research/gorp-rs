# gorp-core Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Create a standalone Rust library with UniFFI bindings that maison can consume for warm session management.

**Architecture:** Extract gorp-agent into gorp-core crate, add session management from gorp-rs/warm_session.rs, wrap with UniFFI for Swift bindings. The library provides SessionManager, SessionHandle, and AgentEvent types that Swift consumes via generated bindings.

**Tech Stack:** Rust, UniFFI, tokio (async), agent-client-protocol (ACP), Swift Package Manager

---

## Phase 1: Repository Setup

### Task 1: Create gorp-core repository

**Files:**
- Create: `/Users/harper/workspace/2389/gorp-core/`
- Create: `/Users/harper/workspace/2389/gorp-core/Cargo.toml`
- Create: `/Users/harper/workspace/2389/gorp-core/README.md`

**Step 1: Create directory and initialize git**

```bash
mkdir -p /Users/harper/workspace/2389/gorp-core
cd /Users/harper/workspace/2389/gorp-core
git init
```

**Step 2: Create workspace Cargo.toml**

```toml
[workspace]
members = ["gorp-core", "gorp-core-ffi"]
resolver = "2"

[workspace.package]
version = "0.1.0"
edition = "2021"
license = "MIT"

[workspace.dependencies]
tokio = { version = "1", features = ["sync", "rt", "rt-multi-thread", "macros", "process", "io-util", "time", "fs"] }
async-trait = "0.1"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tracing = "0.1"
anyhow = "1"
thiserror = "1"
futures = "0.3"
uuid = { version = "1", features = ["v4"] }
uniffi = "0.28"
```

**Step 3: Create README.md**

```markdown
# gorp-core

Shared Rust library for warm session management with Claude Code CLI.

## Usage (Rust)

```rust
use gorp_core::{SessionManager, WarmConfig, SessionConfig};

let manager = SessionManager::new(WarmConfig::default());
let session = manager.get_or_create(config).await?;
let events = session.prompt("Hello").await?;
```

## Usage (Swift)

```swift
import GorpCore

let manager = createSessionManager(config: WarmConfig(...))
let session = try await manager.getOrCreate(config: SessionConfig(...))
for try await event in session.prompt("Hello") {
    // handle event
}
```

## Building

```bash
cargo build --release
./scripts/build-xcframework.sh
```
```

**Step 4: Commit**

```bash
git add .
git commit -m "chore: initialize gorp-core workspace"
```

---

### Task 2: Create gorp-core crate structure

**Files:**
- Create: `gorp-core/Cargo.toml`
- Create: `gorp-core/src/lib.rs`

**Step 1: Create crate directory**

```bash
mkdir -p gorp-core/src
```

**Step 2: Create gorp-core/Cargo.toml**

```toml
[package]
name = "gorp-core"
version.workspace = true
edition.workspace = true
license.workspace = true
description = "Warm session management for Claude Code CLI"

[dependencies]
tokio = { workspace = true }
async-trait = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
tracing = { workspace = true }
anyhow = { workspace = true }
thiserror = { workspace = true }
futures = { workspace = true }
uuid = { workspace = true }
pin-project-lite = "0.2"
tokio-util = { version = "0.7", features = ["compat"], optional = true }
agent-client-protocol = { version = "0.9", optional = true }

[dev-dependencies]
tempfile = "3"
tokio = { workspace = true, features = ["test-util"] }
tracing-subscriber = { version = "0.3", features = ["env-filter"] }

[features]
default = ["acp"]
acp = ["dep:agent-client-protocol", "dep:tokio-util"]
```

**Step 3: Create gorp-core/src/lib.rs stub**

```rust
// ABOUTME: Core library for warm session management with Claude Code CLI.
// ABOUTME: Provides SessionManager, AgentHandle, and event types with UniFFI bindings.

pub mod event;
pub mod handle;
pub mod traits;
pub mod registry;
pub mod config;
pub mod backends;
pub mod session;

// Re-exports
pub use event::{AgentEvent, ErrorCode, Usage};
pub use handle::{AgentHandle, EventReceiver, SessionState};
pub use traits::AgentBackend;
pub use registry::{AgentRegistry, BackendFactory};
pub use config::{BackendConfig, Config};
pub use session::{SessionManager, SessionHandle, WarmConfig, SessionConfig};
```

**Step 4: Verify it compiles (will fail, that's expected)**

```bash
cargo check 2>&1 | head -5
# Expected: errors about missing modules
```

**Step 5: Commit**

```bash
git add .
git commit -m "chore: add gorp-core crate structure"
```

---

## Phase 2: Extract Core Types from gorp-agent

### Task 3: Extract event.rs

**Files:**
- Create: `gorp-core/src/event.rs`
- Reference: `/Users/harper/workspace/2389/gorp-rs/gorp-agent/src/event.rs`

**Step 1: Copy event.rs from gorp-agent**

```bash
cp /Users/harper/workspace/2389/gorp-rs/gorp-agent/src/event.rs gorp-core/src/event.rs
```

**Step 2: Verify no changes needed (file is self-contained)**

```bash
cargo check -p gorp-core 2>&1 | grep -E "^error" | head -5
# Should show errors about OTHER missing modules, not event.rs
```

**Step 3: Commit**

```bash
git add gorp-core/src/event.rs
git commit -m "feat: add AgentEvent types from gorp-agent"
```

---

### Task 4: Extract handle.rs

**Files:**
- Create: `gorp-core/src/handle.rs`
- Reference: `/Users/harper/workspace/2389/gorp-rs/gorp-agent/src/handle.rs`

**Step 1: Copy handle.rs from gorp-agent**

```bash
cp /Users/harper/workspace/2389/gorp-rs/gorp-agent/src/handle.rs gorp-core/src/handle.rs
```

**Step 2: Update import path**

The file imports `crate::AgentEvent` which should work since we re-export it.

**Step 3: Commit**

```bash
git add gorp-core/src/handle.rs
git commit -m "feat: add AgentHandle and EventReceiver from gorp-agent"
```

---

### Task 5: Extract traits.rs

**Files:**
- Create: `gorp-core/src/traits.rs`
- Reference: `/Users/harper/workspace/2389/gorp-rs/gorp-agent/src/traits.rs`

**Step 1: Copy traits.rs from gorp-agent**

```bash
cp /Users/harper/workspace/2389/gorp-rs/gorp-agent/src/traits.rs gorp-core/src/traits.rs
```

**Step 2: Commit**

```bash
git add gorp-core/src/traits.rs
git commit -m "feat: add AgentBackend trait from gorp-agent"
```

---

### Task 6: Extract config.rs

**Files:**
- Create: `gorp-core/src/config.rs`
- Reference: `/Users/harper/workspace/2389/gorp-rs/gorp-agent/src/config.rs`

**Step 1: Copy config.rs from gorp-agent**

```bash
cp /Users/harper/workspace/2389/gorp-rs/gorp-agent/src/config.rs gorp-core/src/config.rs
```

**Step 2: Commit**

```bash
git add gorp-core/src/config.rs
git commit -m "feat: add config types from gorp-agent"
```

---

### Task 7: Extract registry.rs

**Files:**
- Create: `gorp-core/src/registry.rs`
- Reference: `/Users/harper/workspace/2389/gorp-rs/gorp-agent/src/registry.rs`

**Step 1: Copy registry.rs from gorp-agent**

```bash
cp /Users/harper/workspace/2389/gorp-rs/gorp-agent/src/registry.rs gorp-core/src/registry.rs
```

**Step 2: Commit**

```bash
git add gorp-core/src/registry.rs
git commit -m "feat: add BackendRegistry from gorp-agent"
```

---

### Task 8: Extract backends

**Files:**
- Create: `gorp-core/src/backends/mod.rs`
- Create: `gorp-core/src/backends/mock.rs`
- Create: `gorp-core/src/backends/direct_cli.rs`
- Create: `gorp-core/src/backends/acp.rs`
- Reference: `/Users/harper/workspace/2389/gorp-rs/gorp-agent/src/backends/`

**Step 1: Create backends directory and copy files**

```bash
mkdir -p gorp-core/src/backends
cp /Users/harper/workspace/2389/gorp-rs/gorp-agent/src/backends/mod.rs gorp-core/src/backends/
cp /Users/harper/workspace/2389/gorp-rs/gorp-agent/src/backends/mock.rs gorp-core/src/backends/
cp /Users/harper/workspace/2389/gorp-rs/gorp-agent/src/backends/direct_cli.rs gorp-core/src/backends/
cp /Users/harper/workspace/2389/gorp-rs/gorp-agent/src/backends/acp.rs gorp-core/src/backends/
```

**Step 2: Check if direct_codex.rs is needed**

```bash
ls /Users/harper/workspace/2389/gorp-rs/gorp-agent/src/backends/
# If direct_codex.rs exists and is used, copy it too
```

**Step 3: Verify compilation**

```bash
cargo check -p gorp-core 2>&1 | grep -E "^error" | head -10
# Should be closer to compiling now
```

**Step 4: Commit**

```bash
git add gorp-core/src/backends/
git commit -m "feat: add backend implementations from gorp-agent"
```

---

## Phase 3: Add Session Management

### Task 9: Create session.rs with WarmSession and SessionManager

**Files:**
- Create: `gorp-core/src/session.rs`
- Reference: `/Users/harper/workspace/2389/gorp-rs/src/warm_session.rs`

**Step 1: Create session.rs with core types**

```rust
// ABOUTME: Warm session management for Claude Code CLI.
// ABOUTME: Keeps AgentHandle instances alive per channel with TTL cleanup.

use crate::{AgentHandle, AgentRegistry, EventReceiver};
use anyhow::Result;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

/// Configuration for warm session behavior
#[derive(Debug, Clone)]
pub struct WarmConfig {
    pub keep_alive_duration: Duration,
    pub agent_binary: String,
    pub backend_type: String,
}

impl Default for WarmConfig {
    fn default() -> Self {
        Self {
            keep_alive_duration: Duration::from_secs(3600),
            agent_binary: "claude".to_string(),
            backend_type: "acp".to_string(),
        }
    }
}

/// Configuration for creating a session
#[derive(Debug, Clone)]
pub struct SessionConfig {
    pub channel_id: String,
    pub workspace_path: String,
    pub session_id: Option<String>,
}

/// Handle to an active warm session
pub struct SessionHandle {
    inner: Arc<Mutex<WarmSession>>,
    session_id: String,
}

impl SessionHandle {
    /// Send a prompt and receive streaming events
    pub async fn prompt(&self, text: &str) -> Result<EventReceiver> {
        let mut session = self.inner.lock().await;
        session.last_used = Instant::now();
        session.handle.prompt(&self.session_id, text).await
    }

    /// Get the session ID
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Check if session is still valid
    pub fn is_valid(&self) -> bool {
        // Try to check without blocking
        match self.inner.try_lock() {
            Ok(session) => !session.invalidated,
            Err(_) => true, // If locked, it's in use and valid
        }
    }

    /// Cancel any in-progress prompt
    pub async fn cancel(&self) -> Result<()> {
        let session = self.inner.lock().await;
        session.handle.cancel(&self.session_id).await
    }
}

/// Internal warm session state
struct WarmSession {
    handle: AgentHandle,
    last_used: Instant,
    invalidated: bool,
}

/// Manages pool of warm sessions
pub struct SessionManager {
    sessions: HashMap<String, Arc<Mutex<WarmSession>>>,
    config: WarmConfig,
    registry: AgentRegistry,
}

impl SessionManager {
    /// Create a new session manager
    pub fn new(config: WarmConfig) -> Self {
        Self {
            sessions: HashMap::new(),
            config,
            registry: AgentRegistry::default(),
        }
    }

    /// Get or create a warm session for a channel
    pub async fn get_or_create(&mut self, config: SessionConfig) -> Result<SessionHandle> {
        // Check for existing session
        if let Some(inner) = self.sessions.get(&config.channel_id) {
            let mut session = inner.lock().await;
            if !session.invalidated {
                session.last_used = Instant::now();
                // Get or create session ID
                let session_id = config.session_id.clone().unwrap_or_else(|| {
                    uuid::Uuid::new_v4().to_string()
                });
                return Ok(SessionHandle {
                    inner: Arc::clone(inner),
                    session_id,
                });
            }
        }

        // Create new session
        let backend_config = serde_json::json!({
            "working_dir": config.workspace_path,
            "binary": self.config.agent_binary,
        });

        let handle = self.registry.create(&self.config.backend_type, &backend_config)?;

        // Create or resume session
        let session_id = match config.session_id {
            Some(id) => {
                handle.load_session(&id).await?;
                id
            }
            None => handle.new_session().await?,
        };

        let warm = WarmSession {
            handle,
            last_used: Instant::now(),
            invalidated: false,
        };

        let inner = Arc::new(Mutex::new(warm));
        self.sessions.insert(config.channel_id.clone(), Arc::clone(&inner));

        Ok(SessionHandle { inner, session_id })
    }

    /// Check if a channel has a warm session
    pub fn has_session(&self, channel_id: &str) -> bool {
        self.sessions.contains_key(channel_id)
    }

    /// Evict a session from the cache
    pub fn evict(&mut self, channel_id: &str) -> bool {
        self.sessions.remove(channel_id).is_some()
    }

    /// Invalidate a session (for orphan recovery)
    pub fn invalidate(&mut self, channel_id: &str) {
        if let Some(inner) = self.sessions.remove(channel_id) {
            if let Ok(mut session) = inner.try_lock() {
                session.invalidated = true;
            }
        }
    }

    /// Clean up expired sessions
    pub fn cleanup_stale(&mut self) {
        let now = Instant::now();
        let ttl = self.config.keep_alive_duration;

        self.sessions.retain(|_channel, inner| {
            match inner.try_lock() {
                Ok(session) => {
                    now.duration_since(session.last_used) <= ttl
                }
                Err(_) => true, // In use, keep it
            }
        });
    }
}
```

**Step 2: Verify compilation**

```bash
cargo check -p gorp-core
```

**Step 3: Commit**

```bash
git add gorp-core/src/session.rs
git commit -m "feat: add SessionManager and SessionHandle"
```

---

### Task 10: Add tests for session management

**Files:**
- Create: `gorp-core/tests/session_tests.rs`

**Step 1: Create test file**

```rust
// ABOUTME: Tests for SessionManager and SessionHandle.

use gorp_core::{SessionManager, WarmConfig, SessionConfig};
use std::time::Duration;

#[tokio::test]
async fn test_session_manager_creation() {
    let config = WarmConfig {
        keep_alive_duration: Duration::from_secs(3600),
        agent_binary: "claude".to_string(),
        backend_type: "mock".to_string(),
    };
    let manager = SessionManager::new(config);
    assert!(!manager.has_session("test-channel"));
}

#[tokio::test]
async fn test_get_or_create_session() {
    let config = WarmConfig {
        keep_alive_duration: Duration::from_secs(3600),
        agent_binary: "claude".to_string(),
        backend_type: "mock".to_string(),
    };
    let mut manager = SessionManager::new(config);

    let session_config = SessionConfig {
        channel_id: "test-channel".to_string(),
        workspace_path: "/tmp/test".to_string(),
        session_id: None,
    };

    let handle = manager.get_or_create(session_config).await.unwrap();
    assert!(manager.has_session("test-channel"));
    assert!(handle.is_valid());
}

#[tokio::test]
async fn test_evict_session() {
    let config = WarmConfig {
        keep_alive_duration: Duration::from_secs(3600),
        agent_binary: "claude".to_string(),
        backend_type: "mock".to_string(),
    };
    let mut manager = SessionManager::new(config);

    let session_config = SessionConfig {
        channel_id: "test-channel".to_string(),
        workspace_path: "/tmp/test".to_string(),
        session_id: None,
    };

    manager.get_or_create(session_config).await.unwrap();
    assert!(manager.has_session("test-channel"));

    let evicted = manager.evict("test-channel");
    assert!(evicted);
    assert!(!manager.has_session("test-channel"));
}
```

**Step 2: Run tests**

```bash
cargo test -p gorp-core session
# Expected: tests pass with mock backend
```

**Step 3: Commit**

```bash
git add gorp-core/tests/
git commit -m "test: add session management tests"
```

---

## Phase 4: UniFFI Bindings

### Task 11: Create gorp-core-ffi crate

**Files:**
- Create: `gorp-core-ffi/Cargo.toml`
- Create: `gorp-core-ffi/src/lib.rs`
- Create: `gorp-core-ffi/src/gorp_core.udl`

**Step 1: Create crate directory**

```bash
mkdir -p gorp-core-ffi/src
```

**Step 2: Create Cargo.toml**

```toml
[package]
name = "gorp-core-ffi"
version.workspace = true
edition.workspace = true
license.workspace = true
description = "UniFFI bindings for gorp-core"

[lib]
crate-type = ["cdylib", "staticlib"]

[dependencies]
gorp-core = { path = "../gorp-core" }
uniffi = { workspace = true }
tokio = { workspace = true }

[build-dependencies]
uniffi = { workspace = true, features = ["build"] }
```

**Step 3: Create src/lib.rs**

```rust
// ABOUTME: UniFFI bindings for gorp-core.
// ABOUTME: Exposes SessionManager and related types to Swift/Kotlin.

use gorp_core::{SessionManager, WarmConfig, SessionConfig, SessionHandle};
use std::sync::Arc;
use tokio::sync::Mutex;

uniffi::setup_scaffolding!();

/// UniFFI-compatible wrapper for SessionManager
#[derive(uniffi::Object)]
pub struct GorpSessionManager {
    inner: Arc<Mutex<SessionManager>>,
    runtime: tokio::runtime::Runtime,
}

#[uniffi::export]
impl GorpSessionManager {
    #[uniffi::constructor]
    pub fn new(config: GorpWarmConfig) -> Self {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let manager = SessionManager::new(config.into());
        Self {
            inner: Arc::new(Mutex::new(manager)),
            runtime,
        }
    }

    pub fn get_or_create(&self, config: GorpSessionConfig) -> Result<GorpSessionHandle, GorpError> {
        let inner = Arc::clone(&self.inner);
        self.runtime.block_on(async {
            let mut manager = inner.lock().await;
            let handle = manager.get_or_create(config.into()).await
                .map_err(|e| GorpError::SessionCreation { message: e.to_string() })?;
            Ok(GorpSessionHandle { inner: handle })
        })
    }

    pub fn has_session(&self, channel_id: String) -> bool {
        self.runtime.block_on(async {
            let manager = self.inner.lock().await;
            manager.has_session(&channel_id)
        })
    }

    pub fn evict(&self, channel_id: String) -> bool {
        self.runtime.block_on(async {
            let mut manager = self.inner.lock().await;
            manager.evict(&channel_id)
        })
    }

    pub fn cleanup_stale(&self) {
        self.runtime.block_on(async {
            let mut manager = self.inner.lock().await;
            manager.cleanup_stale();
        })
    }
}

/// UniFFI-compatible config
#[derive(uniffi::Record)]
pub struct GorpWarmConfig {
    pub keep_alive_secs: u64,
    pub agent_binary: String,
    pub backend_type: String,
}

impl From<GorpWarmConfig> for WarmConfig {
    fn from(c: GorpWarmConfig) -> Self {
        WarmConfig {
            keep_alive_duration: std::time::Duration::from_secs(c.keep_alive_secs),
            agent_binary: c.agent_binary,
            backend_type: c.backend_type,
        }
    }
}

#[derive(uniffi::Record)]
pub struct GorpSessionConfig {
    pub channel_id: String,
    pub workspace_path: String,
    pub session_id: Option<String>,
}

impl From<GorpSessionConfig> for SessionConfig {
    fn from(c: GorpSessionConfig) -> Self {
        SessionConfig {
            channel_id: c.channel_id,
            workspace_path: c.workspace_path,
            session_id: c.session_id,
        }
    }
}

/// UniFFI-compatible session handle
#[derive(uniffi::Object)]
pub struct GorpSessionHandle {
    inner: SessionHandle,
}

#[uniffi::export]
impl GorpSessionHandle {
    pub fn session_id(&self) -> String {
        self.inner.session_id().to_string()
    }

    pub fn is_valid(&self) -> bool {
        self.inner.is_valid()
    }
}

/// Error types for UniFFI
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum GorpError {
    #[error("Session creation failed: {message}")]
    SessionCreation { message: String },

    #[error("Prompt failed: {message}")]
    PromptFailed { message: String },

    #[error("Session orphaned: {reason}")]
    SessionOrphaned { reason: String },
}
```

**Step 4: Create build.rs**

```bash
cat > gorp-core-ffi/build.rs << 'EOF'
fn main() {
    uniffi::generate_scaffolding("src/gorp_core.udl").unwrap();
}
EOF
```

**Step 5: Create UDL file**

```bash
cat > gorp-core-ffi/src/gorp_core.udl << 'EOF'
namespace gorp_core {};

[Error]
enum GorpError {
    "SessionCreation",
    "PromptFailed",
    "SessionOrphaned",
};

dictionary GorpWarmConfig {
    u64 keep_alive_secs;
    string agent_binary;
    string backend_type;
};

dictionary GorpSessionConfig {
    string channel_id;
    string workspace_path;
    string? session_id;
};

interface GorpSessionManager {
    constructor(GorpWarmConfig config);

    [Throws=GorpError]
    GorpSessionHandle get_or_create(GorpSessionConfig config);

    boolean has_session(string channel_id);
    boolean evict(string channel_id);
    void cleanup_stale();
};

interface GorpSessionHandle {
    string session_id();
    boolean is_valid();
};
EOF
```

**Step 6: Verify compilation**

```bash
cargo build -p gorp-core-ffi
```

**Step 7: Commit**

```bash
git add gorp-core-ffi/
git commit -m "feat: add UniFFI bindings for gorp-core"
```

---

## Phase 5: Build Infrastructure

### Task 12: Create xcframework build script

**Files:**
- Create: `scripts/build-xcframework.sh`

**Step 1: Create script**

```bash
mkdir -p scripts
cat > scripts/build-xcframework.sh << 'SCRIPT'
#!/bin/bash
set -euo pipefail

# Build for macOS arm64 and x86_64
echo "Building for macOS arm64..."
cargo build --release --target aarch64-apple-darwin -p gorp-core-ffi

echo "Building for macOS x86_64..."
cargo build --release --target x86_64-apple-darwin -p gorp-core-ffi

# Generate Swift bindings
echo "Generating Swift bindings..."
cargo run --release -p gorp-core-ffi --bin uniffi-bindgen generate \
    --library target/aarch64-apple-darwin/release/libgorp_core_ffi.dylib \
    --language swift \
    --out-dir swift/GorpCore/Sources/GorpCore

# Create universal binary
echo "Creating universal binary..."
mkdir -p target/universal-apple-darwin/release
lipo -create \
    target/aarch64-apple-darwin/release/libgorp_core_ffi.a \
    target/x86_64-apple-darwin/release/libgorp_core_ffi.a \
    -output target/universal-apple-darwin/release/libgorp_core_ffi.a

# Create xcframework
echo "Creating xcframework..."
rm -rf swift/GorpCore/GorpCoreFFI.xcframework
xcodebuild -create-xcframework \
    -library target/universal-apple-darwin/release/libgorp_core_ffi.a \
    -headers swift/GorpCore/Sources/GorpCore/gorp_coreFFI.h \
    -output swift/GorpCore/GorpCoreFFI.xcframework

echo "Done! xcframework at swift/GorpCore/GorpCoreFFI.xcframework"
SCRIPT

chmod +x scripts/build-xcframework.sh
```

**Step 2: Commit**

```bash
git add scripts/
git commit -m "chore: add xcframework build script"
```

---

### Task 13: Create Swift package

**Files:**
- Create: `swift/GorpCore/Package.swift`
- Create: `swift/GorpCore/Sources/GorpCore/.gitkeep`

**Step 1: Create directory structure**

```bash
mkdir -p swift/GorpCore/Sources/GorpCore
touch swift/GorpCore/Sources/GorpCore/.gitkeep
```

**Step 2: Create Package.swift**

```swift
// swift-tools-version: 5.9
import PackageDescription

let package = Package(
    name: "GorpCore",
    platforms: [
        .macOS(.v13),
        .iOS(.v16),
    ],
    products: [
        .library(
            name: "GorpCore",
            targets: ["GorpCore"]
        ),
    ],
    targets: [
        .binaryTarget(
            name: "GorpCoreFFI",
            path: "GorpCoreFFI.xcframework"
        ),
        .target(
            name: "GorpCore",
            dependencies: ["GorpCoreFFI"],
            path: "Sources/GorpCore"
        ),
    ]
)
```

**Step 3: Commit**

```bash
git add swift/
git commit -m "chore: add Swift package structure"
```

---

### Task 14: Add CI workflow

**Files:**
- Create: `.github/workflows/ci.yml`

**Step 1: Create workflow**

```bash
mkdir -p .github/workflows
cat > .github/workflows/ci.yml << 'EOF'
name: CI

on:
  push:
    branches: [main]
  pull_request:
    branches: [main]

jobs:
  test:
    runs-on: macos-latest
    steps:
      - uses: actions/checkout@v4

      - name: Install Rust
        uses: dtolnay/rust-action@stable
        with:
          targets: aarch64-apple-darwin,x86_64-apple-darwin

      - name: Run tests
        run: cargo test --all-features

      - name: Build release
        run: cargo build --release -p gorp-core-ffi

      - name: Build xcframework
        run: ./scripts/build-xcframework.sh

      - name: Test Swift package
        run: |
          cd swift/GorpCore
          swift build
EOF
```

**Step 2: Commit**

```bash
git add .github/
git commit -m "ci: add GitHub Actions workflow"
```

---

### Task 15: Final verification and tag

**Step 1: Run all tests**

```bash
cargo test --all-features
```

**Step 2: Build everything**

```bash
cargo build --release
./scripts/build-xcframework.sh
```

**Step 3: Create initial tag**

```bash
git tag -a v0.1.0 -m "Initial release with SessionManager and UniFFI bindings"
```

**Step 4: Push to GitHub (if repo exists)**

```bash
# git remote add origin git@github.com:2389/gorp-core.git
# git push -u origin main --tags
```

---

## Summary

This plan creates gorp-core in 15 tasks:

1. **Tasks 1-2:** Repository and crate setup
2. **Tasks 3-8:** Extract core types from gorp-agent
3. **Tasks 9-10:** Add session management with tests
4. **Tasks 11:** UniFFI bindings
5. **Tasks 12-14:** Build infrastructure (xcframework, Swift package, CI)
6. **Task 15:** Final verification

Each task is a small, committable unit of work following TDD where applicable.
