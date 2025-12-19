# ACP Migration Design

Replace Claude CLI spawning with ACP (Agent Client Protocol) for cleaner architecture, agent flexibility, and future-proofing.

## Goals

1. **Agent flexibility** — Support multiple ACP-compliant agents (not just Claude)
2. **Cleaner architecture** — Typed protocol instead of CLI stdout parsing
3. **Future-proofing** — ACP is becoming the standard for AI agent integration

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                        gorp-acp                             │
│                                                             │
│  Matrix Client ──→ Message Handler ──→ ACP Client ──────────┼──→ claude-code-acp
│       ↑                   │                │                │         (stdio)
│       │                   ↓                ↓                │
│       └────────────── Session DB     Process Pool           │
│                      (unchanged)     (spawn per channel)    │
└─────────────────────────────────────────────────────────────┘
```

**What changes:**
- `claude.rs` deleted, replaced with `acp_client.rs`
- Uses `agent-client-protocol` Rust crate
- Spawns `claude-code-acp` (npm package) per channel

**What stays the same:**
- Matrix client, webhook server, session DB, admin UI, MCP config
- One process per channel model
- Working directory per channel

## Protocol Mapping

| Current (claude.rs) | New (ACP) |
|---------------------|-----------|
| `--session X` flag | `LoadSessionRequest { session_id }` |
| New session | `NewSessionRequest` |
| Prompt string | `PromptRequest { content: [TextContent] }` |
| `ClaudeEvent::ToolUse` | `SessionUpdate` with `ToolCall` |
| `ClaudeEvent::Result` | `SessionUpdate` with final `ContentBlock` |
| `ClaudeEvent::Error` | `SessionUpdate` with error or connection close |
| `ClaudeUsage` stats | `SessionUpdate` metadata (if ACP exposes it) |

## Permissions

Auto-approve all tool permissions to match current `--dangerously-skip-permissions` behavior:

```rust
// When we receive a tool permission request
SessionUpdate::ToolCall { id, permission_request: Some(_), .. } => {
    connection.send(ClientResponse::ToolCallApproval {
        tool_call_id: id,
        approved: true
    }).await?;
}
```

Environment variable when spawning:
```
CLAUDE_CODE_SKIP_PERMISSIONS=true claude-code-acp
```

## Error Handling

| Scenario | Behavior |
|----------|----------|
| Agent process crashes | stdio close triggers cleanup, report error to Matrix |
| Orphaned session | `LoadSessionRequest` returns error, create new session |
| Timeout | Kill process, report timeout to Matrix |
| API rate limit | `SessionUpdate` with error content block |
| ACP protocol errors | Log, report "Internal error" to Matrix |
| Initialization failure | Retry once, then report "Agent unavailable" |

## Implementation Plan

### Files to change

| File | Change |
|------|--------|
| `src/claude.rs` | **Delete** |
| `src/acp_client.rs` | **New** — ACP client implementation |
| `src/message_handler.rs` | Update to use `acp_client` |
| `Cargo.toml` | Add `agent-client-protocol` |
| `Dockerfile` | Add `npm install -g @anthropic/claude-code-acp` |
| `config.rs` | Replace `claude.binary_path` with `acp.agent_binary` |

### New module structure

```rust
pub struct AcpClient {
    child: Child,
    connection: ClientSideConnection,
    session_id: Option<SessionId>,
}

impl AcpClient {
    pub async fn spawn(working_dir: &Path) -> Result<Self>;
    pub async fn initialize(&mut self) -> Result<SessionCapabilities>;
    pub async fn new_session(&mut self) -> Result<SessionId>;
    pub async fn load_session(&mut self, id: SessionId) -> Result<()>;
    pub async fn prompt(&mut self, text: &str) -> Result<Receiver<AcpEvent>>;
}

pub enum AcpEvent {
    ToolUse { name: String, preview: String },
    Text(String),
    Result { text: String, usage: Option<Usage> },
    Error(String),
}
```

### Migration steps

1. Add `agent-client-protocol` to Cargo.toml
2. Write `acp_client.rs` with tests
3. Update `message_handler.rs` to use new client
4. Delete `claude.rs`
5. Update Dockerfile
6. Test end-to-end

## Testing Strategy

### Scenario tests (`.scratch/`, gitignored)

| Scenario | Validates |
|----------|-----------|
| `test-new-channel-flow.sh` | Fresh session creation, first prompt, response arrives |
| `test-session-persistence.sh` | Session ID stored, process restart, session resumes |
| `test-tool-use-events.sh` | Tool events stream to Matrix, final result delivered |
| `test-orphaned-session-recovery.sh` | Detects invalid session, creates new one |
| `test-agent-crash-recovery.sh` | Error reported to Matrix, channel remains usable |
| `test-permission-auto-approve.sh` | Permission request auto-approved, tool executes |

### Real dependencies (no mocks)

- Real `claude-code-acp` adapter
- Real Claude API
- Real SQLite DB
- Real file system

### Patterns to extract (`scenarios.jsonl`)

```jsonl
{"name": "new-session", "given": "fresh channel", "when": "first prompt sent", "then": "session created and response received", "validates": "ACP Initialize + NewSession flow"}
{"name": "session-resume", "given": "existing session ID in DB", "when": "prompt sent after restart", "then": "context preserved from previous conversation", "validates": "LoadSession with persisted ID"}
{"name": "tool-streaming", "given": "prompt requiring file read", "when": "agent uses Read tool", "then": "ToolUse event emitted before result", "validates": "SessionUpdate streaming"}
```

## Logging

Log ACP JSON-RPC messages to `.gorp/acp-messages.jsonl` (replaces `claude-messages.jsonl`).

## References

- [ACP Go SDK](https://github.com/coder/acp-go-sdk)
- [agent-client-protocol crate](https://crates.io/crates/agent-client-protocol)
- [claude-code-acp adapter](https://github.com/zed-industries/claude-code-acp)
- [Zed ACP blog post](https://zed.dev/blog/claude-code-via-acp)
- [Claude Code ACP feature request](https://github.com/anthropics/claude-code/issues/6686)
