# Message Bus Refactor Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Refactor gorp from a Matrix-centric agent bridge into a platform-agnostic message bus where the web interface is built-in and chat platforms are optional gateway adapters.

**Architecture:** Bus-centric with internal tokio broadcast channels. An Orchestrator consumes inbound messages, routes to agent sessions or DISPATCH, and publishes responses to outbound channels. Gateway adapters (Matrix, Slack, Telegram, Web) translate between platform-native events and bus types.

**Tech Stack:** Rust, tokio (broadcast channels), rusqlite (SQLite), axum (web/websocket), askama (templates), gorp-agent (agent backends), gorp-core (warm sessions, scheduling)

**Design doc:** `docs/plans/2026-02-15-message-bus-design.md`

---

### Task 1: Core Bus Types

**Files:**
- Create: `src/bus.rs`
- Modify: `src/lib.rs` (add `pub mod bus;`)
- Test: `tests/bus_tests.rs`

**Step 1: Write failing tests for bus types**

```rust
// tests/bus_tests.rs
use gorp::bus::*;
use chrono::Utc;

#[test]
fn test_bus_message_dispatch_target() {
    let msg = BusMessage {
        id: "evt-1".to_string(),
        source: MessageSource::Web { connection_id: "ws-42".to_string() },
        session_target: SessionTarget::Dispatch,
        sender: "harper".to_string(),
        body: "!create research".to_string(),
        timestamp: Utc::now(),
    };
    assert!(matches!(msg.session_target, SessionTarget::Dispatch));
    assert_eq!(msg.sender, "harper");
}

#[test]
fn test_bus_message_session_target() {
    let msg = BusMessage {
        id: "evt-2".to_string(),
        source: MessageSource::Platform {
            platform_id: "matrix".to_string(),
            channel_id: "!room123:matrix.org".to_string(),
        },
        session_target: SessionTarget::Session { name: "research".to_string() },
        sender: "harper".to_string(),
        body: "summarize the paper".to_string(),
        timestamp: Utc::now(),
    };
    assert!(matches!(msg.session_target, SessionTarget::Session { ref name } if name == "research"));
}

#[test]
fn test_bus_response_chunk() {
    let resp = BusResponse {
        session_name: "research".to_string(),
        content: ResponseContent::Chunk("partial output...".to_string()),
        timestamp: Utc::now(),
    };
    assert!(matches!(resp.content, ResponseContent::Chunk(_)));
}

#[test]
fn test_bus_response_complete() {
    let resp = BusResponse {
        session_name: "research".to_string(),
        content: ResponseContent::Complete("full response".to_string()),
        timestamp: Utc::now(),
    };
    assert!(matches!(resp.content, ResponseContent::Complete(_)));
}

#[test]
fn test_bus_response_system_notice() {
    let resp = BusResponse {
        session_name: "".to_string(),
        content: ResponseContent::SystemNotice("Session 'research' created".to_string()),
        timestamp: Utc::now(),
    };
    assert!(matches!(resp.content, ResponseContent::SystemNotice(_)));
}

#[test]
fn test_message_source_api() {
    let source = MessageSource::Api { token_hint: "sk-***abc".to_string() };
    assert!(matches!(source, MessageSource::Api { .. }));
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test --test bus_tests`
Expected: FAIL — `unresolved import gorp::bus`

**Step 3: Implement bus types**

```rust
// src/bus.rs
// ABOUTME: Core message bus types for platform-agnostic message routing.
// ABOUTME: Defines BusMessage (inbound), BusResponse (outbound), and supporting enums.

use chrono::{DateTime, Utc};

/// A message entering the bus from any source (gateway, web, API).
#[derive(Debug, Clone)]
pub struct BusMessage {
    /// Unique event ID for deduplication
    pub id: String,
    /// Which gateway/interface sent this message
    pub source: MessageSource,
    /// Where the message should be routed
    pub session_target: SessionTarget,
    /// Human-readable sender identity
    pub sender: String,
    /// Message content
    pub body: String,
    /// When the message was created
    pub timestamp: DateTime<Utc>,
}

/// Identifies where a message originated.
#[derive(Debug, Clone)]
pub enum MessageSource {
    /// From a chat platform (Matrix, Slack, Telegram)
    Platform { platform_id: String, channel_id: String },
    /// From the web admin chat UI
    Web { connection_id: String },
    /// From the webhook API
    Api { token_hint: String },
}

/// Where a message should be routed.
#[derive(Debug, Clone)]
pub enum SessionTarget {
    /// Unmapped channel — route to DISPATCH command handler
    Dispatch,
    /// Mapped channel — route to the named agent session
    Session { name: String },
}

/// A response leaving the bus toward connected platforms.
#[derive(Debug, Clone)]
pub struct BusResponse {
    /// Which agent session produced this response
    pub session_name: String,
    /// Response payload
    pub content: ResponseContent,
    /// When the response was generated
    pub timestamp: DateTime<Utc>,
}

/// Payload types for outbound responses.
#[derive(Debug, Clone)]
pub enum ResponseContent {
    /// Streaming text chunk (partial output)
    Chunk(String),
    /// Final assembled response
    Complete(String),
    /// Agent or system error
    Error(String),
    /// DISPATCH or system notification
    SystemNotice(String),
}
```

Add to `src/lib.rs`:
```rust
pub mod bus;
```

**Step 4: Run tests to verify they pass**

Run: `cargo test --test bus_tests`
Expected: PASS — all 6 tests green

**Step 5: Commit**

```bash
git add src/bus.rs src/lib.rs tests/bus_tests.rs
git commit -m "feat: add core bus types (BusMessage, BusResponse, MessageSource, SessionTarget)"
```

---

### Task 2: MessageBus Struct

**Files:**
- Modify: `src/bus.rs`
- Test: `tests/bus_tests.rs` (append)

**Step 1: Write failing tests for MessageBus**

```rust
// append to tests/bus_tests.rs
use tokio::sync::broadcast;

#[tokio::test]
async fn test_message_bus_publish_and_receive() {
    let bus = MessageBus::new(64);
    let mut rx = bus.subscribe_inbound();

    let msg = BusMessage {
        id: "evt-1".to_string(),
        source: MessageSource::Web { connection_id: "ws-1".to_string() },
        session_target: SessionTarget::Dispatch,
        sender: "harper".to_string(),
        body: "hello".to_string(),
        timestamp: Utc::now(),
    };
    bus.publish_inbound(msg);

    let received = rx.recv().await.unwrap();
    assert_eq!(received.id, "evt-1");
    assert_eq!(received.body, "hello");
}

#[tokio::test]
async fn test_message_bus_response_broadcast() {
    let bus = MessageBus::new(64);
    let mut rx1 = bus.subscribe_responses();
    let mut rx2 = bus.subscribe_responses();

    let resp = BusResponse {
        session_name: "research".to_string(),
        content: ResponseContent::Complete("done".to_string()),
        timestamp: Utc::now(),
    };
    bus.publish_response(resp);

    let r1 = rx1.recv().await.unwrap();
    let r2 = rx2.recv().await.unwrap();
    assert_eq!(r1.session_name, "research");
    assert_eq!(r2.session_name, "research");
}

#[tokio::test]
async fn test_message_bus_channel_binding() {
    let bus = MessageBus::new(64);

    // No binding → resolves to Dispatch
    let target = bus.resolve_target("matrix", "!room1:m.org");
    assert!(matches!(target, SessionTarget::Dispatch));

    // Bind channel → resolves to Session
    bus.bind_channel("matrix", "!room1:m.org", "research");
    let target = bus.resolve_target("matrix", "!room1:m.org");
    assert!(matches!(target, SessionTarget::Session { ref name } if name == "research"));

    // Unbind → back to Dispatch
    bus.unbind_channel("matrix", "!room1:m.org");
    let target = bus.resolve_target("matrix", "!room1:m.org");
    assert!(matches!(target, SessionTarget::Dispatch));
}

#[tokio::test]
async fn test_message_bus_multiple_bindings_same_session() {
    let bus = MessageBus::new(64);

    bus.bind_channel("matrix", "!room1:m.org", "research");
    bus.bind_channel("slack", "C12345", "research");

    let t1 = bus.resolve_target("matrix", "!room1:m.org");
    let t2 = bus.resolve_target("slack", "C12345");

    assert!(matches!(t1, SessionTarget::Session { ref name } if name == "research"));
    assert!(matches!(t2, SessionTarget::Session { ref name } if name == "research"));
}

#[test]
fn test_message_bus_list_bindings_for_session() {
    let bus = MessageBus::new(64);
    bus.bind_channel("matrix", "!room1:m.org", "research");
    bus.bind_channel("slack", "C12345", "research");
    bus.bind_channel("matrix", "!room2:m.org", "ops");

    let bindings = bus.bindings_for_session("research");
    assert_eq!(bindings.len(), 2);
    assert!(bindings.contains(&("matrix".to_string(), "!room1:m.org".to_string())));
    assert!(bindings.contains(&("slack".to_string(), "C12345".to_string())));
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test --test bus_tests`
Expected: FAIL — `MessageBus` not found

**Step 3: Implement MessageBus**

Add to `src/bus.rs`:

```rust
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};

/// Central message bus connecting gateways to the orchestrator.
/// Uses tokio broadcast channels for pub/sub and an in-memory
/// map for channel-to-session bindings.
#[derive(Clone)]
pub struct MessageBus {
    inbound_tx: broadcast::Sender<BusMessage>,
    outbound_tx: broadcast::Sender<BusResponse>,
    channel_map: Arc<RwLock<HashMap<(String, String), String>>>,
}

impl MessageBus {
    /// Create a new MessageBus with the given channel capacity.
    pub fn new(capacity: usize) -> Self {
        let (inbound_tx, _) = broadcast::channel(capacity);
        let (outbound_tx, _) = broadcast::channel(capacity);
        Self {
            inbound_tx,
            outbound_tx,
            channel_map: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Publish an inbound message (gateway → orchestrator).
    pub fn publish_inbound(&self, msg: BusMessage) {
        let _ = self.inbound_tx.send(msg);
    }

    /// Subscribe to inbound messages (orchestrator consumes these).
    pub fn subscribe_inbound(&self) -> broadcast::Receiver<BusMessage> {
        self.inbound_tx.subscribe()
    }

    /// Publish an outbound response (orchestrator → gateways).
    pub fn publish_response(&self, resp: BusResponse) {
        let _ = self.outbound_tx.send(resp);
    }

    /// Subscribe to outbound responses (gateways consume these).
    pub fn subscribe_responses(&self) -> broadcast::Receiver<BusResponse> {
        self.outbound_tx.subscribe()
    }

    /// Resolve a platform channel to its session target.
    /// Returns Dispatch if no binding exists.
    pub fn resolve_target(&self, platform_id: &str, channel_id: &str) -> SessionTarget {
        let map = self.channel_map.blocking_read();
        match map.get(&(platform_id.to_string(), channel_id.to_string())) {
            Some(session_name) => SessionTarget::Session { name: session_name.clone() },
            None => SessionTarget::Dispatch,
        }
    }

    /// Async version of resolve_target for use in async contexts.
    pub async fn resolve_target_async(&self, platform_id: &str, channel_id: &str) -> SessionTarget {
        let map = self.channel_map.read().await;
        match map.get(&(platform_id.to_string(), channel_id.to_string())) {
            Some(session_name) => SessionTarget::Session { name: session_name.clone() },
            None => SessionTarget::Dispatch,
        }
    }

    /// Bind a platform channel to an agent session.
    pub fn bind_channel(&self, platform_id: &str, channel_id: &str, session_name: &str) {
        let mut map = self.channel_map.blocking_write();
        map.insert(
            (platform_id.to_string(), channel_id.to_string()),
            session_name.to_string(),
        );
    }

    /// Async version of bind_channel.
    pub async fn bind_channel_async(&self, platform_id: &str, channel_id: &str, session_name: &str) {
        let mut map = self.channel_map.write().await;
        map.insert(
            (platform_id.to_string(), channel_id.to_string()),
            session_name.to_string(),
        );
    }

    /// Unbind a platform channel from its session.
    pub fn unbind_channel(&self, platform_id: &str, channel_id: &str) {
        let mut map = self.channel_map.blocking_write();
        map.remove(&(platform_id.to_string(), channel_id.to_string()));
    }

    /// List all bindings for a given session.
    pub fn bindings_for_session(&self, session_name: &str) -> Vec<(String, String)> {
        let map = self.channel_map.blocking_read();
        map.iter()
            .filter(|(_, v)| *v == session_name)
            .map(|((p, c), _)| (p.clone(), c.clone()))
            .collect()
    }

    /// Load channel bindings from an external source (called at startup).
    pub async fn load_bindings(&self, bindings: Vec<(String, String, String)>) {
        let mut map = self.channel_map.write().await;
        for (platform_id, channel_id, session_name) in bindings {
            map.insert((platform_id, channel_id), session_name);
        }
    }
}
```

Note: `BusMessage` and `BusResponse` need to derive `Clone` for broadcast channels. They already do from Task 1.

**Step 4: Run tests to verify they pass**

Run: `cargo test --test bus_tests`
Expected: PASS — all tests green

**Step 5: Commit**

```bash
git add src/bus.rs tests/bus_tests.rs
git commit -m "feat: add MessageBus with channel binding and pub/sub"
```

---

### Task 3: GatewayAdapter Trait and GatewayRegistry

**Files:**
- Create: `src/gateway/mod.rs`
- Create: `src/gateway/registry.rs`
- Modify: `src/lib.rs` (add `pub mod gateway;`)
- Test: `tests/gateway_tests.rs`

**Step 1: Write failing tests**

```rust
// tests/gateway_tests.rs
use gorp::bus::*;
use gorp::gateway::registry::GatewayRegistry;
use gorp::gateway::GatewayAdapter;
use async_trait::async_trait;
use std::sync::Arc;

struct MockAdapter {
    id: String,
}

#[async_trait]
impl GatewayAdapter for MockAdapter {
    fn platform_id(&self) -> &str { &self.id }

    async fn start(&self, _bus: Arc<MessageBus>) -> anyhow::Result<()> { Ok(()) }

    async fn send(&self, _channel_id: &str, _content: ResponseContent) -> anyhow::Result<()> { Ok(()) }

    async fn stop(&self) -> anyhow::Result<()> { Ok(()) }
}

#[tokio::test]
async fn test_registry_register_and_get() {
    let mut registry = GatewayRegistry::new();
    let adapter = MockAdapter { id: "test".to_string() };
    registry.register(Box::new(adapter));
    assert!(registry.get("test").is_some());
    assert!(registry.get("nonexistent").is_none());
}

#[tokio::test]
async fn test_registry_platform_ids() {
    let mut registry = GatewayRegistry::new();
    registry.register(Box::new(MockAdapter { id: "matrix".to_string() }));
    registry.register(Box::new(MockAdapter { id: "slack".to_string() }));
    let ids = registry.platform_ids();
    assert_eq!(ids.len(), 2);
    assert!(ids.contains(&"matrix".to_string()));
    assert!(ids.contains(&"slack".to_string()));
}

#[tokio::test]
async fn test_registry_unregister() {
    let mut registry = GatewayRegistry::new();
    registry.register(Box::new(MockAdapter { id: "matrix".to_string() }));
    assert!(registry.get("matrix").is_some());
    registry.unregister("matrix");
    assert!(registry.get("matrix").is_none());
}

#[tokio::test]
async fn test_registry_shutdown_all() {
    let mut registry = GatewayRegistry::new();
    registry.register(Box::new(MockAdapter { id: "a".to_string() }));
    registry.register(Box::new(MockAdapter { id: "b".to_string() }));
    // Should not panic
    registry.shutdown_all().await;
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test --test gateway_tests`
Expected: FAIL — `unresolved import gorp::gateway`

**Step 3: Implement GatewayAdapter trait**

```rust
// src/gateway/mod.rs
// ABOUTME: Gateway adapter abstraction for platform-agnostic message routing.
// ABOUTME: Defines the GatewayAdapter trait that all platform integrations implement.

pub mod registry;

use async_trait::async_trait;
use std::sync::Arc;

use crate::bus::{MessageBus, ResponseContent};

/// Trait for platform gateway adapters. Each adapter translates between
/// platform-native events and bus types. Adapters have two loops:
/// inbound (platform → bus) and outbound (bus → platform).
#[async_trait]
pub trait GatewayAdapter: Send + Sync {
    /// Unique platform identifier (e.g., "matrix", "slack", "telegram", "web")
    fn platform_id(&self) -> &str;

    /// Start the adapter's inbound and outbound loops.
    /// The adapter should spawn its own tasks and return immediately.
    async fn start(&self, bus: Arc<MessageBus>) -> anyhow::Result<()>;

    /// Send a response to a specific channel on this platform.
    async fn send(&self, channel_id: &str, content: ResponseContent) -> anyhow::Result<()>;

    /// Graceful shutdown — stop loops, close connections.
    async fn stop(&self) -> anyhow::Result<()>;
}
```

**Step 4: Implement GatewayRegistry**

```rust
// src/gateway/registry.rs
// ABOUTME: Registry that manages gateway adapter lifecycle.
// ABOUTME: Handles registration, lookup, and coordinated shutdown of all adapters.

use std::collections::HashMap;
use super::GatewayAdapter;

/// Manages the lifecycle of all registered gateway adapters.
pub struct GatewayRegistry {
    adapters: HashMap<String, Box<dyn GatewayAdapter>>,
}

impl GatewayRegistry {
    pub fn new() -> Self {
        Self {
            adapters: HashMap::new(),
        }
    }

    /// Register a gateway adapter. Replaces any existing adapter with the same platform_id.
    pub fn register(&mut self, adapter: Box<dyn GatewayAdapter>) {
        let id = adapter.platform_id().to_string();
        self.adapters.insert(id, adapter);
    }

    /// Get a reference to an adapter by platform ID.
    pub fn get(&self, platform_id: &str) -> Option<&dyn GatewayAdapter> {
        self.adapters.get(platform_id).map(|a| a.as_ref())
    }

    /// List all registered platform IDs.
    pub fn platform_ids(&self) -> Vec<String> {
        self.adapters.keys().cloned().collect()
    }

    /// Remove and return an adapter by platform ID.
    pub fn unregister(&mut self, platform_id: &str) -> Option<Box<dyn GatewayAdapter>> {
        self.adapters.remove(platform_id)
    }

    /// Gracefully shut down all adapters.
    pub async fn shutdown_all(&mut self) {
        for (id, adapter) in self.adapters.drain() {
            if let Err(e) = adapter.stop().await {
                tracing::error!(platform = %id, error = %e, "Failed to stop gateway adapter");
            }
        }
    }
}
```

Add to `src/lib.rs`:
```rust
pub mod gateway;
```

**Step 5: Run tests to verify they pass**

Run: `cargo test --test gateway_tests`
Expected: PASS — all tests green

**Step 6: Commit**

```bash
git add src/gateway/ src/lib.rs tests/gateway_tests.rs
git commit -m "feat: add GatewayAdapter trait and GatewayRegistry"
```

---

### Task 4: Session Store Schema Migration

**Files:**
- Modify: `gorp-core/src/session.rs`
- Test: `gorp-core/tests/` or inline tests

The session store needs a new `channel_bindings` table and the `Channel` struct needs to evolve into `Session`. Since this is a clean break, we add the new table alongside the existing one and provide migration.

**Step 1: Write failing tests for channel_bindings**

```rust
// Add to session store tests
#[test]
fn test_create_and_list_channel_bindings() {
    let store = create_test_store();
    store.bind_channel("matrix", "!room1:m.org", "research").unwrap();
    store.bind_channel("slack", "C12345", "research").unwrap();

    let bindings = store.list_bindings_for_session("research").unwrap();
    assert_eq!(bindings.len(), 2);
}

#[test]
fn test_resolve_binding() {
    let store = create_test_store();
    store.bind_channel("matrix", "!room1:m.org", "research").unwrap();

    let session = store.resolve_binding("matrix", "!room1:m.org").unwrap();
    assert_eq!(session, Some("research".to_string()));

    let none = store.resolve_binding("matrix", "!unknown:m.org").unwrap();
    assert_eq!(none, None);
}

#[test]
fn test_unbind_channel() {
    let store = create_test_store();
    store.bind_channel("matrix", "!room1:m.org", "research").unwrap();
    store.unbind_channel("matrix", "!room1:m.org").unwrap();

    let session = store.resolve_binding("matrix", "!room1:m.org").unwrap();
    assert_eq!(session, None);
}

#[test]
fn test_list_all_bindings() {
    let store = create_test_store();
    store.bind_channel("matrix", "!r1:m.org", "research").unwrap();
    store.bind_channel("slack", "C1", "ops").unwrap();

    let all = store.list_all_bindings().unwrap();
    assert_eq!(all.len(), 2);
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p gorp-core`
Expected: FAIL — methods not found on SessionStore

**Step 3: Add channel_bindings table and methods to SessionStore**

Add to `SessionStore::new()` table creation:
```rust
db.execute_batch(
    "CREATE TABLE IF NOT EXISTS channel_bindings (
        platform_id TEXT NOT NULL,
        channel_id TEXT NOT NULL,
        session_name TEXT NOT NULL,
        bound_at TEXT NOT NULL DEFAULT (datetime('now')),
        PRIMARY KEY (platform_id, channel_id)
    )"
)?;
```

Add methods:
```rust
pub fn bind_channel(&self, platform_id: &str, channel_id: &str, session_name: &str) -> Result<()> {
    let db = self.db.lock().unwrap();
    db.execute(
        "INSERT OR REPLACE INTO channel_bindings (platform_id, channel_id, session_name) VALUES (?1, ?2, ?3)",
        rusqlite::params![platform_id, channel_id, session_name],
    )?;
    Ok(())
}

pub fn unbind_channel(&self, platform_id: &str, channel_id: &str) -> Result<()> {
    let db = self.db.lock().unwrap();
    db.execute(
        "DELETE FROM channel_bindings WHERE platform_id = ?1 AND channel_id = ?2",
        rusqlite::params![platform_id, channel_id],
    )?;
    Ok(())
}

pub fn resolve_binding(&self, platform_id: &str, channel_id: &str) -> Result<Option<String>> {
    let db = self.db.lock().unwrap();
    let mut stmt = db.prepare(
        "SELECT session_name FROM channel_bindings WHERE platform_id = ?1 AND channel_id = ?2"
    )?;
    let result = stmt.query_row(rusqlite::params![platform_id, channel_id], |row| {
        row.get::<_, String>(0)
    }).optional()?;
    Ok(result)
}

pub fn list_bindings_for_session(&self, session_name: &str) -> Result<Vec<(String, String)>> {
    let db = self.db.lock().unwrap();
    let mut stmt = db.prepare(
        "SELECT platform_id, channel_id FROM channel_bindings WHERE session_name = ?1"
    )?;
    let rows = stmt.query_map(rusqlite::params![session_name], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    let mut bindings = Vec::new();
    for row in rows {
        bindings.push(row?);
    }
    Ok(bindings)
}

pub fn list_all_bindings(&self) -> Result<Vec<(String, String, String)>> {
    let db = self.db.lock().unwrap();
    let mut stmt = db.prepare(
        "SELECT platform_id, channel_id, session_name FROM channel_bindings"
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?))
    })?;
    let mut bindings = Vec::new();
    for row in rows {
        bindings.push(row?);
    }
    Ok(bindings)
}
```

**Step 4: Run tests to verify they pass**

Run: `cargo test -p gorp-core`
Expected: PASS

**Step 5: Commit**

```bash
git add gorp-core/src/session.rs
git commit -m "feat: add channel_bindings table to session store"
```

---

### Task 5: Orchestrator Foundation

**Files:**
- Create: `src/orchestrator.rs`
- Modify: `src/lib.rs` (add `pub mod orchestrator;`)
- Test: `tests/orchestrator_tests.rs`

**Step 1: Write failing tests for orchestrator message routing**

```rust
// tests/orchestrator_tests.rs
use gorp::bus::*;
use gorp::orchestrator::DispatchCommand;
use chrono::Utc;

#[test]
fn test_parse_dispatch_create() {
    let cmd = DispatchCommand::parse("!create research");
    assert!(matches!(cmd, DispatchCommand::Create { ref name, .. } if name == "research"));
}

#[test]
fn test_parse_dispatch_create_with_workspace() {
    let cmd = DispatchCommand::parse("!create research /home/harper/ws/research");
    match cmd {
        DispatchCommand::Create { name, workspace } => {
            assert_eq!(name, "research");
            assert_eq!(workspace, Some("/home/harper/ws/research".to_string()));
        }
        _ => panic!("Expected Create"),
    }
}

#[test]
fn test_parse_dispatch_join() {
    let cmd = DispatchCommand::parse("!join research");
    assert!(matches!(cmd, DispatchCommand::Join { ref name } if name == "research"));
}

#[test]
fn test_parse_dispatch_leave() {
    let cmd = DispatchCommand::parse("!leave");
    assert!(matches!(cmd, DispatchCommand::Leave));
}

#[test]
fn test_parse_dispatch_list() {
    let cmd = DispatchCommand::parse("!list");
    assert!(matches!(cmd, DispatchCommand::List));
}

#[test]
fn test_parse_dispatch_status() {
    let cmd = DispatchCommand::parse("!status research");
    assert!(matches!(cmd, DispatchCommand::Status { ref name } if name == "research"));
}

#[test]
fn test_parse_dispatch_tell() {
    let cmd = DispatchCommand::parse("!tell research summarize the paper");
    match cmd {
        DispatchCommand::Tell { session, message } => {
            assert_eq!(session, "research");
            assert_eq!(message, "summarize the paper");
        }
        _ => panic!("Expected Tell"),
    }
}

#[test]
fn test_parse_dispatch_read() {
    let cmd = DispatchCommand::parse("!read research 5");
    match cmd {
        DispatchCommand::Read { session, count } => {
            assert_eq!(session, "research");
            assert_eq!(count, Some(5));
        }
        _ => panic!("Expected Read"),
    }
}

#[test]
fn test_parse_dispatch_read_default_count() {
    let cmd = DispatchCommand::parse("!read research");
    match cmd {
        DispatchCommand::Read { session, count } => {
            assert_eq!(session, "research");
            assert_eq!(count, None);
        }
        _ => panic!("Expected Read"),
    }
}

#[test]
fn test_parse_dispatch_broadcast() {
    let cmd = DispatchCommand::parse("!broadcast hey everyone");
    assert!(matches!(cmd, DispatchCommand::Broadcast { ref message } if message == "hey everyone"));
}

#[test]
fn test_parse_dispatch_delete() {
    let cmd = DispatchCommand::parse("!delete research");
    assert!(matches!(cmd, DispatchCommand::Delete { ref name } if name == "research"));
}

#[test]
fn test_parse_dispatch_help() {
    let cmd = DispatchCommand::parse("!help");
    assert!(matches!(cmd, DispatchCommand::Help));
}

#[test]
fn test_parse_dispatch_unknown() {
    let cmd = DispatchCommand::parse("hello there");
    assert!(matches!(cmd, DispatchCommand::Unknown(_)));
}

#[test]
fn test_parse_dispatch_unknown_command() {
    let cmd = DispatchCommand::parse("!frobnicate");
    assert!(matches!(cmd, DispatchCommand::Unknown(_)));
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test --test orchestrator_tests`
Expected: FAIL

**Step 3: Implement DispatchCommand parser**

```rust
// src/orchestrator.rs
// ABOUTME: Message bus orchestrator — consumes inbound messages, routes to agent sessions or DISPATCH.
// ABOUTME: DISPATCH is a built-in command handler for session lifecycle and supervisor operations.

/// DISPATCH commands parsed from message bodies.
#[derive(Debug, PartialEq)]
pub enum DispatchCommand {
    /// Create a new agent session
    Create { name: String, workspace: Option<String> },
    /// Delete a session
    Delete { name: String },
    /// List all active sessions
    List,
    /// Show status of a session
    Status { name: String },
    /// Bind this platform channel to a session
    Join { name: String },
    /// Unbind this platform channel
    Leave,
    /// Inject a message into another session
    Tell { session: String, message: String },
    /// Read recent messages from a session
    Read { session: String, count: Option<usize> },
    /// Send a message to all active sessions
    Broadcast { message: String },
    /// Show available commands
    Help,
    /// Unrecognized input
    Unknown(String),
}

impl DispatchCommand {
    /// Parse a message body into a dispatch command.
    pub fn parse(input: &str) -> Self {
        let input = input.trim();

        if !input.starts_with('!') {
            return Self::Unknown(input.to_string());
        }

        let parts: Vec<&str> = input.splitn(3, ' ').collect();
        let cmd = parts[0].to_lowercase();

        match cmd.as_str() {
            "!create" => {
                let name = parts.get(1).map(|s| s.to_string());
                let workspace = parts.get(2).map(|s| s.to_string());
                match name {
                    Some(name) => Self::Create { name, workspace },
                    None => Self::Unknown(input.to_string()),
                }
            }
            "!delete" => {
                parts.get(1)
                    .map(|name| Self::Delete { name: name.to_string() })
                    .unwrap_or(Self::Unknown(input.to_string()))
            }
            "!list" => Self::List,
            "!status" => {
                parts.get(1)
                    .map(|name| Self::Status { name: name.to_string() })
                    .unwrap_or(Self::Unknown(input.to_string()))
            }
            "!join" => {
                parts.get(1)
                    .map(|name| Self::Join { name: name.to_string() })
                    .unwrap_or(Self::Unknown(input.to_string()))
            }
            "!leave" => Self::Leave,
            "!tell" => {
                let session = parts.get(1).map(|s| s.to_string());
                let message = parts.get(2).map(|s| s.to_string());
                match (session, message) {
                    (Some(session), Some(message)) => Self::Tell { session, message },
                    _ => Self::Unknown(input.to_string()),
                }
            }
            "!read" => {
                let session = parts.get(1).map(|s| s.to_string());
                let count = parts.get(2).and_then(|s| s.parse().ok());
                match session {
                    Some(session) => Self::Read { session, count },
                    None => Self::Unknown(input.to_string()),
                }
            }
            "!broadcast" => {
                // Rejoin parts 1+ as the message
                let message = if input.len() > "!broadcast ".len() {
                    input["!broadcast ".len()..].to_string()
                } else {
                    return Self::Unknown(input.to_string());
                };
                Self::Broadcast { message }
            }
            "!help" => Self::Help,
            _ => Self::Unknown(input.to_string()),
        }
    }
}
```

Add to `src/lib.rs`:
```rust
pub mod orchestrator;
```

**Step 4: Run tests to verify they pass**

Run: `cargo test --test orchestrator_tests`
Expected: PASS — all tests green

**Step 5: Commit**

```bash
git add src/orchestrator.rs src/lib.rs tests/orchestrator_tests.rs
git commit -m "feat: add DispatchCommand parser for orchestrator"
```

---

### Task 6: Orchestrator Run Loop

**Files:**
- Modify: `src/orchestrator.rs`
- Test: `tests/orchestrator_tests.rs` (append)

This task adds the `Orchestrator` struct and its main `run()` loop that consumes from the bus, deduplicates, and routes messages. The agent invocation (warm session manager integration) will be wired in a later task.

**Step 1: Write failing tests**

```rust
// append to tests/orchestrator_tests.rs
use gorp::orchestrator::Orchestrator;
use std::sync::Arc;
use tokio::time::{timeout, Duration};

#[tokio::test]
async fn test_orchestrator_routes_dispatch_messages() {
    let bus = Arc::new(MessageBus::new(64));
    let orchestrator = Orchestrator::new(bus.clone());

    // Start orchestrator in background
    let orch = orchestrator.clone();
    let handle = tokio::spawn(async move { orch.run().await });

    // Subscribe to responses
    let mut rx = bus.subscribe_responses();

    // Send a dispatch message
    bus.publish_inbound(BusMessage {
        id: "evt-1".to_string(),
        source: MessageSource::Web { connection_id: "ws-1".to_string() },
        session_target: SessionTarget::Dispatch,
        sender: "harper".to_string(),
        body: "!help".to_string(),
        timestamp: Utc::now(),
    });

    // Should receive a SystemNotice response
    let resp = timeout(Duration::from_secs(2), rx.recv()).await
        .expect("timed out")
        .expect("recv failed");
    assert!(matches!(resp.content, ResponseContent::SystemNotice(_)));

    handle.abort();
}

#[tokio::test]
async fn test_orchestrator_deduplicates_messages() {
    let bus = Arc::new(MessageBus::new(64));
    let orchestrator = Orchestrator::new(bus.clone());

    let orch = orchestrator.clone();
    let handle = tokio::spawn(async move { orch.run().await });

    let mut rx = bus.subscribe_responses();

    // Send same message twice
    for _ in 0..2 {
        bus.publish_inbound(BusMessage {
            id: "evt-dup".to_string(),
            source: MessageSource::Web { connection_id: "ws-1".to_string() },
            session_target: SessionTarget::Dispatch,
            sender: "harper".to_string(),
            body: "!help".to_string(),
            timestamp: Utc::now(),
        });
    }

    // Should only receive ONE response (dedup)
    let _ = timeout(Duration::from_millis(500), rx.recv()).await
        .expect("should receive first");
    let second = timeout(Duration::from_millis(200), rx.recv()).await;
    assert!(second.is_err(), "should NOT receive duplicate");

    handle.abort();
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test --test orchestrator_tests test_orchestrator`
Expected: FAIL — `Orchestrator` not found

**Step 3: Implement Orchestrator struct and run loop**

Add to `src/orchestrator.rs`:

```rust
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::bus::{BusMessage, BusResponse, MessageBus, ResponseContent, SessionTarget};

/// The orchestrator consumes inbound messages from the bus,
/// deduplicates them, and routes to agent sessions or DISPATCH.
#[derive(Clone)]
pub struct Orchestrator {
    bus: Arc<MessageBus>,
    seen_ids: Arc<Mutex<HashSet<String>>>,
}

impl Orchestrator {
    pub fn new(bus: Arc<MessageBus>) -> Self {
        Self {
            bus,
            seen_ids: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    /// Main loop — consumes from the bus forever.
    pub async fn run(&self) {
        let mut rx = self.bus.subscribe_inbound();
        loop {
            match rx.recv().await {
                Ok(msg) => {
                    // Dedup by message ID
                    {
                        let mut seen = self.seen_ids.lock().await;
                        if seen.contains(&msg.id) {
                            tracing::debug!(id = %msg.id, "Dropping duplicate message");
                            continue;
                        }
                        seen.insert(msg.id.clone());
                        // Prevent unbounded growth
                        if seen.len() > 10_000 {
                            seen.clear();
                        }
                    }

                    let this = self.clone();
                    tokio::spawn(async move {
                        this.handle(msg).await;
                    });
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!(skipped = n, "Orchestrator lagged, skipped messages");
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    tracing::info!("Bus closed, orchestrator shutting down");
                    break;
                }
            }
        }
    }

    async fn handle(&self, msg: BusMessage) {
        match msg.session_target {
            SessionTarget::Dispatch => self.handle_dispatch(msg).await,
            SessionTarget::Session { ref name } => {
                self.handle_agent_message(name.clone(), msg).await;
            }
        }
    }

    async fn handle_dispatch(&self, msg: BusMessage) {
        let cmd = DispatchCommand::parse(&msg.body);
        let response_text = match cmd {
            DispatchCommand::Help => {
                "Available commands:\n\
                 !create <name> [workspace] - Create a new agent session\n\
                 !delete <name> - Delete a session\n\
                 !list - List active sessions\n\
                 !status <name> - Session details\n\
                 !join <name> - Bind this channel to a session\n\
                 !leave - Unbind this channel\n\
                 !tell <session> <message> - Send a message to another session\n\
                 !read <session> [count] - Read recent messages from a session\n\
                 !broadcast <message> - Send to all sessions\n\
                 !help - Show this help".to_string()
            }
            DispatchCommand::List => {
                // Will be wired to SessionStore in a later task
                "Active sessions: (none yet - wire to session store)".to_string()
            }
            DispatchCommand::Unknown(text) => {
                format!("Unknown command: '{}'. Use !help to see available commands.", text)
            }
            // Other commands will be implemented when wired to SessionStore + WarmSessionManager
            _ => format!("Command recognized but not yet wired: {:?}", cmd),
        };

        self.bus.publish_response(BusResponse {
            session_name: String::new(), // DISPATCH responses have no session
            content: ResponseContent::SystemNotice(response_text),
            timestamp: chrono::Utc::now(),
        });
    }

    async fn handle_agent_message(&self, session_name: String, msg: BusMessage) {
        // Will be wired to WarmSessionManager in a later task
        tracing::info!(session = %session_name, sender = %msg.sender, "Agent message (not yet wired)");
        self.bus.publish_response(BusResponse {
            session_name,
            content: ResponseContent::SystemNotice("Agent routing not yet wired".to_string()),
            timestamp: chrono::Utc::now(),
        });
    }
}
```

**Step 4: Run tests to verify they pass**

Run: `cargo test --test orchestrator_tests`
Expected: PASS

**Step 5: Commit**

```bash
git add src/orchestrator.rs tests/orchestrator_tests.rs
git commit -m "feat: add Orchestrator with dedup, dispatch routing, and run loop"
```

---

### Task 7: Wire Orchestrator to SessionStore and WarmSessionManager

**Files:**
- Modify: `src/orchestrator.rs`
- Test: `tests/orchestrator_tests.rs` (append)

This task wires the DISPATCH commands (`!create`, `!delete`, `!list`, `!join`, `!leave`, `!status`) to the real SessionStore and connects agent message routing to the WarmSessionManager.

**Step 1: Write failing integration tests**

Tests should use a real in-memory SessionStore and verify that `!create` actually creates a session, `!list` returns it, `!join` binds a channel, etc. These tests need the Orchestrator constructor to accept a SessionStore.

**Step 2: Update Orchestrator constructor**

```rust
pub struct Orchestrator {
    bus: Arc<MessageBus>,
    seen_ids: Arc<Mutex<HashSet<String>>>,
    session_store: SessionStore,
    warm_manager: Option<SharedWarmSessionManager>,
}

impl Orchestrator {
    pub fn new(
        bus: Arc<MessageBus>,
        session_store: SessionStore,
        warm_manager: Option<SharedWarmSessionManager>,
    ) -> Self { ... }
}
```

**Step 3: Implement DISPATCH commands against real stores**

Wire each `DispatchCommand` variant:
- `Create` → `session_store.create_channel()` (or new `create_session()` method)
- `Delete` → `session_store.delete_channel()`
- `List` → `session_store.list_all()`
- `Status` → `session_store.get_by_name()` + warm_manager status
- `Join` → `bus.bind_channel_async()` + `session_store.bind_channel()`
- `Leave` → `bus.unbind_channel()` + `session_store.unbind_channel()`
- `Tell` → publish a new `BusMessage` targeting the named session
- `Read` → session store message history (if stored) or agent session logs

**Step 4: Wire agent message routing**

In `handle_agent_message()`:
```rust
// 1. Get or create warm session via warm_manager
// 2. Send prompt
// 3. Stream response chunks as BusResponse::Chunk
// 4. Publish BusResponse::Complete when done
```

This follows the same pattern as `send_prompt_with_handle()` in the current `message_handler/mod.rs` but publishes to the bus instead of calling `platform.send()`.

**Step 5: Run tests, commit**

```bash
git commit -m "feat: wire orchestrator to session store and warm session manager"
```

---

### Task 8: Web Gateway Adapter

**Files:**
- Create: `src/gateway/web.rs`
- Modify: `src/gateway/mod.rs`
- Modify: `src/admin/websocket.rs`
- Test: inline or `tests/gateway_web_tests.rs`

The web adapter translates between WebSocket `ClientMessage::ChatSend` and bus types. This completes the web chat wiring that was previously stubbed.

**Step 1: Implement WebAdapter**

```rust
// src/gateway/web.rs
// ABOUTME: Web gateway adapter — translates between admin WebSocket and the message bus.
// ABOUTME: Always started regardless of platform config. Handles web chat UI connections.

pub struct WebAdapter {
    ws_hub: WsHub,
}

#[async_trait]
impl GatewayAdapter for WebAdapter {
    fn platform_id(&self) -> &str { "web" }

    async fn start(&self, bus: Arc<MessageBus>) -> Result<()> {
        // Spawn outbound loop: subscribe to bus responses,
        // convert to ServerMessage::ChatChunk / ChatComplete,
        // broadcast via WsHub
    }

    async fn send(&self, channel_id: &str, content: ResponseContent) -> Result<()> {
        // Convert ResponseContent to ServerMessage, broadcast via WsHub
    }

    async fn stop(&self) -> Result<()> { Ok(()) }
}
```

**Step 2: Wire ChatSend in websocket.rs**

Change the `ChatSend` handler from logging to publishing to the bus:
```rust
ClientMessage::ChatSend { workspace, body } => {
    let msg = BusMessage {
        id: uuid::Uuid::new_v4().to_string(),
        source: MessageSource::Web { connection_id: connection_id.clone() },
        session_target: bus.resolve_target_async("web", &workspace).await,
        sender: "admin".to_string(),
        body,
        timestamp: Utc::now(),
    };
    bus.publish_inbound(msg);
}
```

**Step 3: Wire outbound responses to WsHub**

The outbound loop subscribes to `bus.subscribe_responses()` and broadcasts `ServerMessage::ChatChunk` / `ServerMessage::ChatComplete` events through the existing WsHub infrastructure.

**Step 4: Tests — verify web chat round-trip**

Test: send ChatSend via bus, verify orchestrator processes it, verify response comes back via WsHub.

**Step 5: Commit**

```bash
git commit -m "feat: add WebAdapter and wire web chat to message bus"
```

---

### Task 9: Matrix Gateway Adapter

**Files:**
- Create: `src/gateway/matrix.rs`
- Modify: `src/gateway/mod.rs`
- Test: `tests/gateway_matrix_tests.rs`

Refactor the Matrix sync loop from `main.rs` into a self-contained `MatrixAdapter`.

**Step 1: Extract sync loop into MatrixAdapter::start()**

Move the Matrix sync loop (currently ~lines 1426-1700 of main.rs) into:
```rust
// src/gateway/matrix.rs
pub struct MatrixAdapter {
    client: Client,
    config: MatrixConfig,
}

#[async_trait]
impl GatewayAdapter for MatrixAdapter {
    fn platform_id(&self) -> &str { "matrix" }

    async fn start(&self, bus: Arc<MessageBus>) -> Result<()> {
        // 1. Set up event handlers that publish BusMessage to bus
        // 2. Spawn sync loop task
        // 3. Return immediately
    }

    async fn send(&self, channel_id: &str, content: ResponseContent) -> Result<()> {
        // channel_id is a Matrix room_id
        // Convert ResponseContent to Matrix message, send via client
    }

    async fn stop(&self) -> Result<()> {
        // Stop sync loop
    }
}
```

**Step 2: Convert Matrix event handler to bus publisher**

The current `register_event_handlers` creates a `mpsc::channel` and sends Matrix events through it. Replace this with publishing `BusMessage` to the bus. The dedup happens in the orchestrator now, not in the event handler.

**Step 3: Convert Matrix message sending**

The `send()` method translates `ResponseContent` variants to Matrix room messages:
- `Chunk` / `Complete` → `room.send(RoomMessageEventContent::text_html(...))`
- `Error` → formatted error message
- `SystemNotice` → notice-style message

**Step 4: Tests**

Unit tests for the adapter creation (mock-friendly). Integration tests for Matrix require a real homeserver, so focus on the type conversion and event normalization logic.

**Step 5: Commit**

```bash
git commit -m "feat: add MatrixAdapter — extract sync loop from main.rs"
```

---

### Task 10: Slack and Telegram Adapters

**Files:**
- Create: `src/gateway/slack.rs`
- Create: `src/gateway/telegram.rs`
- Modify: `src/gateway/mod.rs`

Same pattern as Matrix. Extract existing `SlackPlatform` and `TelegramPlatform` into adapters. These are simpler since they already use the `MessagingPlatform` trait — the refactor is mostly renaming and wiring to the bus instead of the old event stream.

**Step 1-3:** Follow same pattern as Task 9 for each platform.

**Step 4: Commit**

```bash
git commit -m "feat: add Slack and Telegram gateway adapters"
```

---

### Task 11: Scheduler Bus Integration

**Files:**
- Modify: `src/scheduler.rs`
- Test: existing scheduler tests + new bus integration tests

The scheduler currently calls into the warm session manager directly. Change it to publish `BusMessage` events to the bus when scheduled prompts fire.

**Step 1: Change scheduler to accept Arc<MessageBus>**

```rust
pub async fn start_scheduler(
    scheduler_store: SchedulerStore,
    session_store: SessionStore,
    bus: Arc<MessageBus>,
    config: Arc<Config>,
    check_interval: Duration,
    warm_manager: SharedWarmSessionManager,
)
```

**Step 2: On prompt execution, publish to bus**

When a scheduled prompt fires:
```rust
let msg = BusMessage {
    id: format!("sched-{}-{}", schedule.id, execution_count),
    source: MessageSource::Api { token_hint: "scheduler".to_string() },
    session_target: SessionTarget::Session { name: schedule.channel_name.clone() },
    sender: schedule.created_by.clone(),
    body: schedule.prompt.clone(),
    timestamp: Utc::now(),
};
bus.publish_inbound(msg);
```

The orchestrator handles routing to the agent session. The scheduler no longer needs direct access to the warm session manager (the orchestrator owns that).

**Step 3: Tests, commit**

```bash
git commit -m "feat: scheduler publishes to message bus instead of direct agent invocation"
```

---

### Task 12: Webhook Bus Integration

**Files:**
- Modify: `src/webhook.rs`
- Test: existing webhook tests

Same pattern as scheduler. The webhook handler publishes `BusMessage` to the bus instead of managing its own job queue.

**Step 1: Simplify WebhookState**

```rust
struct WebhookState {
    session_store: SessionStore,
    bus: Arc<MessageBus>,
    config: Arc<Config>,
}
```

No more `matrix_client`, no more `WebhookJob` mpsc channel. The bus handles routing.

**Step 2: Webhook handler publishes to bus**

```rust
async fn handle_webhook(
    State(state): State<WebhookState>,
    Path(session_id): Path<String>,
    Json(req): Json<WebhookRequest>,
) -> Json<WebhookResponse> {
    let msg = BusMessage {
        id: uuid::Uuid::new_v4().to_string(),
        source: MessageSource::Api { token_hint: "webhook".to_string() },
        session_target: SessionTarget::Session { name: channel.channel_name },
        sender: "webhook".to_string(),
        body: req.prompt,
        timestamp: Utc::now(),
    };
    state.bus.publish_inbound(msg);
    // For sync webhook response, subscribe to bus and wait for Complete
}
```

**Step 3: Tests, commit**

```bash
git commit -m "feat: webhook publishes to message bus"
```

---

### Task 13: Rewire main.rs

**Files:**
- Modify: `src/main.rs`

This is the big rewire. Replace the 1700-line `run_start()` with the new lifecycle:

```rust
async fn run_start() -> Result<()> {
    // 1. Load config
    let config = Config::load()?;
    let config = Arc::new(config);

    // 2. Open SQLite, create stores
    let session_store = SessionStore::new(&workspace_path)?;
    let scheduler_store = SchedulerStore::new(session_store.connection())?;

    // 3. Create MessageBus, load bindings from DB
    let bus = Arc::new(MessageBus::new(256));
    let bindings = session_store.list_all_bindings()?;
    bus.load_bindings(bindings).await;

    // 4. Create WarmSessionManager
    let warm_manager = ...;

    // 5. Create Orchestrator
    let orchestrator = Orchestrator::new(bus.clone(), session_store.clone(), Some(warm_manager));

    // 6. Create and start gateway adapters
    let mut registry = GatewayRegistry::new();

    if let Some(ref matrix_config) = config.matrix {
        let client = create_matrix_client(matrix_config).await?;
        let adapter = MatrixAdapter::new(client, matrix_config.clone());
        adapter.start(bus.clone()).await?;
        registry.register(Box::new(adapter));
    }
    // ... same for Telegram, Slack

    // 7. Start web adapter (always)
    let web_adapter = WebAdapter::new(ws_hub.clone());
    web_adapter.start(bus.clone()).await?;

    // 8. Start webhook server
    start_webhook_server(session_store.clone(), bus.clone(), config.clone()).await?;

    // 9. Start admin panel
    start_admin_server(...).await?;

    // 10. Start scheduler
    start_scheduler(scheduler_store, session_store.clone(), bus.clone(), config.clone(), ...).await;

    // 11. Start orchestrator (main loop)
    let orch = orchestrator.clone();
    tokio::spawn(async move { orch.run().await });

    // 12. Wait for shutdown
    tracing::info!("gorp running — admin panel at http://localhost:{}/admin", webhook_port);
    tokio::signal::ctrl_c().await?;

    // 13. Cleanup
    registry.shutdown_all().await;
    Ok(())
}
```

**Step 1: Incrementally replace run_start()**

Keep the old code commented/gated during transition. Wire new components one at a time.

**Step 2: Verify boot with no platforms**

Run: `cargo run` with empty config
Expected: Starts, shows admin panel URL, web chat connects to DISPATCH

**Step 3: Verify boot with Matrix**

Run: `cargo run` with `[matrix]` config
Expected: Matrix sync loop starts via adapter, messages route through bus

**Step 4: Commit**

```bash
git commit -m "feat: rewire main.rs to message bus architecture"
```

---

### Task 14: Admin Panel Updates

**Files:**
- Modify: `src/admin/routes.rs`
- Modify: `src/admin/templates.rs`
- Modify: various `templates/admin/*.html`

Update the admin panel to reflect the new architecture:
- Dashboard shows sessions (not "channels") and connected gateways
- Channel list becomes session list
- Channel detail shows which platforms are bound
- Health view shows gateway adapter status

**Step 1: Update terminology in templates**

- "Channels" → "Sessions" throughout
- "Room ID" → "Bound Platforms" (show list of bindings)
- Channel create form → Session create form

**Step 2: Update route handlers to use new store methods**

**Step 3: Commit**

```bash
git commit -m "feat: update admin panel for message bus architecture"
```

---

### Task 15: Delete Old Code

**Files:**
- Delete: `src/message_handler/generic_channel.rs`
- Delete: old `MessagingPlatform` trait (if fully replaced)
- Delete: old `PlatformRegistry` (if fully replaced)
- Delete: old `ServerState` in `src/server.rs` (if fully replaced)
- Clean: `src/main.rs` — remove commented-out old code

Only delete after all tests pass with new architecture.

**Step 1: Remove dead code**

**Step 2: Run full test suite**

Run: `cargo test`
Expected: All tests pass

**Step 3: Commit**

```bash
git commit -m "refactor: remove legacy message handler and platform registry"
```

---

### Task 16: End-to-End Verification

**No code changes — verification only.**

**Test 1: Zero-config boot**
```bash
# Empty config (no [matrix], no [telegram], no [slack])
cargo run
# Expected: boots, shows admin panel URL
# Open browser → admin panel works
# Go to chat → connected to DISPATCH
# Type !help → see command list
# Type !create test → session created
# Type !join test → bound to session
# Type "hello" → agent responds
```

**Test 2: Matrix gateway**
```bash
# Config with [matrix] section
cargo run
# Expected: Matrix sync loop starts
# DM bot from Matrix → lands in DISPATCH
# !join test → Matrix room bound to session
# Message from Matrix → agent responds
# Response visible in BOTH Matrix and web chat
```

**Test 3: Scheduler**
```bash
# Create a schedule via admin panel
# Wait for it to fire
# Verify response broadcasts to all bound platforms
```

**Test 4: Webhook**
```bash
curl -X POST http://localhost:PORT/webhook/session/SESSION_ID \
  -H 'Content-Type: application/json' \
  -d '{"prompt": "hello"}'
# Verify response and bus broadcast
```
