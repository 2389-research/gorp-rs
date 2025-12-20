# Pluggable Agent Backend Design

## Overview

Extract agent communication into a separate `gorp-agent` crate with a pluggable backend architecture. This enables:

- **Testing**: Mock/fake agents for fast, deterministic tests
- **Flexibility**: Runtime backend switching via config
- **Vendor diversity**: Support ACP, direct CLI, HTTP APIs, local LLMs

## Architecture

### Crate Structure

```
gorp-agent/
├── Cargo.toml
├── src/
│   ├── lib.rs              # Public API exports
│   ├── event.rs            # AgentEvent, Usage, ErrorCode
│   ├── traits.rs           # AgentBackend trait, AgentHandle
│   ├── registry.rs         # AgentRegistry, factory pattern
│   ├── backends/
│   │   ├── mod.rs
│   │   ├── acp.rs          # ACP protocol backend
│   │   ├── direct_cli.rs   # Direct claude CLI backend
│   │   └── mock.rs         # Mock for testing
│   ├── testing/
│   │   ├── mod.rs
│   │   ├── mock.rs         # MockAgent builder
│   │   ├── recording.rs    # RecordingAgent wrapper
│   │   ├── replay.rs       # ReplayAgent from transcripts
│   │   └── scenarios.rs    # Scenario runner
│   └── handle.rs           # AgentHandle (Send+Sync wrapper)
├── scenarios/
│   ├── basic/              # Simple prompt/response
│   ├── sessions/           # Session lifecycle
│   ├── errors/             # Error handling
│   ├── tools/
│   │   ├── internal/       # Read, Write, Edit, Bash, etc.
│   │   ├── chained/        # Multi-tool sequences
│   │   └── failures/       # Tool error scenarios
│   └── mcp/
│       ├── discovery/      # List tools/resources
│       ├── tools/          # MCP tool invocation
│       ├── resources/      # MCP resource access
│       └── failures/       # MCP error scenarios
└── tests/
    ├── acp_integration.rs
    ├── direct_cli_integration.rs
    └── scenario_tests.rs
```

### Dependencies

- `tokio` (runtime, channels)
- `async-trait`
- `serde`, `serde_json` (event serialization)
- `tracing` (observability)
- `agent-client-protocol` (optional feature for ACP)

## Core Types

### AgentBackend Trait

```rust
/// The core trait backends implement (may be !Send internally)
pub trait AgentBackend {
    /// Create a new session, returns session ID
    fn new_session(&self) -> BoxFuture<'_, Result<String>>;

    /// Load/resume an existing session
    fn load_session(&self, session_id: &str) -> BoxFuture<'_, Result<()>>;

    /// Send a prompt, returns event stream
    fn prompt(
        &self,
        session_id: &str,
        text: &str,
    ) -> BoxFuture<'_, Result<EventStream>>;

    /// Cancel an in-progress prompt
    fn cancel(&self, session_id: &str) -> BoxFuture<'_, Result<()>>;

    /// Backend name for logging/metrics
    fn name(&self) -> &'static str;
}

pub type EventStream = Pin<Box<dyn Stream<Item = AgentEvent> + Send>>;
```

### AgentHandle (Send + Sync Wrapper)

```rust
/// Send + Sync wrapper that gorp interacts with
/// Internally manages LocalSet/worker thread if needed
pub struct AgentHandle {
    tx: mpsc::Sender<Command>,
    name: &'static str,
}

impl AgentHandle {
    pub async fn new_session(&self) -> Result<String>;
    pub async fn load_session(&self, session_id: &str) -> Result<()>;
    pub async fn prompt(&self, session_id: &str, text: &str) -> Result<EventReceiver>;
    pub async fn cancel(&self, session_id: &str) -> Result<()>;
    pub fn name(&self) -> &'static str;
}

pub struct EventReceiver {
    rx: mpsc::Receiver<AgentEvent>,
}
```

The key insight: `AgentBackend` trait can be `!Send`, but `AgentHandle` is always `Send + Sync`. The handle communicates with a worker that runs the actual backend. This solves the ACP `LocalSet` problem.

### AgentEvent

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AgentEvent {
    /// Streaming text chunk for real-time display
    Text(String),

    /// Tool started execution
    ToolStart {
        id: String,
        name: String,
        input: Value,
    },

    /// Tool progress update (backend-specific)
    ToolProgress {
        id: String,
        update: Value,
    },

    /// Tool completed
    ToolEnd {
        id: String,
        name: String,
        output: Value,
        success: bool,
        duration_ms: u64,
    },

    /// Final result with optional usage stats
    Result {
        text: String,
        usage: Option<Usage>,
        metadata: Value,
    },

    /// Error occurred
    Error {
        code: ErrorCode,
        message: String,
        recoverable: bool,
    },

    /// Session invalid, needs recreation
    SessionInvalid { reason: String },

    /// Backend forced new session
    SessionChanged { new_session_id: String },

    /// Backend-specific event for extensibility
    Custom {
        kind: String,
        payload: Value,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ErrorCode {
    Timeout,
    RateLimited,
    AuthFailed,
    SessionOrphaned,
    ToolFailed,
    PermissionDenied,
    BackendError,
    Unknown,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: Option<u64>,
    pub cache_write_tokens: Option<u64>,
    pub cost_usd: Option<f64>,
    pub extra: Option<Value>,
}
```

### Registry Pattern

```rust
pub type BackendFactory = Box<dyn Fn(&Value) -> Result<AgentHandle> + Send + Sync>;

pub struct AgentRegistry {
    factories: HashMap<String, BackendFactory>,
}

impl AgentRegistry {
    pub fn new() -> Self;
    pub fn register<F>(mut self, name: &str, factory: F) -> Self
    where
        F: Fn(&Value) -> Result<AgentHandle> + Send + Sync + 'static;
    pub fn create(&self, name: &str, config: &Value) -> Result<AgentHandle>;
    pub fn available(&self) -> Vec<&str>;
}

impl Default for AgentRegistry {
    fn default() -> Self {
        Self::new()
            .register("acp", AcpBackend::factory())
            .register("direct", DirectCliBackend::factory())
            .register("mock", MockBackend::factory())
    }
}
```

## Testing Infrastructure

### MockAgent

```rust
pub struct MockAgent {
    expectations: VecDeque<Expectation>,
}

impl MockAgent {
    pub fn new() -> Self;
    pub fn on_prompt(mut self, pattern: &str) -> ExpectationBuilder;
}

impl ExpectationBuilder {
    pub fn respond_with(self, events: Vec<AgentEvent>) -> MockAgent;
    pub fn respond_text(self, text: &str) -> MockAgent;
    pub fn respond_error(self, code: ErrorCode, msg: &str) -> MockAgent;
    pub fn respond_with_tools(self, tools: Vec<ToolCall>, result: &str) -> MockAgent;
}
```

### Recording/Replay

```rust
pub struct RecordingAgent<T: AgentBackend> {
    inner: T,
    transcript: Arc<Mutex<Vec<Interaction>>>,
}

pub struct ReplayAgent {
    transcript: Vec<Interaction>,
}
```

### Scenario Testing

```rust
#[derive(Debug, Serialize, Deserialize)]
pub struct Scenario {
    pub name: String,
    pub description: Option<String>,
    pub setup: Option<ScenarioSetup>,
    pub prompt: String,
    pub expected_events: Vec<EventMatcher>,
    pub assertions: Option<ScenarioAssertions>,
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ScenarioSetup {
    pub files: Option<HashMap<String, String>>,
    pub mcp_servers: Option<Vec<String>>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum EventMatcher {
    Text { contains: String },
    ToolStart { name: String, input_contains: Option<Value> },
    ToolEnd { name: String, success: bool },
    Result { contains: String },
    Error { code: Option<ErrorCode> },
    Custom { kind: String },
    Any { count: usize },
}

pub async fn run_scenarios(
    handle: &AgentHandle,
    scenarios_dir: &Path,
) -> ScenarioReport;
```

Example scenario:

```json
{
  "name": "read_then_edit_file",
  "description": "Agent reads a file, then edits it",
  "setup": {
    "files": {
      "/tmp/test.txt": "original content"
    }
  },
  "prompt": "Read /tmp/test.txt and add a header",
  "expected_events": [
    {"type": "ToolStart", "name": "Read", "input_contains": {"path": "/tmp/test.txt"}},
    {"type": "ToolEnd", "name": "Read", "success": true},
    {"type": "ToolStart", "name": "Edit"},
    {"type": "ToolEnd", "name": "Edit", "success": true},
    {"type": "Result", "contains": "added header"}
  ],
  "assertions": {
    "files": {
      "/tmp/test.txt": {"contains": "header"}
    }
  }
}
```

## Integration with Gorp

### Config Changes

```toml
# config.toml
agent_backend = "acp"  # or "direct", "mock"

[acp]
agent_binary = "codex-acp"
timeout_secs = 300

[direct]
binary = "claude"
sdk_url = "http://localhost:8080"
```

### WarmSessionManager Updates

```rust
pub struct WarmSession {
    pub handle: AgentHandle,  // was: AcpClient
    pub session_id: String,
    pub last_used: Instant,
}

pub struct WarmSessionManager {
    registry: AgentRegistry,
    backend_name: String,
    sessions: HashMap<String, Arc<Mutex<WarmSession>>>,
}
```

### What Moves to gorp-agent

- `src/acp_client.rs` (889 lines) → `gorp-agent/src/backends/acp.rs`
- Old `src/claude.rs` logic → `gorp-agent/src/backends/direct_cli.rs`

### What Stays in Gorp

- `WarmSessionManager` (session lifecycle, per-channel management)
- Matrix integration
- MCP server
- All orchestration logic

## Migration Path

1. Create `gorp-agent` crate with trait definitions
2. Move `AcpClient` to `gorp-agent/src/backends/acp.rs`, implement trait
3. Implement `AgentHandle` wrapper for `Send + Sync` safety
4. Update gorp to depend on `gorp-agent`, use `AgentHandle`
5. Port old `claude.rs` to `DirectCliBackend`
6. Add `MockBackend` for testing
7. Build out scenario test suite
8. Add recording/replay infrastructure

## Adding New Backends

```rust
pub struct OpenAIAssistantsBackend { /* ... */ }

impl AgentBackend for OpenAIAssistantsBackend {
    fn name(&self) -> &'static str { "openai-assistants" }
    // ... implement methods
}

impl OpenAIAssistantsBackend {
    pub fn factory() -> BackendFactory {
        Box::new(|config| {
            let cfg: OpenAIConfig = serde_json::from_value(config.clone())?;
            let backend = OpenAIAssistantsBackend::new(cfg)?;
            Ok(AgentHandle::wrap(backend))
        })
    }
}

// Register:
let registry = AgentRegistry::default()
    .register("openai-assistants", OpenAIAssistantsBackend::factory());
```

## Observability

Built-in `tracing` instrumentation:

```rust
#[tracing::instrument(skip(self, text))]
async fn prompt(&self, session_id: &str, text: &str) -> Result<EventStream> {
    tracing::info!(backend = self.name(), session_id, prompt_len = text.len(), "sending prompt");
    // ...
}
```

Gorp controls verbosity via tracing subscriber configuration.
