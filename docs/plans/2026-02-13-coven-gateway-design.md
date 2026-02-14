# Coven-Gateway Agent Provider — Design Document

**Date:** 2026-02-13
**Status:** Draft
**Related:** [fold-project/coven-gateway](../../fold-project/coven-gateway)

## Summary

Expose gorp workspaces as coven-agents via gRPC bidirectional streaming to a coven-gateway instance. Each workspace registers as its own agent (e.g., `gorp-research`, `gorp-news`), plus an optional DISPATCH agent for control plane operations. This is fundamentally different from the platform integrations — platforms are inbound (users message gorp), while the coven provider is outbound (gorp exposes itself as agents that coven-gateway routes to).

## Decisions

| Decision | Choice | Rationale |
|---|---|---|
| Role | gorp as agent provider | Coven-gateway handles user-facing interfaces; gorp provides the agent backends |
| Agent model | One agent per workspace | Each workspace registers independently with its own capabilities and gRPC stream |
| DISPATCH | Yes, DISPATCH + workspaces | N+1 agents: one per workspace + one DISPATCH for control plane operations |
| Architecture | Standalone module (not a ChatPlatform) | Data flows opposite to platforms — gorp is the server, not the client |
| gRPC crate | tonic + prost | Standard Rust gRPC stack, async, well-maintained |

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│  coven-gateway (Go)                                     │
│                                                         │
│  HTTP API ← users/frontends                             │
│  gRPC ↕ agents                                          │
│                                                         │
│  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌───────────┐  │
│  │gorp-     │ │gorp-     │ │gorp-     │ │gorp-      │  │
│  │research  │ │news      │ │pa        │ │DISPATCH   │  │
│  └────┬─────┘ └────┬─────┘ └────┬─────┘ └─────┬─────┘  │
└───────┼────────────┼────────────┼──────────────┼────────┘
        │ gRPC       │ gRPC       │ gRPC         │ gRPC
┌───────┼────────────┼────────────┼──────────────┼────────┐
│  gorp-rs                                                │
│                                                         │
│  ┌─ CovenProvider ────────────────────────────────────┐  │
│  │                                                    │  │
│  │  AgentStream     AgentStream     AgentStream       │  │
│  │  (research)      (news)          (pa)              │  │
│  │      │               │               │            │  │
│  └──────┼───────────────┼───────────────┼────────────┘  │
│         │               │               │               │
│  ┌──────▼───────────────▼───────────────▼────────────┐  │
│  │  gorp-agent backends (ACP, mux, direct)           │  │
│  └───────────────────────────────────────────────────┘  │
│                                                         │
│  ┌─ PlatformRegistry (independent, still running) ───┐  │
│  │  Matrix │ Telegram │ Slack │ WhatsApp              │  │
│  └────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────┘
```

The coven provider runs alongside the platform registry — they're independent. Platforms handle user messages inbound. Coven exposes workspaces outbound. Both use the same agent backends and workspace state.

## Config

```toml
[coven]
gateway_addr = "localhost:50051"       # gRPC address
register_dispatch = true                # Register DISPATCH agent
agent_name_prefix = "gorp"             # Agents named "gorp-research", "gorp-news", etc.

# Optional auth
ssh_key_path = "~/.ssh/id_ed25519"     # For agent authentication
```

```rust
pub struct CovenConfig {
    pub gateway_addr: String,
    pub register_dispatch: bool,
    pub agent_name_prefix: String,
    pub ssh_key_path: Option<String>,
}
```

Top-level `Config`:

```rust
pub struct Config {
    pub matrix: Option<MatrixConfig>,
    pub telegram: Option<TelegramConfig>,
    pub slack: Option<SlackConfig>,
    pub whatsapp: Option<WhatsAppConfig>,
    pub coven: Option<CovenConfig>,          // New
    pub backend: BackendConfig,
    pub webhook: WebhookConfig,
    pub workspace: WorkspaceConfig,
    pub scheduler: SchedulerConfig,
}
```

## Agent Registration

On startup, gorp scans workspaces and registers each one with the gateway:

```rust
// Per workspace
RegisterAgent {
    agent_id: deterministic_uuid("gorp-research"),  // Stable across restarts
    name: "gorp-research",
    capabilities: ["chat", "base"],
    metadata: AgentMetadata {
        working_directory: "/path/to/workspace/research",
        hostname: hostname(),
        os: "linux",
        workspaces: ["research"],
        backend: "acp",
    },
    protocol_features: ["token_usage", "tool_states", "cancellation"],
}

// DISPATCH agent (if register_dispatch = true)
RegisterAgent {
    agent_id: deterministic_uuid("gorp-dispatch"),
    name: "gorp-DISPATCH",
    capabilities: ["chat", "base", "admin"],
    metadata: AgentMetadata {
        working_directory: config.workspace.path,
        workspaces: ["dispatch"],
        backend: "dispatch",
    },
    protocol_features: ["token_usage", "tool_states", "cancellation"],
}
```

Agent IDs are deterministic (derived from workspace name) so the gateway recognizes the same agent across restarts. The gateway responds with a `Welcome` message containing instance ID, available tools, and MCP token.

## gRPC Stream Management

Each registered agent maintains its own long-lived bidirectional gRPC stream.

### CovenProvider

```rust
// src/coven/mod.rs
pub struct CovenProvider {
    config: CovenConfig,
    streams: HashMap<String, AgentStream>,   // workspace name → stream
    server: Arc<ServerState>,
}
```

### AgentStream

```rust
// src/coven/stream.rs
pub struct AgentStream {
    agent_id: String,
    instance_id: String,               // Short code from Welcome
    workspace_name: String,
    grpc_tx: mpsc::Sender<AgentMessage>,
    agent_handle: Option<AgentHandle>,
    heartbeat_interval: Duration,
}
```

### Lifecycle

```
gorp startup
    │
    ├── CovenProvider::start(config, server)
    │
    ├── For each workspace:
    │   ├── Open gRPC channel to gateway_addr
    │   ├── Call AgentStream(bidirectional)
    │   ├── Send RegisterAgent
    │   ├── Await Welcome response
    │   ├── Spawn heartbeat task (every 30s)
    │   └── Spawn message handler task (awaits SendMessage)
    │
    ├── Register DISPATCH stream (if enabled)
    │
    └── Running... streams handle messages concurrently
```

### Per-Stream Tasks

```rust
impl AgentStream {
    async fn run(mut self, grpc_rx: Streaming<ServerMessage>) {
        tokio::select! {
            // Task 1: Read from gateway
            msg = grpc_rx.next() => {
                match msg.payload {
                    SendMessage(req) => self.handle_message(req).await,
                    Shutdown(_) => self.graceful_shutdown().await,
                    CancelRequest(req) => self.cancel(req).await,
                    InjectContext(ctx) => self.inject(ctx).await,
                    _ => {}
                }
            }

            // Task 2: Heartbeat
            _ = tokio::time::sleep(self.heartbeat_interval) => {
                self.grpc_tx.send(AgentMessage::heartbeat()).await;
            }
        }
    }
}
```

### Reconnection

If the gRPC stream drops, each `AgentStream` retries with exponential backoff (2s, 4s, 8s, max 60s). On reconnect, it re-registers with the same `agent_id` so the gateway recognizes it as the same agent returning.

## Message Handling & Response Streaming

When coven-gateway sends a `SendMessage`, gorp routes it to the workspace's agent backend and streams the response back as `MessageResponse` events over gRPC.

### Workspace Message Flow

```rust
impl AgentStream {
    async fn handle_message(&mut self, req: SendMessage) -> Result<()> {
        let request_id = req.request_id.clone();

        if self.workspace_name == "dispatch" {
            return self.handle_dispatch(req).await;
        }

        // Route to agent backend
        let handle = self.agent_handle.as_ref().unwrap();
        let mut stream = handle.prompt(&req.content).await?;

        while let Some(event) = stream.next().await {
            let msg = match event {
                AgentEvent::Text(chunk) => {
                    AgentMessage::response(MessageResponse {
                        request_id: request_id.clone(),
                        event: Some(Event::Text(chunk)),
                    })
                }
                AgentEvent::Thinking(text) => {
                    AgentMessage::response(MessageResponse {
                        request_id: request_id.clone(),
                        event: Some(Event::Thinking(text)),
                    })
                }
                AgentEvent::ToolUse { id, name, input } => {
                    AgentMessage::response(MessageResponse {
                        request_id: request_id.clone(),
                        event: Some(Event::ToolUse(ToolUse {
                            id, name, input_json: input,
                        })),
                    })
                }
                AgentEvent::ToolResult { id, output, is_error } => {
                    AgentMessage::response(MessageResponse {
                        request_id: request_id.clone(),
                        event: Some(Event::ToolResult(ToolResult {
                            id, output, is_error,
                        })),
                    })
                }
                AgentEvent::Complete { usage } => {
                    // Send usage first
                    self.grpc_tx.send(AgentMessage::response(MessageResponse {
                        request_id: request_id.clone(),
                        event: Some(Event::Usage(TokenUsage {
                            input_tokens: usage.input,
                            output_tokens: usage.output,
                        })),
                    })).await?;

                    // Then done
                    AgentMessage::response(MessageResponse {
                        request_id: request_id.clone(),
                        event: Some(Event::Done(Done {
                            full_response: String::new(),
                        })),
                    })
                }
                AgentEvent::Error(e) => {
                    AgentMessage::response(MessageResponse {
                        request_id: request_id.clone(),
                        event: Some(Event::Error(e.to_string())),
                    })
                }
            };

            self.grpc_tx.send(msg).await?;
        }

        Ok(())
    }
}
```

### Event Mapping

| gorp AgentEvent | coven MessageResponse event |
|---|---|
| `Text(chunk)` | `text` |
| `Thinking(text)` | `thinking` |
| `ToolUse { id, name, input }` | `tool_use` |
| `ToolResult { id, output, is_error }` | `tool_result` |
| `Complete { usage }` | `usage` then `done` |
| `Error(e)` | `error` |

### Cancellation

When coven-gateway sends `CancelRequest`, gorp calls `handle.cancel()` on the agent backend. The stream terminates, and gorp sends a `cancelled` response event.

### Attachments

`SendMessage` can include `FileAttachment` objects. Gorp writes them to the workspace's attachment directory before passing to the agent backend, same as it does for platform messages.

## DISPATCH Agent

The DISPATCH agent exposes gorp's control plane through coven-gateway. Users talking to `gorp-DISPATCH` can manage the whole gorp instance.

### Supported Operations

| Command | Action |
|---|---|
| "list workspaces" | Returns workspace names, status, active sessions |
| "list schedules" | Returns all scheduled tasks across workspaces |
| "create schedule for research: check arxiv daily at 6am" | Creates a schedule via SchedulerStore |
| "pause the news digest schedule" | Pauses a schedule |
| "show platform status" | Returns connected platforms and their state |
| "show recent activity" | Returns recent messages/tasks across all workspaces |
| "run the news digest now" | Triggers an immediate schedule execution |

### Implementation

DISPATCH routes through the existing `dispatch_handler` — the same code path that handles DM-based dispatch from Matrix/WhatsApp:

```rust
if self.workspace_name == "dispatch" {
    let response = dispatch_handler::handle_text(
        &req.content,
        &self.server,
    ).await?;

    for chunk in chunk_text(&response, 200) {
        self.grpc_tx.send(text_response(&request_id, &chunk)).await?;
    }
    self.send_done(&request_id, &response).await?;
}
```

### Capabilities

```rust
capabilities: ["chat", "base", "admin"]
```

The `admin` capability tells coven-gateway this agent can do privileged operations. Coven-gateway can gate access so only authorized users talk to DISPATCH.

## Comparison with Platform Integrations

| Concern | Platforms (Matrix, Slack, etc.) | Coven Provider |
|---|---|---|
| Direction | Inbound — users message gorp | Outbound — gorp exposes itself |
| Protocol | Various (SDK, HTTP, sidecar) | gRPC bidirectional streaming |
| Abstraction | `ChatPlatform` trait | Standalone module |
| Multiplicity | One connection per platform | One stream per workspace |
| Message flow | Platform → message handler → agent | Gateway → agent stream → agent backend |
| Auth | Bot tokens, QR codes | SSH key fingerprint |
| Lifecycle | Platforms are consumers | Coven is a provider |

## Files Created

| File | Purpose |
|---|---|
| `src/coven/mod.rs` | `CovenProvider` — startup, workspace scanning, stream lifecycle |
| `src/coven/stream.rs` | `AgentStream` — per-agent gRPC stream, heartbeat, message handling |
| `src/coven/proto.rs` | Generated protobuf types (via tonic-build) |
| `src/coven/reconnect.rs` | Exponential backoff reconnection logic |
| `proto/coven.proto` | Protobuf service definition (synced from coven-gateway) |

## Files Modified

| File | Change |
|---|---|
| `Cargo.toml` | Add `coven` feature flag, `tonic` + `prost` deps |
| `gorp-core/src/config.rs` | Add `CovenConfig` |
| `src/main.rs` | Start `CovenProvider` if coven config present |
| `src/lib.rs` | Add `pub mod coven` behind feature gate |
| `build.rs` | Add tonic-build protobuf compilation (if not already present) |

## Files Untouched

Everything else — gorp-core traits, gorp-agent, all platform implementations, TUI, GUI, web, message handler, scheduler. The coven provider is purely additive.

## Dependencies

```toml
[features]
coven = ["dep:tonic", "dep:prost"]

[dependencies]
tonic = { version = "0.12", optional = true }
prost = { version = "0.13", optional = true }

[build-dependencies]
tonic-build = { version = "0.12", optional = true }
```
