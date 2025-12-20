# gorp-agent Crate Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Extract agent communication into a pluggable `gorp-agent` crate with ACP, direct CLI, and mock backends.

**Architecture:** New sibling crate `gorp-agent/` with trait-based backend abstraction. `AgentHandle` provides `Send + Sync` wrapper around potentially `!Send` backends. Registry pattern enables runtime backend selection.

**Tech Stack:** Rust, tokio, async-trait, serde, tracing, agent-client-protocol (optional)

**Design Doc:** `docs/plans/2025-12-20-pluggable-agent-backend-design.md`

---

## Dependency Graph (for parallel execution)

```
Phase 1 (Sequential - Foundation):
  Task 1: Create crate skeleton
  Task 2: Core event types
  Task 3: AgentBackend trait
  Task 4: AgentHandle wrapper

Phase 2 (Parallel - Backends):
  Task 5a: ACP backend        ─┐
  Task 5b: Direct CLI backend ─┼─ can run in parallel
  Task 5c: Mock backend       ─┘

Phase 3 (Parallel - Testing):
  Task 6a: MockAgent builder     ─┐
  Task 6b: Recording/Replay      ─┼─ can run in parallel
  Task 6c: Scenario test runner  ─┘

Phase 4 (Sequential - Integration):
  Task 7: Registry implementation
  Task 8: Gorp integration
  Task 9: Scenario test suite
```

---

## Phase 1: Foundation (Sequential)

### Task 1: Create Crate Skeleton

**Files:**
- Create: `gorp-agent/Cargo.toml`
- Create: `gorp-agent/src/lib.rs`
- Modify: `Cargo.toml` (workspace or path dependency)

**Step 1: Create the gorp-agent directory**

```bash
mkdir -p gorp-agent/src
```

**Step 2: Create Cargo.toml**

Create `gorp-agent/Cargo.toml`:

```toml
[package]
name = "gorp-agent"
version = "0.1.0"
edition = "2021"
description = "Pluggable agent backend abstraction for gorp"
license = "MIT"

[dependencies]
tokio = { version = "1", features = ["sync", "rt", "macros"] }
async-trait = "0.1"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tracing = "0.1"
anyhow = "1"
futures = "0.3"
pin-project-lite = "0.2"

[dev-dependencies]
tokio = { version = "1", features = ["full", "test-util"] }
tracing-subscriber = { version = "0.3", features = ["env-filter"] }

[features]
default = []
acp = ["dep:agent-client-protocol"]

[dependencies.agent-client-protocol]
version = "0.9"
optional = true
```

**Step 3: Create lib.rs with module structure**

Create `gorp-agent/src/lib.rs`:

```rust
// ABOUTME: Pluggable agent backend abstraction for gorp.
// ABOUTME: Provides trait-based backends (ACP, direct CLI, mock) with Send+Sync handles.

pub mod event;
pub mod traits;
pub mod handle;
pub mod registry;

pub mod backends;
pub mod testing;

pub use event::{AgentEvent, ErrorCode, Usage};
pub use traits::AgentBackend;
pub use handle::{AgentHandle, EventReceiver};
pub use registry::{AgentRegistry, BackendFactory};
```

**Step 4: Create placeholder modules**

Create `gorp-agent/src/event.rs`:
```rust
// ABOUTME: Event types emitted by agent backends during prompt execution.
// ABOUTME: Includes tool lifecycle, results, errors, and extensibility via Custom variant.
```

Create `gorp-agent/src/traits.rs`:
```rust
// ABOUTME: Core AgentBackend trait that all backends implement.
// ABOUTME: Defines session management and prompt execution interface.
```

Create `gorp-agent/src/handle.rs`:
```rust
// ABOUTME: AgentHandle provides Send+Sync wrapper around potentially !Send backends.
// ABOUTME: Uses channels to communicate with backend worker thread.
```

Create `gorp-agent/src/registry.rs`:
```rust
// ABOUTME: Registry pattern for runtime backend selection.
// ABOUTME: Backends register factories, gorp creates by name from config.
```

Create `gorp-agent/src/backends/mod.rs`:
```rust
// ABOUTME: Backend implementations (ACP, direct CLI, mock).
// ABOUTME: Each backend implements AgentBackend trait.

pub mod mock;

#[cfg(feature = "acp")]
pub mod acp;

pub mod direct_cli;
```

Create `gorp-agent/src/backends/mock.rs`:
```rust
// ABOUTME: Mock backend for testing - returns pre-configured responses.
// ABOUTME: Allows deterministic tests without spawning real agent processes.
```

Create `gorp-agent/src/backends/direct_cli.rs`:
```rust
// ABOUTME: Direct CLI backend - spawns claude with --print --output-format stream-json.
// ABOUTME: Parses streaming JSONL from stdout, emits AgentEvents.
```

Create `gorp-agent/src/testing/mod.rs`:
```rust
// ABOUTME: Testing infrastructure for agent backends.
// ABOUTME: Mock builders, recording/replay, scenario test runner.

pub mod mock_builder;
pub mod recording;
pub mod scenarios;
```

Create placeholder files:
```bash
touch gorp-agent/src/testing/mock_builder.rs
touch gorp-agent/src/testing/recording.rs
touch gorp-agent/src/testing/scenarios.rs
```

**Step 5: Verify it compiles**

Run: `cd gorp-agent && cargo check`
Expected: Compiles with warnings about empty modules

**Step 6: Commit**

```bash
git add gorp-agent/
git commit -m "feat(gorp-agent): create crate skeleton with module structure"
```

---

### Task 2: Core Event Types

**Files:**
- Modify: `gorp-agent/src/event.rs`
- Create: `gorp-agent/tests/event_tests.rs`

**Step 1: Write failing test for event serialization**

Create `gorp-agent/tests/event_tests.rs`:

```rust
use gorp_agent::{AgentEvent, ErrorCode, Usage};
use serde_json::json;

#[test]
fn test_text_event_serializes() {
    let event = AgentEvent::Text("hello".to_string());
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json, json!({"Text": "hello"}));
}

#[test]
fn test_tool_start_event_serializes() {
    let event = AgentEvent::ToolStart {
        id: "tool-1".to_string(),
        name: "Read".to_string(),
        input: json!({"path": "/tmp/foo.txt"}),
    };
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["ToolStart"]["name"], "Read");
}

#[test]
fn test_tool_end_event_serializes() {
    let event = AgentEvent::ToolEnd {
        id: "tool-1".to_string(),
        name: "Read".to_string(),
        output: json!({"content": "file contents"}),
        success: true,
        duration_ms: 42,
    };
    let json = serde_json::to_value(&event).unwrap();
    assert!(json["ToolEnd"]["success"].as_bool().unwrap());
    assert_eq!(json["ToolEnd"]["duration_ms"], 42);
}

#[test]
fn test_result_event_with_usage() {
    let event = AgentEvent::Result {
        text: "Done!".to_string(),
        usage: Some(Usage {
            input_tokens: 100,
            output_tokens: 50,
            cache_read_tokens: Some(10),
            cache_write_tokens: None,
            cost_usd: Some(0.001),
            extra: None,
        }),
        metadata: json!({}),
    };
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["Result"]["usage"]["input_tokens"], 100);
}

#[test]
fn test_error_event_with_code() {
    let event = AgentEvent::Error {
        code: ErrorCode::Timeout,
        message: "Request timed out".to_string(),
        recoverable: true,
    };
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["Error"]["code"], "Timeout");
}

#[test]
fn test_custom_event_extensibility() {
    let event = AgentEvent::Custom {
        kind: "acp.thought_chunk".to_string(),
        payload: json!({"text": "thinking..."}),
    };
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["Custom"]["kind"], "acp.thought_chunk");
}

#[test]
fn test_event_deserializes_roundtrip() {
    let event = AgentEvent::ToolStart {
        id: "t1".to_string(),
        name: "Bash".to_string(),
        input: json!({"command": "ls"}),
    };
    let json_str = serde_json::to_string(&event).unwrap();
    let parsed: AgentEvent = serde_json::from_str(&json_str).unwrap();
    match parsed {
        AgentEvent::ToolStart { name, .. } => assert_eq!(name, "Bash"),
        _ => panic!("Wrong variant"),
    }
}
```

**Step 2: Run tests to verify they fail**

Run: `cd gorp-agent && cargo test event_tests`
Expected: Compilation errors - types don't exist yet

**Step 3: Implement event types**

Replace `gorp-agent/src/event.rs`:

```rust
// ABOUTME: Event types emitted by agent backends during prompt execution.
// ABOUTME: Includes tool lifecycle, results, errors, and extensibility via Custom variant.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Events emitted by agent backends during prompt execution
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AgentEvent {
    /// Streaming text chunk for real-time display
    Text(String),

    /// Tool started execution
    ToolStart {
        /// Unique identifier for this tool invocation
        id: String,
        /// Tool name (e.g., "Read", "Bash", "Edit")
        name: String,
        /// Full input passed to the tool
        input: Value,
    },

    /// Tool progress update (backend-specific)
    ToolProgress {
        /// Matches the id from ToolStart
        id: String,
        /// Backend-specific progress data
        update: Value,
    },

    /// Tool completed execution
    ToolEnd {
        /// Matches the id from ToolStart
        id: String,
        /// Tool name
        name: String,
        /// Full output from the tool
        output: Value,
        /// Whether the tool succeeded
        success: bool,
        /// Execution time in milliseconds
        duration_ms: u64,
    },

    /// Final result with optional usage statistics
    Result {
        /// The final text response
        text: String,
        /// Token usage and cost (if available)
        usage: Option<Usage>,
        /// Backend-specific metadata
        metadata: Value,
    },

    /// Error occurred during execution
    Error {
        /// Typed error code for programmatic handling
        code: ErrorCode,
        /// Human-readable error message
        message: String,
        /// Whether the error is recoverable (can retry)
        recoverable: bool,
    },

    /// Session is invalid and needs to be recreated
    SessionInvalid {
        /// Reason the session became invalid
        reason: String,
    },

    /// Backend forced creation of a new session
    SessionChanged {
        /// The new session ID to use
        new_session_id: String,
    },

    /// Backend-specific event for extensibility
    Custom {
        /// Event kind (e.g., "acp.thought_chunk", "openai.run_step")
        kind: String,
        /// Event payload
        payload: Value,
    },
}

/// Typed error codes for programmatic handling
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ErrorCode {
    /// Request timed out
    Timeout,
    /// Rate limited by the backend
    RateLimited,
    /// Authentication failed
    AuthFailed,
    /// Session no longer exists
    SessionOrphaned,
    /// Tool execution failed
    ToolFailed,
    /// Permission denied for operation
    PermissionDenied,
    /// Backend-specific error
    BackendError,
    /// Unknown error
    Unknown,
}

/// Token usage and cost tracking
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Usage {
    /// Input tokens consumed
    pub input_tokens: u64,
    /// Output tokens generated
    pub output_tokens: u64,
    /// Tokens read from cache
    pub cache_read_tokens: Option<u64>,
    /// Tokens written to cache
    pub cache_write_tokens: Option<u64>,
    /// Total cost in USD
    pub cost_usd: Option<f64>,
    /// Backend-specific usage data
    pub extra: Option<Value>,
}
```

**Step 4: Run tests to verify they pass**

Run: `cd gorp-agent && cargo test event_tests`
Expected: All 7 tests pass

**Step 5: Commit**

```bash
git add gorp-agent/
git commit -m "feat(gorp-agent): implement AgentEvent, ErrorCode, and Usage types"
```

---

### Task 3: AgentBackend Trait

**Files:**
- Modify: `gorp-agent/src/traits.rs`
- Create: `gorp-agent/tests/traits_tests.rs`

**Step 1: Write failing test for trait bounds**

Create `gorp-agent/tests/traits_tests.rs`:

```rust
use gorp_agent::traits::AgentBackend;
use gorp_agent::AgentEvent;
use anyhow::Result;
use futures::stream::BoxStream;

// Test that a simple mock can implement the trait
struct TestBackend;

impl AgentBackend for TestBackend {
    fn name(&self) -> &'static str {
        "test"
    }

    fn new_session<'a>(&'a self) -> futures::future::BoxFuture<'a, Result<String>> {
        Box::pin(async { Ok("session-1".to_string()) })
    }

    fn load_session<'a>(&'a self, _session_id: &'a str) -> futures::future::BoxFuture<'a, Result<()>> {
        Box::pin(async { Ok(()) })
    }

    fn prompt<'a>(
        &'a self,
        _session_id: &'a str,
        _text: &'a str,
    ) -> futures::future::BoxFuture<'a, Result<BoxStream<'a, AgentEvent>>> {
        Box::pin(async {
            let stream = futures::stream::empty();
            Ok(Box::pin(stream) as BoxStream<'a, AgentEvent>)
        })
    }

    fn cancel<'a>(&'a self, _session_id: &'a str) -> futures::future::BoxFuture<'a, Result<()>> {
        Box::pin(async { Ok(()) })
    }
}

#[tokio::test]
async fn test_backend_can_create_session() {
    let backend = TestBackend;
    let session_id = backend.new_session().await.unwrap();
    assert_eq!(session_id, "session-1");
}

#[tokio::test]
async fn test_backend_returns_name() {
    let backend = TestBackend;
    assert_eq!(backend.name(), "test");
}
```

**Step 2: Run tests to verify they fail**

Run: `cd gorp-agent && cargo test traits_tests`
Expected: Compilation error - AgentBackend trait doesn't exist

**Step 3: Implement AgentBackend trait**

Replace `gorp-agent/src/traits.rs`:

```rust
// ABOUTME: Core AgentBackend trait that all backends implement.
// ABOUTME: Defines session management and prompt execution interface.

use crate::AgentEvent;
use anyhow::Result;
use futures::future::BoxFuture;
use futures::stream::BoxStream;

/// Core trait that all agent backends implement.
///
/// Backends may have `!Send` internals (like ACP), but the trait methods
/// return boxed futures that can be sent to other threads via AgentHandle.
pub trait AgentBackend {
    /// Backend name for logging and metrics
    fn name(&self) -> &'static str;

    /// Create a new session, returns the session ID
    fn new_session<'a>(&'a self) -> BoxFuture<'a, Result<String>>;

    /// Load/resume an existing session by ID
    fn load_session<'a>(&'a self, session_id: &'a str) -> BoxFuture<'a, Result<()>>;

    /// Send a prompt and receive a stream of events
    ///
    /// The returned stream emits events as they occur (text chunks, tool calls,
    /// etc.) and completes with a Result or Error event.
    fn prompt<'a>(
        &'a self,
        session_id: &'a str,
        text: &'a str,
    ) -> BoxFuture<'a, Result<BoxStream<'a, AgentEvent>>>;

    /// Cancel an in-progress prompt
    fn cancel<'a>(&'a self, session_id: &'a str) -> BoxFuture<'a, Result<()>>;
}
```

**Step 4: Update lib.rs to export traits module properly**

Update `gorp-agent/src/lib.rs`:

```rust
// ABOUTME: Pluggable agent backend abstraction for gorp.
// ABOUTME: Provides trait-based backends (ACP, direct CLI, mock) with Send+Sync handles.

pub mod event;
pub mod traits;
pub mod handle;
pub mod registry;

pub mod backends;
pub mod testing;

pub use event::{AgentEvent, ErrorCode, Usage};
pub use traits::AgentBackend;
// handle and registry exports will be added when implemented
```

**Step 5: Run tests to verify they pass**

Run: `cd gorp-agent && cargo test traits_tests`
Expected: All tests pass

**Step 6: Commit**

```bash
git add gorp-agent/
git commit -m "feat(gorp-agent): implement AgentBackend trait"
```

---

### Task 4: AgentHandle Wrapper

**Files:**
- Modify: `gorp-agent/src/handle.rs`
- Create: `gorp-agent/tests/handle_tests.rs`

**Step 1: Write failing test for handle**

Create `gorp-agent/tests/handle_tests.rs`:

```rust
use gorp_agent::handle::{AgentHandle, EventReceiver, Command};
use gorp_agent::AgentEvent;
use tokio::sync::mpsc;

fn assert_send<T: Send>() {}
fn assert_sync<T: Sync>() {}

#[test]
fn test_agent_handle_is_send_sync() {
    assert_send::<AgentHandle>();
    assert_sync::<AgentHandle>();
}

#[test]
fn test_event_receiver_is_send() {
    assert_send::<EventReceiver>();
}

#[tokio::test]
async fn test_event_receiver_receives_events() {
    let (tx, rx) = mpsc::channel(32);
    let mut receiver = EventReceiver::new(rx);

    tx.send(AgentEvent::Text("hello".to_string())).await.unwrap();
    tx.send(AgentEvent::Text("world".to_string())).await.unwrap();
    drop(tx);

    let event1 = receiver.recv().await.unwrap();
    assert!(matches!(event1, AgentEvent::Text(s) if s == "hello"));

    let event2 = receiver.recv().await.unwrap();
    assert!(matches!(event2, AgentEvent::Text(s) if s == "world"));

    let event3 = receiver.recv().await;
    assert!(event3.is_none());
}
```

**Step 2: Run tests to verify they fail**

Run: `cd gorp-agent && cargo test handle_tests`
Expected: Compilation error - types don't exist

**Step 3: Implement AgentHandle and EventReceiver**

Replace `gorp-agent/src/handle.rs`:

```rust
// ABOUTME: AgentHandle provides Send+Sync wrapper around potentially !Send backends.
// ABOUTME: Uses channels to communicate with backend worker thread.

use crate::AgentEvent;
use anyhow::Result;
use tokio::sync::{mpsc, oneshot};

/// Commands sent from AgentHandle to the backend worker
#[derive(Debug)]
pub enum Command {
    NewSession {
        reply: oneshot::Sender<Result<String>>,
    },
    LoadSession {
        session_id: String,
        reply: oneshot::Sender<Result<()>>,
    },
    Prompt {
        session_id: String,
        text: String,
        event_tx: mpsc::Sender<AgentEvent>,
        reply: oneshot::Sender<Result<()>>,
    },
    Cancel {
        session_id: String,
        reply: oneshot::Sender<Result<()>>,
    },
}

/// Send + Sync handle that gorp interacts with.
///
/// Internally communicates with a worker thread/task that runs the actual
/// backend. This allows backends with `!Send` futures (like ACP) to be
/// used safely across async tasks.
#[derive(Clone)]
pub struct AgentHandle {
    tx: mpsc::Sender<Command>,
    name: &'static str,
}

impl AgentHandle {
    /// Create a new AgentHandle with the given command channel and backend name
    pub fn new(tx: mpsc::Sender<Command>, name: &'static str) -> Self {
        Self { tx, name }
    }

    /// Get the backend name
    pub fn name(&self) -> &'static str {
        self.name
    }

    /// Create a new session
    pub async fn new_session(&self) -> Result<String> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.tx
            .send(Command::NewSession { reply: reply_tx })
            .await
            .map_err(|_| anyhow::anyhow!("Backend worker closed"))?;
        reply_rx
            .await
            .map_err(|_| anyhow::anyhow!("Backend worker dropped reply channel"))?
    }

    /// Load an existing session
    pub async fn load_session(&self, session_id: &str) -> Result<()> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.tx
            .send(Command::LoadSession {
                session_id: session_id.to_string(),
                reply: reply_tx,
            })
            .await
            .map_err(|_| anyhow::anyhow!("Backend worker closed"))?;
        reply_rx
            .await
            .map_err(|_| anyhow::anyhow!("Backend worker dropped reply channel"))?
    }

    /// Send a prompt and receive events via EventReceiver
    pub async fn prompt(&self, session_id: &str, text: &str) -> Result<EventReceiver> {
        let (event_tx, event_rx) = mpsc::channel(2048);
        let (reply_tx, reply_rx) = oneshot::channel();

        self.tx
            .send(Command::Prompt {
                session_id: session_id.to_string(),
                text: text.to_string(),
                event_tx,
                reply: reply_tx,
            })
            .await
            .map_err(|_| anyhow::anyhow!("Backend worker closed"))?;

        // Wait for the backend to acknowledge the prompt started
        reply_rx
            .await
            .map_err(|_| anyhow::anyhow!("Backend worker dropped reply channel"))??;

        Ok(EventReceiver::new(event_rx))
    }

    /// Cancel an in-progress prompt
    pub async fn cancel(&self, session_id: &str) -> Result<()> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.tx
            .send(Command::Cancel {
                session_id: session_id.to_string(),
                reply: reply_tx,
            })
            .await
            .map_err(|_| anyhow::anyhow!("Backend worker closed"))?;
        reply_rx
            .await
            .map_err(|_| anyhow::anyhow!("Backend worker dropped reply channel"))?
    }
}

/// Receiver for streaming events from a prompt.
///
/// This is `Send` so it can be passed across async task boundaries.
pub struct EventReceiver {
    rx: mpsc::Receiver<AgentEvent>,
}

impl EventReceiver {
    /// Create a new EventReceiver wrapping the given channel
    pub fn new(rx: mpsc::Receiver<AgentEvent>) -> Self {
        Self { rx }
    }

    /// Receive the next event, or None if the stream is closed
    pub async fn recv(&mut self) -> Option<AgentEvent> {
        self.rx.recv().await
    }

    /// Try to receive an event without blocking
    pub fn try_recv(&mut self) -> Option<AgentEvent> {
        self.rx.try_recv().ok()
    }
}
```

**Step 4: Update lib.rs exports**

Update `gorp-agent/src/lib.rs`:

```rust
// ABOUTME: Pluggable agent backend abstraction for gorp.
// ABOUTME: Provides trait-based backends (ACP, direct CLI, mock) with Send+Sync handles.

pub mod event;
pub mod traits;
pub mod handle;
pub mod registry;

pub mod backends;
pub mod testing;

pub use event::{AgentEvent, ErrorCode, Usage};
pub use traits::AgentBackend;
pub use handle::{AgentHandle, EventReceiver};
// registry export will be added when implemented
```

**Step 5: Run tests to verify they pass**

Run: `cd gorp-agent && cargo test handle_tests`
Expected: All tests pass

**Step 6: Commit**

```bash
git add gorp-agent/
git commit -m "feat(gorp-agent): implement AgentHandle and EventReceiver"
```

---

## Phase 2: Backends (Parallel)

> **These three tasks can run in parallel after Phase 1 completes.**

### Task 5a: ACP Backend

**Files:**
- Create: `gorp-agent/src/backends/acp.rs`
- Reference: `src/acp_client.rs` (existing gorp code)

**Step 1: Write failing test**

Create `gorp-agent/tests/acp_backend_tests.rs`:

```rust
#[cfg(feature = "acp")]
mod acp_tests {
    use gorp_agent::backends::acp::AcpBackend;
    use gorp_agent::AgentBackend;

    #[test]
    fn test_acp_backend_name() {
        // We can't test much without a real ACP binary, but we can test the name
        // This test validates the struct exists and implements the trait
    }

    #[test]
    fn test_acp_config_deserializes() {
        use gorp_agent::backends::acp::AcpConfig;
        let json = serde_json::json!({
            "binary": "codex-acp",
            "timeout_secs": 300,
            "working_dir": "/tmp"
        });
        let config: AcpConfig = serde_json::from_value(json).unwrap();
        assert_eq!(config.binary, "codex-acp");
        assert_eq!(config.timeout_secs, 300);
    }
}
```

**Step 2: Implement ACP backend**

This task requires porting code from `src/acp_client.rs`. The subagent should:

1. Read existing `src/acp_client.rs` (889 lines)
2. Extract the core ACP logic
3. Wrap it to implement `AgentBackend` trait
4. Handle the `!Send` futures via a worker task

Create `gorp-agent/src/backends/acp.rs`:

```rust
// ABOUTME: ACP protocol backend - communicates with claude-code-acp or codex-acp.
// ABOUTME: Wraps agent-client-protocol crate, handles !Send futures via worker task.

#[cfg(feature = "acp")]
use crate::event::{AgentEvent, ErrorCode, Usage};
#[cfg(feature = "acp")]
use crate::handle::{AgentHandle, Command};
#[cfg(feature = "acp")]
use crate::traits::AgentBackend;

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Configuration for the ACP backend
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcpConfig {
    /// Path to the ACP binary (codex-acp or claude-code-acp)
    pub binary: String,
    /// Timeout in seconds for prompts
    pub timeout_secs: u64,
    /// Working directory for the agent
    pub working_dir: PathBuf,
}

#[cfg(feature = "acp")]
pub struct AcpBackend {
    config: AcpConfig,
    // The actual ACP client internals go here
    // This will be ported from src/acp_client.rs
}

#[cfg(feature = "acp")]
impl AcpBackend {
    /// Create a new ACP backend with the given config
    pub fn new(config: AcpConfig) -> anyhow::Result<Self> {
        Ok(Self { config })
    }

    /// Create an AgentHandle that communicates with this backend
    ///
    /// This spawns a worker task that handles the !Send ACP futures.
    pub fn into_handle(self) -> AgentHandle {
        use tokio::sync::mpsc;

        let (tx, mut rx) = mpsc::channel::<Command>(32);
        let name = "acp";

        // Spawn worker task
        // NOTE: In the real implementation, this needs to use LocalSet
        // for !Send futures. See src/acp_client.rs for the pattern.
        tokio::spawn(async move {
            while let Some(cmd) = rx.recv().await {
                match cmd {
                    Command::NewSession { reply } => {
                        // TODO: Implement using ACP client
                        let _ = reply.send(Ok("session-placeholder".to_string()));
                    }
                    Command::LoadSession { reply, .. } => {
                        let _ = reply.send(Ok(()));
                    }
                    Command::Prompt { reply, event_tx, .. } => {
                        // Acknowledge prompt started
                        let _ = reply.send(Ok(()));
                        // TODO: Stream events from ACP
                        let _ = event_tx
                            .send(AgentEvent::Result {
                                text: "ACP response".to_string(),
                                usage: None,
                                metadata: serde_json::json!({}),
                            })
                            .await;
                    }
                    Command::Cancel { reply, .. } => {
                        let _ = reply.send(Ok(()));
                    }
                }
            }
        });

        AgentHandle::new(tx, name)
    }

    /// Factory function for the registry
    pub fn factory() -> crate::registry::BackendFactory {
        Box::new(|config| {
            let cfg: AcpConfig = serde_json::from_value(config.clone())?;
            let backend = AcpBackend::new(cfg)?;
            Ok(backend.into_handle())
        })
    }
}
```

> **NOTE TO SUBAGENT:** The full ACP implementation requires porting ~800 lines from
> `src/acp_client.rs`. Key elements to port:
> - AcpClientHandler implementing acp::Client trait
> - Session management (new_session, load_session)
> - Event translation from ACP to AgentEvent
> - LocalSet pattern for !Send futures
> - File I/O callbacks

**Step 3: Update backends/mod.rs**

```rust
// ABOUTME: Backend implementations (ACP, direct CLI, mock).
// ABOUTME: Each backend implements AgentBackend trait.

pub mod mock;
pub mod direct_cli;

#[cfg(feature = "acp")]
pub mod acp;
```

**Step 4: Run tests**

Run: `cd gorp-agent && cargo test --features acp acp_backend`
Expected: Tests pass (basic structure works)

**Step 5: Commit**

```bash
git add gorp-agent/
git commit -m "feat(gorp-agent): add ACP backend skeleton"
```

---

### Task 5b: Direct CLI Backend

**Files:**
- Modify: `gorp-agent/src/backends/direct_cli.rs`
- Reference: Git history for old `src/claude.rs`

**Step 1: Write failing test**

Create `gorp-agent/tests/direct_cli_tests.rs`:

```rust
use gorp_agent::backends::direct_cli::{DirectCliBackend, DirectCliConfig};

#[test]
fn test_direct_cli_config_deserializes() {
    let json = serde_json::json!({
        "binary": "claude",
        "sdk_url": "http://localhost:8080",
        "working_dir": "/tmp"
    });
    let config: DirectCliConfig = serde_json::from_value(json).unwrap();
    assert_eq!(config.binary, "claude");
    assert_eq!(config.sdk_url, Some("http://localhost:8080".to_string()));
}

#[test]
fn test_direct_cli_config_without_sdk_url() {
    let json = serde_json::json!({
        "binary": "claude",
        "working_dir": "/tmp"
    });
    let config: DirectCliConfig = serde_json::from_value(json).unwrap();
    assert!(config.sdk_url.is_none());
}
```

**Step 2: Implement Direct CLI backend**

Replace `gorp-agent/src/backends/direct_cli.rs`:

```rust
// ABOUTME: Direct CLI backend - spawns claude with --print --output-format stream-json.
// ABOUTME: Parses streaming JSONL from stdout, emits AgentEvents.

use crate::event::{AgentEvent, ErrorCode, Usage};
use crate::handle::{AgentHandle, Command};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command as ProcessCommand;
use tokio::sync::mpsc;

/// Configuration for the Direct CLI backend
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectCliConfig {
    /// Path to the claude binary
    pub binary: String,
    /// Optional SDK URL for the claude CLI
    pub sdk_url: Option<String>,
    /// Working directory for the agent
    pub working_dir: PathBuf,
}

pub struct DirectCliBackend {
    config: DirectCliConfig,
}

impl DirectCliBackend {
    pub fn new(config: DirectCliConfig) -> Result<Self> {
        Ok(Self { config })
    }

    pub fn into_handle(self) -> AgentHandle {
        let (tx, mut rx) = mpsc::channel::<Command>(32);
        let name = "direct";
        let config = self.config;

        tokio::spawn(async move {
            while let Some(cmd) = rx.recv().await {
                match cmd {
                    Command::NewSession { reply } => {
                        // Direct CLI doesn't have persistent sessions by default
                        // Generate a UUID for tracking
                        let session_id = uuid::Uuid::new_v4().to_string();
                        let _ = reply.send(Ok(session_id));
                    }
                    Command::LoadSession { reply, .. } => {
                        // Direct CLI can use --resume with session ID
                        let _ = reply.send(Ok(()));
                    }
                    Command::Prompt {
                        session_id,
                        text,
                        event_tx,
                        reply,
                    } => {
                        let _ = reply.send(Ok(()));
                        if let Err(e) = run_prompt(&config, &session_id, &text, event_tx).await {
                            tracing::error!(error = %e, "Direct CLI prompt failed");
                        }
                    }
                    Command::Cancel { reply, .. } => {
                        // TODO: Kill the running process
                        let _ = reply.send(Ok(()));
                    }
                }
            }
        });

        AgentHandle::new(tx, name)
    }

    pub fn factory() -> crate::registry::BackendFactory {
        Box::new(|config| {
            let cfg: DirectCliConfig = serde_json::from_value(config.clone())?;
            let backend = DirectCliBackend::new(cfg)?;
            Ok(backend.into_handle())
        })
    }
}

async fn run_prompt(
    config: &DirectCliConfig,
    session_id: &str,
    text: &str,
    event_tx: mpsc::Sender<AgentEvent>,
) -> Result<()> {
    let mut args = vec![
        "--print".to_string(),
        "--output-format".to_string(),
        "stream-json".to_string(),
        "--verbose".to_string(),
        "--dangerously-skip-permissions".to_string(),
        "--resume".to_string(),
        session_id.to_string(),
    ];

    if let Some(ref url) = config.sdk_url {
        args.push("--sdk-url".to_string());
        args.push(url.clone());
    }

    args.push(text.to_string());

    tracing::debug!(?args, "Spawning Claude CLI");

    let mut child = ProcessCommand::new(&config.binary)
        .args(&args)
        .current_dir(&config.working_dir)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .context("Failed to spawn Claude CLI")?;

    let stdout = child.stdout.take().context("Failed to capture stdout")?;
    let reader = BufReader::new(stdout);
    let mut lines = reader.lines();

    while let Ok(Some(line)) = lines.next_line().await {
        if line.is_empty() {
            continue;
        }

        if let Ok(json) = serde_json::from_str::<Value>(&line) {
            if let Some(event) = parse_cli_event(&json) {
                if event_tx.send(event).await.is_err() {
                    break;
                }
            }
        }
    }

    let status = child.wait().await?;
    if !status.success() {
        let _ = event_tx
            .send(AgentEvent::Error {
                code: ErrorCode::BackendError,
                message: format!("CLI exited with status: {:?}", status.code()),
                recoverable: false,
            })
            .await;
    }

    Ok(())
}

fn parse_cli_event(json: &Value) -> Option<AgentEvent> {
    let event_type = json.get("type")?.as_str()?;

    match event_type {
        "assistant" => {
            if let Some(content) = json
                .get("message")
                .and_then(|m| m.get("content"))
                .and_then(|c| c.as_array())
            {
                for item in content {
                    let item_type = item.get("type").and_then(|t| t.as_str());

                    if item_type == Some("tool_use") {
                        let name = item
                            .get("name")
                            .and_then(|n| n.as_str())
                            .unwrap_or("unknown")
                            .to_string();
                        let id = item
                            .get("id")
                            .and_then(|i| i.as_str())
                            .unwrap_or("unknown")
                            .to_string();
                        let input = item.get("input").cloned().unwrap_or(Value::Null);

                        return Some(AgentEvent::ToolStart { id, name, input });
                    } else if item_type == Some("text") {
                        if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
                            return Some(AgentEvent::Text(text.to_string()));
                        }
                    }
                }
            }
            None
        }
        "result" => {
            let is_error = json
                .get("is_error")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            if is_error {
                let message = json
                    .get("error")
                    .and_then(|e| e.as_str())
                    .unwrap_or("Unknown error")
                    .to_string();
                Some(AgentEvent::Error {
                    code: ErrorCode::BackendError,
                    message,
                    recoverable: false,
                })
            } else {
                let text = json
                    .get("result")
                    .and_then(|r| r.as_str())
                    .unwrap_or("")
                    .to_string();

                let usage = extract_usage(json);

                Some(AgentEvent::Result {
                    text,
                    usage,
                    metadata: json.clone(),
                })
            }
        }
        _ => None,
    }
}

fn extract_usage(json: &Value) -> Option<Usage> {
    let usage_obj = json.get("usage")?;

    Some(Usage {
        input_tokens: usage_obj
            .get("input_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0),
        output_tokens: usage_obj
            .get("output_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0),
        cache_read_tokens: usage_obj.get("cache_read_tokens").and_then(|v| v.as_u64()),
        cache_write_tokens: usage_obj
            .get("cache_creation_tokens")
            .and_then(|v| v.as_u64()),
        cost_usd: json.get("total_cost_usd").and_then(|v| v.as_f64()),
        extra: None,
    })
}
```

**Step 3: Add uuid dependency**

Update `gorp-agent/Cargo.toml` dependencies:

```toml
uuid = { version = "1", features = ["v4"] }
```

**Step 4: Run tests**

Run: `cd gorp-agent && cargo test direct_cli`
Expected: Tests pass

**Step 5: Commit**

```bash
git add gorp-agent/
git commit -m "feat(gorp-agent): implement Direct CLI backend"
```

---

### Task 5c: Mock Backend

**Files:**
- Modify: `gorp-agent/src/backends/mock.rs`
- Create: `gorp-agent/tests/mock_backend_tests.rs`

**Step 1: Write failing test**

Create `gorp-agent/tests/mock_backend_tests.rs`:

```rust
use gorp_agent::backends::mock::MockBackend;
use gorp_agent::AgentEvent;
use serde_json::json;

#[tokio::test]
async fn test_mock_backend_returns_configured_response() {
    let mock = MockBackend::new()
        .on_prompt("hello")
        .respond_text("Hi there!");

    let handle = mock.into_handle();
    let session_id = handle.new_session().await.unwrap();

    let mut receiver = handle.prompt(&session_id, "hello").await.unwrap();

    let event = receiver.recv().await.unwrap();
    match event {
        AgentEvent::Result { text, .. } => assert_eq!(text, "Hi there!"),
        _ => panic!("Expected Result event"),
    }
}

#[tokio::test]
async fn test_mock_backend_tool_response() {
    let mock = MockBackend::new()
        .on_prompt("read file")
        .respond_with(vec![
            AgentEvent::ToolStart {
                id: "t1".to_string(),
                name: "Read".to_string(),
                input: json!({"path": "/tmp/foo"}),
            },
            AgentEvent::ToolEnd {
                id: "t1".to_string(),
                name: "Read".to_string(),
                output: json!({"content": "file contents"}),
                success: true,
                duration_ms: 10,
            },
            AgentEvent::Result {
                text: "Read the file".to_string(),
                usage: None,
                metadata: json!({}),
            },
        ]);

    let handle = mock.into_handle();
    let session_id = handle.new_session().await.unwrap();
    let mut receiver = handle.prompt(&session_id, "read file").await.unwrap();

    let events: Vec<_> = std::iter::from_fn(|| receiver.try_recv()).collect();
    // Give async time to process
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    let mut receiver = handle.prompt(&session_id, "read file").await.unwrap();
    let mut events = vec![];
    while let Some(e) = receiver.recv().await {
        events.push(e);
    }

    assert_eq!(events.len(), 3);
    assert!(matches!(&events[0], AgentEvent::ToolStart { name, .. } if name == "Read"));
    assert!(matches!(&events[1], AgentEvent::ToolEnd { success: true, .. }));
    assert!(matches!(&events[2], AgentEvent::Result { .. }));
}
```

**Step 2: Implement Mock backend**

Replace `gorp-agent/src/backends/mock.rs`:

```rust
// ABOUTME: Mock backend for testing - returns pre-configured responses.
// ABOUTME: Allows deterministic tests without spawning real agent processes.

use crate::event::AgentEvent;
use crate::handle::{AgentHandle, Command};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

/// Mock backend for testing
pub struct MockBackend {
    expectations: Arc<Mutex<VecDeque<Expectation>>>,
}

struct Expectation {
    pattern: String,
    events: Vec<AgentEvent>,
}

impl MockBackend {
    pub fn new() -> Self {
        Self {
            expectations: Arc::new(Mutex::new(VecDeque::new())),
        }
    }

    /// Set up an expectation for a prompt matching the given pattern
    pub fn on_prompt(self, pattern: &str) -> ExpectationBuilder {
        ExpectationBuilder {
            backend: self,
            pattern: pattern.to_string(),
        }
    }

    pub fn into_handle(self) -> AgentHandle {
        let (tx, mut rx) = mpsc::channel::<Command>(32);
        let name = "mock";
        let expectations = self.expectations;

        tokio::spawn(async move {
            let mut session_counter = 0u64;

            while let Some(cmd) = rx.recv().await {
                match cmd {
                    Command::NewSession { reply } => {
                        session_counter += 1;
                        let _ = reply.send(Ok(format!("mock-session-{}", session_counter)));
                    }
                    Command::LoadSession { reply, .. } => {
                        let _ = reply.send(Ok(()));
                    }
                    Command::Prompt {
                        text,
                        event_tx,
                        reply,
                        ..
                    } => {
                        let _ = reply.send(Ok(()));

                        // Find matching expectation
                        let events = {
                            let mut exp = expectations.lock().unwrap();
                            exp.iter()
                                .position(|e| text.contains(&e.pattern))
                                .map(|i| exp.remove(i).unwrap().events)
                        };

                        if let Some(events) = events {
                            for event in events {
                                if event_tx.send(event).await.is_err() {
                                    break;
                                }
                            }
                        } else {
                            let _ = event_tx
                                .send(AgentEvent::Result {
                                    text: format!("Mock: no expectation for '{}'", text),
                                    usage: None,
                                    metadata: serde_json::json!({}),
                                })
                                .await;
                        }
                    }
                    Command::Cancel { reply, .. } => {
                        let _ = reply.send(Ok(()));
                    }
                }
            }
        });

        AgentHandle::new(tx, name)
    }

    pub fn factory() -> crate::registry::BackendFactory {
        Box::new(|_config| {
            let backend = MockBackend::new();
            Ok(backend.into_handle())
        })
    }
}

impl Default for MockBackend {
    fn default() -> Self {
        Self::new()
    }
}

/// Builder for setting up mock expectations
pub struct ExpectationBuilder {
    backend: MockBackend,
    pattern: String,
}

impl ExpectationBuilder {
    /// Respond with a list of events
    pub fn respond_with(self, events: Vec<AgentEvent>) -> MockBackend {
        self.backend
            .expectations
            .lock()
            .unwrap()
            .push_back(Expectation {
                pattern: self.pattern,
                events,
            });
        self.backend
    }

    /// Respond with a simple text result
    pub fn respond_text(self, text: &str) -> MockBackend {
        self.respond_with(vec![AgentEvent::Result {
            text: text.to_string(),
            usage: None,
            metadata: serde_json::json!({}),
        }])
    }

    /// Respond with an error
    pub fn respond_error(self, code: crate::event::ErrorCode, message: &str) -> MockBackend {
        self.respond_with(vec![AgentEvent::Error {
            code,
            message: message.to_string(),
            recoverable: false,
        }])
    }
}
```

**Step 3: Run tests**

Run: `cd gorp-agent && cargo test mock_backend`
Expected: Tests pass

**Step 4: Commit**

```bash
git add gorp-agent/
git commit -m "feat(gorp-agent): implement Mock backend with expectation builder"
```

---

## Phase 3: Testing Infrastructure (Parallel)

> **These three tasks can run in parallel after Phase 2 completes.**

### Task 6a: Enhanced MockAgent Builder

**Files:**
- Modify: `gorp-agent/src/testing/mock_builder.rs`

This extends the mock backend with more sophisticated test helpers. See design doc for full API.

---

### Task 6b: Recording/Replay

**Files:**
- Modify: `gorp-agent/src/testing/recording.rs`

Implement `RecordingAgent<T>` wrapper and `ReplayAgent` for transcript-based testing.

---

### Task 6c: Scenario Test Runner

**Files:**
- Modify: `gorp-agent/src/testing/scenarios.rs`
- Create: `gorp-agent/scenarios/basic/simple_prompt.json`

Implement the scenario runner and create baseline scenarios.

---

## Phase 4: Integration (Sequential)

### Task 7: Registry Implementation

**Files:**
- Modify: `gorp-agent/src/registry.rs`

**Step 1: Write failing test**

Create `gorp-agent/tests/registry_tests.rs`:

```rust
use gorp_agent::registry::AgentRegistry;
use serde_json::json;

#[tokio::test]
async fn test_registry_creates_mock_backend() {
    let registry = AgentRegistry::default();
    let handle = registry.create("mock", &json!({})).unwrap();
    assert_eq!(handle.name(), "mock");
}

#[tokio::test]
async fn test_registry_lists_available_backends() {
    let registry = AgentRegistry::default();
    let available = registry.available();
    assert!(available.contains(&"mock"));
}

#[test]
fn test_registry_unknown_backend_errors() {
    let registry = AgentRegistry::default();
    let result = registry.create("nonexistent", &json!({}));
    assert!(result.is_err());
}
```

**Step 2: Implement registry**

Replace `gorp-agent/src/registry.rs`:

```rust
// ABOUTME: Registry pattern for runtime backend selection.
// ABOUTME: Backends register factories, gorp creates by name from config.

use crate::handle::AgentHandle;
use anyhow::{anyhow, Result};
use serde_json::Value;
use std::collections::HashMap;

/// Factory function that creates an AgentHandle from config
pub type BackendFactory = Box<dyn Fn(&Value) -> Result<AgentHandle> + Send + Sync>;

/// Registry for runtime backend selection
pub struct AgentRegistry {
    factories: HashMap<String, BackendFactory>,
}

impl AgentRegistry {
    /// Create a new empty registry
    pub fn new() -> Self {
        Self {
            factories: HashMap::new(),
        }
    }

    /// Register a backend factory by name
    pub fn register<F>(mut self, name: &str, factory: F) -> Self
    where
        F: Fn(&Value) -> Result<AgentHandle> + Send + Sync + 'static,
    {
        self.factories.insert(name.to_string(), Box::new(factory));
        self
    }

    /// Create a backend by name with the given config
    pub fn create(&self, name: &str, config: &Value) -> Result<AgentHandle> {
        let factory = self
            .factories
            .get(name)
            .ok_or_else(|| anyhow!("Unknown backend: {}", name))?;
        factory(config)
    }

    /// List available backend names
    pub fn available(&self) -> Vec<&str> {
        self.factories.keys().map(|s| s.as_str()).collect()
    }
}

impl Default for AgentRegistry {
    fn default() -> Self {
        use crate::backends::mock::MockBackend;
        use crate::backends::direct_cli::DirectCliBackend;

        let mut registry = Self::new()
            .register("mock", MockBackend::factory())
            .register("direct", DirectCliBackend::factory());

        #[cfg(feature = "acp")]
        {
            use crate::backends::acp::AcpBackend;
            registry = registry.register("acp", AcpBackend::factory());
        }

        registry
    }
}
```

**Step 3: Update lib.rs exports**

Update `gorp-agent/src/lib.rs`:

```rust
// ABOUTME: Pluggable agent backend abstraction for gorp.
// ABOUTME: Provides trait-based backends (ACP, direct CLI, mock) with Send+Sync handles.

pub mod event;
pub mod traits;
pub mod handle;
pub mod registry;

pub mod backends;
pub mod testing;

pub use event::{AgentEvent, ErrorCode, Usage};
pub use traits::AgentBackend;
pub use handle::{AgentHandle, EventReceiver};
pub use registry::{AgentRegistry, BackendFactory};
```

**Step 4: Run tests**

Run: `cd gorp-agent && cargo test registry`
Expected: Tests pass

**Step 5: Commit**

```bash
git add gorp-agent/
git commit -m "feat(gorp-agent): implement AgentRegistry with factory pattern"
```

---

### Task 8: Gorp Integration

**Files:**
- Modify: `Cargo.toml` (add gorp-agent dependency)
- Modify: `src/config.rs` (add backend selection)
- Modify: `src/warm_session.rs` (use AgentHandle)
- Modify: `src/message_handler.rs` (use AgentEvent)
- Delete: `src/acp_client.rs` (moved to gorp-agent)

This is a large integration task. The subagent should:

1. Add gorp-agent as a path dependency
2. Update Config to include `agent_backend` field
3. Replace `AcpClient` usage with `AgentHandle`
4. Update event handling to use `AgentEvent`
5. Remove the old `acp_client.rs` module

---

### Task 9: Scenario Test Suite

**Files:**
- Create: `gorp-agent/scenarios/basic/*.json`
- Create: `gorp-agent/scenarios/tools/internal/*.json`
- Create: `gorp-agent/scenarios/mcp/*.json`

Build out the comprehensive scenario test suite as described in the design doc.

---

## Summary

| Phase | Tasks | Parallelizable | Estimated Complexity |
|-------|-------|----------------|---------------------|
| 1 | 1-4 | No (sequential) | Medium |
| 2 | 5a, 5b, 5c | Yes (3 parallel) | High (5a), Medium (5b, 5c) |
| 3 | 6a, 6b, 6c | Yes (3 parallel) | Medium |
| 4 | 7-9 | No (sequential) | High (8), Medium (7, 9) |

**Recommended swarm allocation:**
- 1 agent for Phase 1 (foundation must be sequential)
- 3 agents for Phase 2 (one per backend)
- 3 agents for Phase 3 (one per testing component)
- 1 agent for Phase 4 (integration must be sequential)
