# gorp-ffi Design: UniFFI Bindings for Swift/Kotlin

**Date:** 2026-01-01
**Status:** Approved
**Author:** Claude + Doctor Biz

## Overview

Add a `gorp-ffi` crate providing UniFFI bindings for gorp-agent and gorp-core, enabling native macOS/iOS/Android apps to use gorp's agent orchestration.

## Motivation

BuddyAgent (macOS SwiftUI chat agent) wants to use gorp-rs instead of raw mux-rs to gain:
- Session persistence (SQLite)
- Scheduling (cron-based prompts)
- Multi-backend support (ACP/mux/CLI)
- Warm sessions
- Built-in command parsing
- AgentEvent streaming

## Design Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Runtime | Embedded Tokio (lazy, 2 threads) | Simple for consumers, no lifecycle management |
| Event streaming | Callback interface | Natural for real-time UI updates |
| Scope | Full stack (gorp-agent + gorp-core) | Provides complete feature set |
| Backends | All (mux, acp, mock, direct) | Mock useful for testing |

## Crate Structure

```
gorp-ffi/
├── Cargo.toml
├── src/
│   ├── lib.rs          # Module exports, UniFFI scaffolding
│   ├── runtime.rs      # Embedded Tokio runtime
│   ├── agent.rs        # FfiAgentHandle, FfiAgentRegistry
│   ├── events.rs       # FfiAgentEvent, AgentEventCallback trait
│   ├── session.rs      # FfiSessionStore wrapper
│   ├── scheduler.rs    # FfiSchedulerStore wrapper
│   └── gorp_ffi.udl    # UniFFI interface definition
└── tests/
    └── bindings_test.rs
```

## Runtime Management

Lazy-initialized embedded Tokio runtime:

```rust
static RUNTIME: Lazy<Arc<Runtime>> = Lazy::new(|| {
    Arc::new(
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("Failed to create Tokio runtime")
    )
});
```

- Created on first use
- 2 worker threads (balance between concurrency and resources)
- Lives for app lifetime (no explicit shutdown)

## Callback Interface

```rust
#[uniffi::export(callback_interface)]
pub trait AgentEventCallback: Send + Sync {
    fn on_text(&self, text: String);
    fn on_tool_start(&self, id: String, name: String, input_json: String);
    fn on_tool_progress(&self, id: String, update_json: String);
    fn on_tool_end(&self, id: String, name: String, output_json: String, success: bool, duration_ms: u64);
    fn on_result(&self, text: String, usage: Option<FfiUsage>, metadata_json: String);
    fn on_error(&self, code: FfiErrorCode, message: String, recoverable: bool);
    fn on_session_invalid(&self, reason: String);
    fn on_session_changed(&self, new_session_id: String);
    fn on_custom(&self, kind: String, payload_json: String);
}
```

Complex types (tool input/output, metadata) passed as JSON strings for FFI simplicity.

## Core Types

### FfiAgentRegistry

```rust
#[derive(uniffi::Object)]
pub struct FfiAgentRegistry { inner: AgentRegistry }

impl FfiAgentRegistry {
    fn new() -> Self;
    fn available_backends(&self) -> Vec<String>;
    fn create(&self, name: String, config_json: String) -> Result<FfiAgentHandle, FfiError>;
}
```

### FfiAgentHandle

```rust
#[derive(uniffi::Object)]
pub struct FfiAgentHandle { inner: AgentHandle }

impl FfiAgentHandle {
    fn name(&self) -> String;
    fn new_session(&self) -> Result<String, FfiError>;
    fn load_session(&self, session_id: String) -> Result<(), FfiError>;
    fn prompt(&self, session_id: String, text: String, callback: Box<dyn AgentEventCallback>) -> Result<(), FfiError>;
    fn cancel(&self, session_id: String) -> Result<(), FfiError>;
}
```

Note: `prompt()` spawns async task and returns immediately. Events stream via callback.

### FfiSessionStore

```rust
#[derive(uniffi::Object)]
pub struct FfiSessionStore { inner: SessionStore }

impl FfiSessionStore {
    fn new(workspace_path: String) -> Result<Self, FfiError>;
    fn create_channel(&self, channel_name: String, room_id: String) -> Result<FfiChannel, FfiError>;
    fn get_by_name(&self, channel_name: String) -> Result<Option<FfiChannel>, FfiError>;
    fn list_all(&self) -> Result<Vec<FfiChannel>, FfiError>;
    fn mark_started(&self, room_id: String) -> Result<(), FfiError>;
    fn delete_channel(&self, channel_name: String) -> Result<(), FfiError>;
    // ... other methods
}
```

### FfiSchedulerStore

```rust
#[derive(uniffi::Object)]
pub struct FfiSchedulerStore { inner: SchedulerStore }

impl FfiSchedulerStore {
    fn new(session_store: &FfiSessionStore) -> Result<Self, FfiError>;
    fn add(&self, channel_name: String, cron: String, prompt: String) -> Result<FfiScheduledPrompt, FfiError>;
    fn list_all(&self) -> Result<Vec<FfiScheduledPrompt>, FfiError>;
    fn get_due(&self) -> Result<Vec<FfiScheduledPrompt>, FfiError>;
    fn mark_run(&self, id: String) -> Result<(), FfiError>;
}
```

## Error Handling

```rust
#[derive(uniffi::Error)]
pub enum FfiError {
    InvalidConfig(String),
    BackendError(String),
    SessionError(String),
    IoError(String),
}

#[derive(uniffi::Enum)]
pub enum FfiErrorCode {
    Timeout, RateLimited, AuthFailed, SessionOrphaned,
    ToolFailed, PermissionDenied, BackendError, Unknown,
}
```

## Build Output

- `libgorp_ffi.a` (static library)
- `libgorp_ffi.dylib` (dynamic library)
- `gorp_ffi.swift` (generated Swift bindings)
- `gorp_ffiFFI.h` (C header for bridging)

## Swift Usage Example

```swift
import GorpFFI

class EventHandler: AgentEventCallback {
    func onText(_ text: String) {
        DispatchQueue.main.async {
            self.textView.text += text
        }
    }

    func onToolStart(_ id: String, _ name: String, _ inputJson: String) {
        print("Tool \(name) started")
    }

    func onResult(_ text: String, _ usage: FfiUsage?, _ metadataJson: String) {
        print("Done! Tokens: \(usage?.inputTokens ?? 0) in, \(usage?.outputTokens ?? 0) out")
    }

    // ... other callbacks
}

// Create registry and agent
let registry = FfiAgentRegistry()
let agent = try registry.create(name: "mux", configJson: #"{"model": "claude-sonnet-4-20250514"}"#)

// Create session and prompt
let sessionId = try agent.newSession()
try agent.prompt(sessionId: sessionId, text: "Hello!", callback: EventHandler())
```

## Future Considerations

- Kotlin bindings (UniFFI generates these too)
- XCFramework packaging for iOS/macOS distribution
- CocoaPods/SPM package
- Callback-based scheduler runner (vs polling `get_due()`)
