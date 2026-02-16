# Message Bus Architecture Design

**Date:** 2026-02-15
**Status:** Approved

## Overview

Refactor gorp from a Matrix-centric agent bridge into a platform-agnostic message bus with agent orchestration. Gorp should work fully without any gateways — the web interface is built-in and always available. Chat platforms (Matrix, Slack, Telegram) are optional gateway adapters that plug into the same bus.

## Core Principles

- **Bus-centric:** A central `MessageBus` with broadcast channels connects all components
- **Internal tokio channels:** No external broker — `tokio::sync::broadcast` for the live message flow
- **N sessions, any interface:** Multiple agent sessions exist simultaneously. Each is reachable from any connected platform or the web
- **One platform channel per agent, many platforms per session:** A Matrix room maps to one agent session. A Slack channel maps to one agent session. But both can map to the *same* session
- **Broadcast responses:** When an agent responds, all connected platforms see the response in real-time
- **DISPATCH as command handler:** A universal control plane reachable from any platform. Handles session lifecycle and supervisor operations
- **Web always on:** The admin panel and web chat work with zero gateways configured
- **Clean break:** Rebuild the core message flow, don't incrementally migrate

## Data Types

```rust
/// A message entering the bus from any source
struct BusMessage {
    id: String,                    // unique event ID (for dedup)
    source: MessageSource,         // which gateway/interface sent this
    session_target: SessionTarget, // where it should go
    sender: String,                // human-readable sender identity
    body: String,                  // message content
    timestamp: DateTime<Utc>,
}

enum MessageSource {
    Platform { platform_id: String, channel_id: String },
    Web { connection_id: String },
    Api { token_hint: String },
}

enum SessionTarget {
    Dispatch,                      // unmapped channel -> goes to DISPATCH
    Session { name: String },      // mapped channel -> goes to named session
}

/// A response leaving the bus toward platforms
struct BusResponse {
    session_name: String,          // which session produced this
    content: ResponseContent,
    timestamp: DateTime<Utc>,
}

enum ResponseContent {
    Chunk(String),                 // streaming text chunk
    Complete(String),              // final assembled response
    Error(String),                 // agent error
    SystemNotice(String),          // dispatch/system messages
}
```

`SessionTarget::Dispatch` is how unmapped channels land in DISPATCH. Once a channel is bound to a session via `!join`, the bus resolves it to `SessionTarget::Session { name }` based on the channel mapping table.

## MessageBus

Two broadcast channels and a mapping table.

```rust
struct MessageBus {
    inbound_tx: broadcast::Sender<BusMessage>,
    outbound_tx: broadcast::Sender<BusResponse>,
    channel_map: Arc<RwLock<HashMap<(String, String), String>>>,
}

impl MessageBus {
    fn publish(&self, msg: BusMessage);
    fn subscribe_responses(&self) -> broadcast::Receiver<BusResponse>;
    fn bind_channel(&self, platform_id: &str, channel_id: &str, session_name: &str);
    fn unbind_channel(&self, platform_id: &str, channel_id: &str);
}
```

The channel map is persisted to SQLite (survives restarts) but held in-memory for fast lookups. On startup, load from DB. On bind/unbind, write-through to both.

Each gateway subscribes to the full outbound broadcast, then filters for responses matching its bound sessions. This is cheap for single-tenant (handful of sessions at most).

## Orchestrator

Consumes from the inbound bus, routes to agent sessions or DISPATCH, publishes responses to the outbound bus.

```rust
struct Orchestrator {
    bus: Arc<MessageBus>,
    warm_manager: WarmSessionManager,
    session_store: SessionStore,
    scheduler_store: SchedulerStore,
}
```

- One `tokio::spawn` per inbound message — sessions don't block each other
- Owns deduplication (replaces the Matrix-only EventDeduplicator)
- DISPATCH is a command handler inside the orchestrator (not an LLM-backed agent)

### DISPATCH Commands

```
!create <name> [workspace]  - Create a new agent session
!delete <name>              - Delete a session
!list                       - List active sessions
!status <name>              - Session details
!join <name>                - Bind this platform channel to a session
!leave                      - Unbind this platform channel
!tell <session> <message>   - Inject a message into another session
!read <session> [count]     - Read recent messages from a session
!broadcast <message>        - Send a message to all active sessions
!help                       - Show available commands
```

DISPATCH is a supervisor — it can inject messages into sessions, read their history, and manage the whole fleet. The web admin UI calls the same underlying methods.

## Gateway Adapters

Each gateway translates between platform-native events and bus types.

```rust
#[async_trait]
trait GatewayAdapter: Send + Sync {
    fn platform_id(&self) -> &str;
    async fn start(&self, bus: Arc<MessageBus>) -> Result<()>;
    async fn send(&self, channel_id: &str, content: ResponseContent) -> Result<()>;
    async fn stop(&self) -> Result<()>;
}
```

Each adapter has two loops:
1. **Inbound:** Listen for platform events, normalize to `BusMessage`, publish to bus
2. **Outbound:** Subscribe to bus responses, filter for bound sessions, deliver to platform

The web adapter is always on and built-in. It's not optional like chat platform gateways.

### Adapter implementations:
- `MatrixAdapter` — wraps Matrix SDK sync loop
- `SlackAdapter` — wraps Slack RTM/socket
- `TelegramAdapter` — wraps Telegram polling
- `WebAdapter` — WebSocket connections, admin panel chat

## Session Store Schema

```sql
sessions (
    name TEXT PRIMARY KEY,
    session_id TEXT,
    workspace_dir TEXT,
    backend_type TEXT,
    started BOOL,
    created_at TEXT
)

channel_bindings (
    platform_id TEXT,
    channel_id TEXT,
    session_name TEXT REFERENCES sessions(name),
    bound_at TEXT,
    PRIMARY KEY (platform_id, channel_id)
)
```

- A session can have multiple channel bindings (multi-platform)
- A platform channel maps to exactly one session (or none = DISPATCH)
- DISPATCH isn't stored — it's the implicit default
- `channel_bindings` is what the `MessageBus.channel_map` loads on startup

## Startup Lifecycle

```
1. Load config
2. Open SQLite, run migrations
3. Create MessageBus (load channel_map from DB)
4. Create Orchestrator (with warm_manager, session_store)
5. Create GatewayRegistry
6. For each configured platform:
     gateway.start(bus)  <- spawns inbound + outbound loops
7. Start WebAdapter (always, regardless of platform config)
8. Start Orchestrator.run()  <- main bus consumer loop
9. Start Scheduler (publishes BusMessages on cron triggers)
10. Wait for shutdown signal
```

Hot connect/disconnect: Adapters implement start/stop, so gateways can be added/removed at runtime via the admin UI without restarting gorp.

## What Gets Deleted vs Kept

### Deleted (replaced by new architecture):
- `src/message_handler/mod.rs` — dual-path handle_message/handle_incoming routing
- `src/message_handler/generic_channel.rs` — replaced by GatewayAdapter trait
- `src/message_handler/traits.rs` — MessagingPlatform trait -> GatewayAdapter
- `src/platform/registry.rs` — replaced by GatewayRegistry
- Matrix sync loop in `main.rs` — moves into MatrixAdapter
- `src/server.rs` ServerState — replaced by MessageBus + Orchestrator
- DISPATCH room logic scattered across message_handler

### Kept / Adapted:
- `gorp-agent/` — agent backend abstraction, untouched
- `WarmSessionManager` — wired to orchestrator instead of message_handler
- `SessionStore` — schema updated, same crate
- `SchedulerStore` — publishes BusMessage instead of calling agent directly
- `src/admin/` — admin panel stays, templates and routes adapt to new types
- Platform implementations — refactored from *Platform into *Adapter
- `src/webhook.rs` — simplified, webhook publishes to the bus
- `src/config.rs` — stays

### New modules:
- `src/bus.rs` — MessageBus, BusMessage, BusResponse types
- `src/orchestrator.rs` — Orchestrator, dispatch command handler
- `src/gateway/mod.rs` — GatewayAdapter trait, GatewayRegistry
- `src/gateway/matrix.rs`, `slack.rs`, `telegram.rs`, `web.rs` — adapters

## Zero-Config Experience

Fresh install, no platforms configured:
1. gorp starts, web adapter comes up
2. Open admin panel, go to chat
3. Talking to DISPATCH immediately
4. `!create research` -> creates a session
5. `!join research` -> binds web connection to it
6. Chatting with an agent, no platform dependencies

Later, configure Matrix in Gateways -> connects -> DM the bot -> lands in DISPATCH -> `!join research` -> Matrix and web both talking to the same agent.
