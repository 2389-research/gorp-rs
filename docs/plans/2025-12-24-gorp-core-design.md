# gorp-core Design

A shared Rust library with UniFFI bindings for warm session management, used by both gorp-rs and maison.

## Problem

gorp-rs (Rust) and maison (Swift) implement the same warm session management logic independently. Bug fixes must be applied twice. The core abstractions are identical but maintained separately.

## Solution

Extract the session management core into a standalone `gorp-core` crate with UniFFI bindings. maison consumes it via Swift package. gorp-rs can optionally migrate later (no changes required to production).

## Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Location | New standalone repo | Clean separation, both consumers depend independently |
| FFI approach | UniFFI | Auto-generates Swift bindings, handles async/memory; maison already uses it via matrix-rust-components |
| Scope | Full extraction from gorp-agent | All abstractions already battle-tested |
| Priority | maison first | gorp-rs production stays untouched |
| gorp-rs impact | None initially | Can migrate later when ready |

## Repository Structure

```
github.com/2389/gorp-core/
├── Cargo.toml              # Workspace root
├── README.md
├── gorp-core/              # Main library crate
│   ├── Cargo.toml
│   ├── src/
│   │   ├── lib.rs          # Public API
│   │   ├── event.rs        # AgentEvent, ErrorCode, Usage
│   │   ├── handle.rs       # AgentHandle, EventReceiver
│   │   ├── session.rs      # WarmSession, SessionManager
│   │   ├── backend.rs      # AgentBackend trait
│   │   ├── registry.rs     # BackendRegistry
│   │   ├── config.rs       # Configuration types
│   │   └── backends/
│   │       ├── mod.rs
│   │       ├── acp.rs      # ACP backend (Claude Code)
│   │       ├── direct_cli.rs
│   │       └── mock.rs
│   └── uniffi.toml         # UniFFI configuration
├── gorp-core-ffi/          # UniFFI bindings crate
│   ├── Cargo.toml
│   ├── src/lib.rs
│   └── src/gorp_core.udl   # UniFFI interface definition
├── swift/                  # Generated + hand-written Swift
│   └── GorpCore/
│       └── Package.swift
└── scripts/
    └── build-xcframework.sh
```

## Core Types

### Events (from gorp-agent)

```rust
pub enum AgentEvent {
    Text(String),
    ToolStart { id: String, name: String, input: Value },
    ToolProgress { id: String, update: Value },
    ToolEnd { id: String, name: String, output: Value, success: bool, duration_ms: u64 },
    Result { text: String, usage: Option<Usage>, metadata: Value },
    Error { code: ErrorCode, message: String, recoverable: bool },
    SessionInvalid { reason: String },
    SessionChanged { new_session_id: String },
}

pub enum ErrorCode {
    Timeout, RateLimited, AuthFailed, SessionOrphaned,
    ToolFailed, PermissionDenied, BackendError, Unknown,
}

pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: Option<u64>,
    pub cost_usd: Option<f64>,
}
```

### Handle

```rust
pub struct AgentHandle { /* channel to worker thread */ }
pub struct EventReceiver { /* mpsc receiver for streaming */ }
pub enum SessionState { New, FirstPromptInFlight, Active }
```

## Warm Session Management

### Config

```rust
pub struct WarmConfig {
    pub keep_alive_duration: Duration,
    pub agent_binary: String,
    pub backend_type: String,  // "acp", "direct", "mock"
}

pub struct SessionConfig {
    pub channel_id: String,
    pub workspace_path: String,
    pub session_id: Option<String>,  // For resume
}
```

### Manager

```rust
pub struct SessionManager {
    sessions: HashMap<String, Arc<Mutex<WarmSession>>>,
    config: WarmConfig,
    registry: BackendRegistry,
}

impl SessionManager {
    pub fn new(config: WarmConfig) -> Self;
    pub async fn get_or_create(&mut self, config: SessionConfig) -> Result<SessionHandle>;
    pub fn has_session(&self, channel_id: &str) -> bool;
    pub fn evict(&mut self, channel_id: &str) -> bool;
    pub fn invalidate(&mut self, channel_id: &str);  // Orphan recovery
    pub fn cleanup_stale(&mut self);                  // TTL cleanup
}
```

## UniFFI Interface

```webidl
namespace gorp_core {
    SessionManager create_session_manager(WarmConfig config);
};

dictionary WarmConfig {
    u64 keep_alive_secs;
    string agent_binary;
    string backend_type;
};

dictionary SessionConfig {
    string channel_id;
    string workspace_path;
    string? session_id;
};

interface SessionManager {
    [Async]
    SessionHandle get_or_create(SessionConfig config);

    boolean has_session(string channel_id);
    boolean evict(string channel_id);
    void invalidate(string channel_id);
    void cleanup_stale();
};

interface SessionHandle {
    [Async]
    EventStream prompt(string text);

    string session_id();
    boolean is_valid();
    void cancel();
};

interface EventStream {
    [Async]
    AgentEvent? next();
};
```

## Maison Integration

### Before (~420 lines duplicated Swift)

- ClaudeCodeSession.swift (~220 lines)
- SessionManager.swift (~160 lines)
- WarmSession.swift (~40 lines)

### After (~30 lines wrapper)

```swift
import GorpCore

actor SessionManager {
    private let manager: GorpCore.SessionManager

    init(config: AgentConfig) {
        self.manager = createSessionManager(config: WarmConfig(
            keepAliveSecs: UInt64(config.warmSessionTTL),
            agentBinary: "claude",
            backendType: "acp"
        ))
    }

    func handlePrompt(_ prompt: String, for channel: ChannelID) -> AsyncThrowingStream<AgentEvent, Error> {
        AsyncThrowingStream { continuation in
            Task {
                let session = try await manager.getOrCreate(config: SessionConfig(
                    channelId: channel.roomID,
                    workspacePath: resolveWorkspacePath(for: channel),
                    sessionId: nil
                ))
                for try await event in try await session.prompt(prompt) {
                    continuation.yield(event.toMaison())
                }
                continuation.finish()
            }
        }
    }
}
```

## Error Handling

```rust
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum GorpError {
    #[error("Session creation failed: {message}")]
    SessionCreation { message: String },

    #[error("Prompt failed: {message}")]
    PromptFailed { message: String },

    #[error("Session orphaned: {reason}")]
    SessionOrphaned { reason: String },

    #[error("Backend error: {message}")]
    Backend { message: String },
}
```

Swift receives typed errors:

```swift
do {
    let session = try await manager.getOrCreate(config: config)
} catch GorpError.SessionOrphaned(let reason) {
    // Handle orphan specifically
}
```

## Testing Strategy

- Rust unit tests (extracted from gorp-agent)
- Mock backend for fast tests without Claude CLI
- Integration tests with actual Claude CLI
- maison Swift tests use mock backend via UniFFI

## CI Pipeline

1. `cargo test` - Rust tests
2. `cargo build --target aarch64-apple-darwin` - macOS arm64
3. `scripts/build-xcframework.sh` - Swift package
4. Swift package tests in maison repo

## Migration Path

1. Create gorp-core repo with extracted code
2. Add UniFFI bindings
3. Build xcframework, publish Swift package
4. maison adopts gorp-core, deletes duplicated Swift
5. (Optional, later) gorp-rs migrates from gorp-agent to gorp-core
