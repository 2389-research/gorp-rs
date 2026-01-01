# DISPATCH Control Plane Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Implement a control plane agent in the 1:1 DM that orchestrates and monitors all workspace rooms.

**Architecture:** DISPATCH is a mux-powered agent with cross-room visibility. It runs in the 1:1 DM room, receives routed events from workers, and can dispatch tasks to any workspace room. It has no workspace directory - pure coordination.

**Tech Stack:** Rust, mux backend, Matrix SDK, SQLite, serde_json for event serialization

---

## Phase 1: Database Schema & Detection

### Task 1.1: Add is_dispatch_room flag to channels table

**Files:**
- Modify: `gorp-core/src/session.rs`

**Step 1: Add migration for is_dispatch_room column**

In `SessionStore::new()`, add migration after the backend_type migration:

```rust
// Migration: Add is_dispatch_room column for control plane detection
let _ = conn.execute("ALTER TABLE channels ADD COLUMN is_dispatch_room INTEGER DEFAULT 0", []);
```

**Step 2: Update Channel struct**

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Channel {
    pub channel_name: String,
    pub room_id: String,
    pub session_id: String,
    pub directory: String,
    pub started: bool,
    pub created_at: String,
    pub backend_type: Option<String>,
    /// True if this is the DISPATCH control plane room (1:1 DM)
    pub is_dispatch_room: bool,
}
```

**Step 3: Update all SELECT queries to include is_dispatch_room**

Update `get_by_room`, `get_by_name`, `list_all`, `get_by_session_id` to include:
```rust
row.get::<_, i32>(7)? != 0  // is_dispatch_room
```

**Step 4: Run tests**

Run: `cargo test -p gorp-core`
Expected: All existing tests pass

**Step 5: Commit**

```bash
git add gorp-core/src/session.rs
git commit -m "feat(dispatch): add is_dispatch_room column to channels table"
```

---

### Task 1.2: Create dispatch_events table

**Files:**
- Modify: `gorp-core/src/session.rs`

**Step 1: Add dispatch_events table creation in SessionStore::new()**

```rust
// Create dispatch_events table for tracking events routed to DISPATCH
conn.execute(
    "CREATE TABLE IF NOT EXISTS dispatch_events (
        id TEXT PRIMARY KEY,
        source_room_id TEXT NOT NULL,
        event_type TEXT NOT NULL,
        payload TEXT NOT NULL,
        created_at TEXT NOT NULL,
        acknowledged_at TEXT
    )",
    [],
)?;
```

**Step 2: Add DispatchEvent struct**

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DispatchEvent {
    pub id: String,
    pub source_room_id: String,
    pub event_type: String,
    pub payload: serde_json::Value,
    pub created_at: String,
    pub acknowledged_at: Option<String>,
}
```

**Step 3: Add CRUD methods for dispatch events**

```rust
impl SessionStore {
    pub fn insert_dispatch_event(&self, event: &DispatchEvent) -> Result<()>;
    pub fn get_pending_dispatch_events(&self) -> Result<Vec<DispatchEvent>>;
    pub fn acknowledge_dispatch_event(&self, id: &str) -> Result<()>;
}
```

**Step 4: Run tests**

Run: `cargo test -p gorp-core`
Expected: All tests pass

**Step 5: Commit**

```bash
git add gorp-core/src/session.rs
git commit -m "feat(dispatch): add dispatch_events table and CRUD methods"
```

---

### Task 1.3: Create dispatch_tasks table

**Files:**
- Modify: `gorp-core/src/session.rs`

**Step 1: Add dispatch_tasks table creation**

```rust
// Create dispatch_tasks table for tracking dispatched work
conn.execute(
    "CREATE TABLE IF NOT EXISTS dispatch_tasks (
        id TEXT PRIMARY KEY,
        target_room_id TEXT NOT NULL,
        prompt TEXT NOT NULL,
        status TEXT NOT NULL,
        created_at TEXT NOT NULL,
        completed_at TEXT,
        result_summary TEXT
    )",
    [],
)?;
```

**Step 2: Add DispatchTask struct and TaskStatus enum**

```rust
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DispatchTaskStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DispatchTask {
    pub id: String,
    pub target_room_id: String,
    pub prompt: String,
    pub status: DispatchTaskStatus,
    pub created_at: String,
    pub completed_at: Option<String>,
    pub result_summary: Option<String>,
}
```

**Step 3: Add CRUD methods for dispatch tasks**

```rust
impl SessionStore {
    pub fn create_dispatch_task(&self, target_room_id: &str, prompt: &str) -> Result<DispatchTask>;
    pub fn get_dispatch_task(&self, id: &str) -> Result<Option<DispatchTask>>;
    pub fn update_dispatch_task_status(&self, id: &str, status: DispatchTaskStatus, result: Option<&str>) -> Result<()>;
    pub fn list_dispatch_tasks(&self, status: Option<DispatchTaskStatus>) -> Result<Vec<DispatchTask>>;
}
```

**Step 4: Run tests**

Run: `cargo test -p gorp-core`
Expected: All tests pass

**Step 5: Commit**

```bash
git add gorp-core/src/session.rs
git commit -m "feat(dispatch): add dispatch_tasks table and task tracking"
```

---

## Phase 2: DISPATCH Room Detection & Initialization

### Task 2.1: Create DISPATCH room on first DM interaction

**Files:**
- Modify: `src/message_handler.rs`
- Modify: `gorp-core/src/session.rs`

**Step 1: Add method to check/create DISPATCH channel for user**

In `session.rs`, add:

```rust
impl SessionStore {
    /// Get the DISPATCH channel for this user's 1:1 DM room
    pub fn get_dispatch_channel(&self, room_id: &str) -> Result<Option<Channel>> {
        // Look for channel where is_dispatch_room = true AND room_id matches
    }

    /// Create DISPATCH channel (no workspace directory)
    pub fn create_dispatch_channel(&self, room_id: &str) -> Result<Channel> {
        // Create with is_dispatch_room = true
        // directory = "" (no workspace)
        // channel_name = "dispatch"
    }
}
```

**Step 2: Update handle_message to detect DISPATCH room**

In the DM handling code, before onboarding check:

```rust
// Check if this DM room is (or should be) a DISPATCH room
if is_dm {
    let dispatch = session_store.get_dispatch_channel(room.room_id().as_str())?;
    if dispatch.is_some() || should_activate_dispatch(body) {
        return handle_dispatch_message(room, event, client, config, session_store, warm_manager).await;
    }
}
```

**Step 3: Run tests**

Run: `cargo test`
Expected: All tests pass

**Step 4: Commit**

```bash
git add gorp-core/src/session.rs src/message_handler.rs
git commit -m "feat(dispatch): add DISPATCH room detection and initialization"
```

---

### Task 2.2: Create DISPATCH message handler skeleton

**Files:**
- Create: `src/dispatch_handler.rs`
- Modify: `src/lib.rs`

**Step 1: Create dispatch_handler.rs**

```rust
// ABOUTME: DISPATCH control plane message handler for orchestrating workspace rooms.
// ABOUTME: Runs in 1:1 DM, provides cross-room visibility and task dispatch.

use anyhow::Result;
use matrix_sdk::{room::Room, Client};

use crate::{
    config::Config,
    session::SessionStore,
    warm_session::SharedWarmSessionManager,
};

/// Handle a message in the DISPATCH control plane room
pub async fn handle_dispatch_message(
    room: Room,
    event: matrix_sdk::ruma::events::room::message::OriginalSyncRoomMessageEvent,
    client: Client,
    config: Config,
    session_store: SessionStore,
    warm_manager: SharedWarmSessionManager,
) -> Result<()> {
    // TODO: Implement DISPATCH agent invocation
    tracing::info!("DISPATCH message received - not yet implemented");
    Ok(())
}
```

**Step 2: Add to lib.rs**

```rust
pub mod dispatch_handler;
```

**Step 3: Run build**

Run: `cargo build`
Expected: Build succeeds

**Step 4: Commit**

```bash
git add src/dispatch_handler.rs src/lib.rs
git commit -m "feat(dispatch): create dispatch_handler skeleton"
```

---

## Phase 3: DISPATCH MCP Tools

### Task 3.1: Create DISPATCH tools module

**Files:**
- Create: `src/dispatch_tools.rs`
- Modify: `src/lib.rs`

**Step 1: Create dispatch_tools.rs with tool definitions**

```rust
// ABOUTME: MCP tools for DISPATCH control plane - room queries and task dispatch.
// ABOUTME: These tools give DISPATCH cross-room visibility without filesystem access.

use serde::{Deserialize, Serialize};
use crate::session::{Channel, SessionStore, DispatchTask, DispatchTaskStatus};

/// Room information for DISPATCH
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoomInfo {
    pub room_id: String,
    pub channel_name: String,
    pub workspace_path: String,
    pub last_activity: Option<String>,
    pub agent_status: AgentStatus,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentStatus {
    Idle,
    Working,
    WaitingInput,
    Error,
}

/// Tool: list_rooms - List all active workspace rooms
pub fn list_rooms(session_store: &SessionStore) -> Result<Vec<RoomInfo>, String> {
    let channels = session_store.list_all()
        .map_err(|e| e.to_string())?;

    Ok(channels.into_iter()
        .filter(|c| !c.is_dispatch_room)
        .map(|c| RoomInfo {
            room_id: c.room_id,
            channel_name: c.channel_name,
            workspace_path: c.directory,
            last_activity: None, // TODO: track this
            agent_status: AgentStatus::Idle,
        })
        .collect())
}

/// Tool: get_room_status - Get detailed status of a specific room
pub fn get_room_status(session_store: &SessionStore, room_id: &str) -> Result<RoomInfo, String> {
    let channel = session_store.get_by_room(room_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Room not found: {}", room_id))?;

    Ok(RoomInfo {
        room_id: channel.room_id,
        channel_name: channel.channel_name,
        workspace_path: channel.directory,
        last_activity: None,
        agent_status: AgentStatus::Idle,
    })
}
```

**Step 2: Add to lib.rs**

```rust
pub mod dispatch_tools;
```

**Step 3: Run build**

Run: `cargo build`
Expected: Build succeeds

**Step 4: Commit**

```bash
git add src/dispatch_tools.rs src/lib.rs
git commit -m "feat(dispatch): add DISPATCH MCP tools module"
```

---

### Task 3.2: Implement dispatch_task tool

**Files:**
- Modify: `src/dispatch_tools.rs`

**Step 1: Add dispatch_task function**

```rust
/// Tool: dispatch_task - Send a task to a worker room
pub async fn dispatch_task(
    session_store: &SessionStore,
    room_id: &str,
    prompt: &str,
    client: &matrix_sdk::Client,
) -> Result<DispatchTask, String> {
    // Create task record
    let task = session_store.create_dispatch_task(room_id, prompt)
        .map_err(|e| e.to_string())?;

    // Send message to room
    let room = client.get_room(&room_id.parse().map_err(|e| format!("{}", e))?)
        .ok_or_else(|| format!("Room not found: {}", room_id))?;

    let content = matrix_sdk::ruma::events::room::message::RoomMessageEventContent::text_plain(prompt);
    room.send(content).await.map_err(|e| e.to_string())?;

    // Update task status to in_progress
    session_store.update_dispatch_task_status(&task.id, DispatchTaskStatus::InProgress, None)
        .map_err(|e| e.to_string())?;

    Ok(task)
}

/// Tool: check_task - Check status of a dispatched task
pub fn check_task(session_store: &SessionStore, task_id: &str) -> Result<DispatchTask, String> {
    session_store.get_dispatch_task(task_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Task not found: {}", task_id))
}
```

**Step 2: Run build**

Run: `cargo build`
Expected: Build succeeds

**Step 3: Commit**

```bash
git add src/dispatch_tools.rs
git commit -m "feat(dispatch): implement dispatch_task and check_task tools"
```

---

### Task 3.3: Implement admin tools (create_room, reset_room)

**Files:**
- Modify: `src/dispatch_tools.rs`

**Step 1: Add create_room function**

```rust
/// Tool: create_room - Create a new workspace room
pub async fn create_room(
    session_store: &SessionStore,
    client: &matrix_sdk::Client,
    name: &str,
    workspace_path: Option<&str>,
    config: &crate::config::Config,
) -> Result<RoomInfo, String> {
    // Validate name
    if !name.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_') {
        return Err("Invalid room name: must be alphanumeric with dashes/underscores".to_string());
    }

    // Create Matrix room
    let room_name = format!("{}: {}", config.matrix.room_prefix, name);
    let room_id = crate::matrix_client::create_room(client, &room_name)
        .await
        .map_err(|e| e.to_string())?;

    // Create channel in database
    let channel = session_store.create_channel(name, room_id.as_str())
        .map_err(|e| e.to_string())?;

    Ok(RoomInfo {
        room_id: channel.room_id,
        channel_name: channel.channel_name,
        workspace_path: channel.directory,
        last_activity: None,
        agent_status: AgentStatus::Idle,
    })
}

/// Tool: reset_room - Reset a room's agent session
pub fn reset_room(session_store: &SessionStore, room_id: &str) -> Result<bool, String> {
    session_store.reset_orphaned_session(room_id)
        .map_err(|e| e.to_string())?;
    Ok(true)
}
```

**Step 2: Run build**

Run: `cargo build`
Expected: Build succeeds

**Step 3: Commit**

```bash
git add src/dispatch_tools.rs
git commit -m "feat(dispatch): implement create_room and reset_room admin tools"
```

---

## Phase 4: Event Routing

### Task 4.1: Define DispatchEvent types

**Files:**
- Create: `gorp-core/src/dispatch_events.rs`
- Modify: `gorp-core/src/lib.rs`

**Step 1: Create dispatch_events.rs**

```rust
// ABOUTME: Event types for DISPATCH control plane communication.
// ABOUTME: Workers emit these events which gorp routes to DISPATCH.

use serde::{Deserialize, Serialize};

/// Events that workers can emit to DISPATCH
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DispatchEvent {
    TaskCompleted {
        room_id: String,
        task_id: Option<String>,
        summary: String,
    },
    TaskFailed {
        room_id: String,
        error: String,
    },
    WaitingForInput {
        room_id: String,
        question: String,
    },
    ProgressUpdate {
        room_id: String,
        message: String,
    },
}

impl DispatchEvent {
    /// Get the source room ID
    pub fn room_id(&self) -> &str {
        match self {
            Self::TaskCompleted { room_id, .. } => room_id,
            Self::TaskFailed { room_id, .. } => room_id,
            Self::WaitingForInput { room_id, .. } => room_id,
            Self::ProgressUpdate { room_id, .. } => room_id,
        }
    }

    /// Get the event type name
    pub fn event_type(&self) -> &'static str {
        match self {
            Self::TaskCompleted { .. } => "task_completed",
            Self::TaskFailed { .. } => "task_failed",
            Self::WaitingForInput { .. } => "waiting_for_input",
            Self::ProgressUpdate { .. } => "progress_update",
        }
    }
}
```

**Step 2: Add to lib.rs**

```rust
pub mod dispatch_events;
pub use dispatch_events::DispatchEvent;
```

**Step 3: Run tests**

Run: `cargo test -p gorp-core`
Expected: All tests pass

**Step 4: Commit**

```bash
git add gorp-core/src/dispatch_events.rs gorp-core/src/lib.rs
git commit -m "feat(dispatch): define DispatchEvent types for worker-to-DISPATCH communication"
```

---

### Task 4.2: Add event routing in message handler

**Files:**
- Modify: `src/message_handler.rs`

**Step 1: Add check for dispatch: custom events in agent event loop**

In the `AgentEvent::Custom` match arm:

```rust
AgentEvent::Custom { kind, payload } => {
    tracing::debug!(kind = %kind, "Received custom event");

    // Check for DISPATCH events
    if kind.starts_with("dispatch:") {
        if let Err(e) = route_to_dispatch(
            &session_store,
            &client,
            room.room_id().as_str(),
            &kind,
            &payload,
        ).await {
            tracing::warn!(error = %e, "Failed to route event to DISPATCH");
        }
    }
}
```

**Step 2: Add route_to_dispatch function**

```rust
/// Route an agent event to the DISPATCH control plane
async fn route_to_dispatch(
    session_store: &SessionStore,
    client: &Client,
    source_room_id: &str,
    event_kind: &str,
    payload: &serde_json::Value,
) -> Result<()> {
    // Find DISPATCH room (user's 1:1 DM with bot)
    // For now, store the event in the database
    let event = gorp_core::session::DispatchEvent {
        id: uuid::Uuid::new_v4().to_string(),
        source_room_id: source_room_id.to_string(),
        event_type: event_kind.to_string(),
        payload: payload.clone(),
        created_at: chrono::Utc::now().to_rfc3339(),
        acknowledged_at: None,
    };

    session_store.insert_dispatch_event(&event)?;
    tracing::info!(event_id = %event.id, event_type = %event_kind, "Event queued for DISPATCH");

    Ok(())
}
```

**Step 3: Run build**

Run: `cargo build`
Expected: Build succeeds

**Step 4: Commit**

```bash
git add src/message_handler.rs
git commit -m "feat(dispatch): add event routing from workers to DISPATCH"
```

---

## Phase 5: DISPATCH Agent Integration

### Task 5.1: Create DISPATCH system prompt

**Files:**
- Create: `src/dispatch_system_prompt.rs`

**Step 1: Create system prompt module**

```rust
// ABOUTME: System prompt for DISPATCH control plane agent.
// ABOUTME: Provides cross-room awareness and orchestration capabilities.

use crate::session::SessionStore;

/// Generate the DISPATCH system prompt with current room state
pub fn generate_dispatch_prompt(session_store: &SessionStore) -> String {
    let rooms = session_store.list_all()
        .unwrap_or_default()
        .into_iter()
        .filter(|c| !c.is_dispatch_room)
        .map(|c| format!("- {} ({}): {}", c.channel_name, c.room_id, c.directory))
        .collect::<Vec<_>>()
        .join("\n");

    format!(r#"You are DISPATCH, the control plane for this workspace grid.

Your role:
- Monitor all active workspace rooms
- Notify the user of important events (completions, errors, questions)
- Dispatch tasks to appropriate rooms on user request
- Summarize activity across rooms
- Help user decide where to focus attention

You do NOT:
- Execute code or modify files directly
- Make decisions without user input on important matters
- Spam the user with trivial updates

Available rooms:
{rooms}

Tools available:
- list_rooms: Get status of all workspace rooms
- get_room_status: Get detailed info about a specific room
- dispatch_task: Send a prompt to a worker room
- check_task: Check status of a dispatched task
- create_room: Create a new workspace room
- reset_room: Reset a room's agent session
- read_room_history: Read message history from any room

When dispatching work, match the task to the right room based on:
- Workspace path and purpose
- Current room status
- Task requirements
"#)
}
```

**Step 2: Run build**

Run: `cargo build`
Expected: Build succeeds

**Step 3: Commit**

```bash
git add src/dispatch_system_prompt.rs src/lib.rs
git commit -m "feat(dispatch): create DISPATCH system prompt generator"
```

---

### Task 5.2: Implement DISPATCH agent invocation

**Files:**
- Modify: `src/dispatch_handler.rs`

**Step 1: Implement full DISPATCH message handling**

```rust
use anyhow::Result;
use matrix_sdk::{
    room::Room,
    ruma::events::room::message::RoomMessageEventContent,
    Client,
};
use gorp_agent::AgentEvent;

use crate::{
    config::Config,
    dispatch_system_prompt::generate_dispatch_prompt,
    session::SessionStore,
    utils::{chunk_message, markdown_to_html, MAX_CHUNK_SIZE},
    warm_session::{prepare_dispatch_session, SharedWarmSessionManager},
};

pub async fn handle_dispatch_message(
    room: Room,
    event: matrix_sdk::ruma::events::room::message::OriginalSyncRoomMessageEvent,
    client: Client,
    config: Config,
    session_store: SessionStore,
    warm_manager: SharedWarmSessionManager,
) -> Result<()> {
    let body = event.content.body();

    // Generate dynamic system prompt
    let system_prompt = generate_dispatch_prompt(&session_store);

    // Get or create DISPATCH session
    let dispatch_channel = session_store.get_or_create_dispatch_channel(room.room_id().as_str())?;

    // Prepare mux session with DISPATCH tools
    let (session_handle, session_id, _is_new) =
        prepare_dispatch_session(&warm_manager, &dispatch_channel, &system_prompt).await?;

    // Send prompt and process events
    room.typing_notice(true).await?;

    let mut event_rx = crate::warm_session::send_prompt_with_handle(&session_handle, &session_id, body).await?;

    let mut final_response = String::new();

    while let Some(event) = event_rx.recv().await {
        match event {
            AgentEvent::Text(text) => {
                final_response.push_str(&text);
            }
            AgentEvent::Result { text, .. } => {
                if final_response.is_empty() {
                    final_response = text;
                }
                break;
            }
            AgentEvent::Error { message, .. } => {
                room.typing_notice(false).await?;
                room.send(RoomMessageEventContent::text_plain(format!("⚠️ Error: {}", message))).await?;
                return Ok(());
            }
            _ => {}
        }
    }

    room.typing_notice(false).await?;

    // Send response
    let chunks = chunk_message(&final_response, MAX_CHUNK_SIZE);
    for chunk in chunks {
        let html = markdown_to_html(&chunk);
        room.send(RoomMessageEventContent::text_html(&chunk, &html)).await?;
    }

    Ok(())
}
```

**Step 2: Run build**

Run: `cargo build`
Expected: Build succeeds

**Step 3: Commit**

```bash
git add src/dispatch_handler.rs
git commit -m "feat(dispatch): implement DISPATCH agent invocation"
```

---

## Phase 6: Integration & Testing

### Task 6.1: Wire up DISPATCH in main message handler

**Files:**
- Modify: `src/message_handler.rs`

**Step 1: Add DISPATCH routing at top of handle_message**

After the is_dm check, add:

```rust
// Check if this is the DISPATCH control plane room
if is_dm {
    if let Some(dispatch_channel) = session_store.get_dispatch_channel(room.room_id().as_str())? {
        return crate::dispatch_handler::handle_dispatch_message(
            room, event, client, config, session_store, warm_manager
        ).await;
    }

    // Check for DISPATCH activation command
    let body_lower = body.to_lowercase();
    if body_lower.starts_with("!dispatch") || body_lower == "dispatch" {
        // Create DISPATCH channel and route to handler
        session_store.create_dispatch_channel(room.room_id().as_str())?;
        return crate::dispatch_handler::handle_dispatch_message(
            room, event, client, config, session_store, warm_manager
        ).await;
    }
}
```

**Step 2: Run build and tests**

Run: `cargo build && cargo test`
Expected: All pass

**Step 3: Commit**

```bash
git add src/message_handler.rs
git commit -m "feat(dispatch): wire up DISPATCH routing in main handler"
```

---

### Task 6.2: Add integration tests

**Files:**
- Create: `tests/dispatch_integration.rs`

**Step 1: Create basic integration tests**

```rust
// ABOUTME: Integration tests for DISPATCH control plane functionality.
// ABOUTME: Tests room detection, event routing, and task dispatch.

use gorp_core::session::SessionStore;
use tempfile::TempDir;

#[test]
fn test_dispatch_channel_creation() {
    let tmp = TempDir::new().unwrap();
    let store = SessionStore::new(tmp.path()).unwrap();

    // Create DISPATCH channel
    let channel = store.create_dispatch_channel("!test:matrix.org").unwrap();

    assert!(channel.is_dispatch_room);
    assert_eq!(channel.channel_name, "dispatch");
    assert!(channel.directory.is_empty());
}

#[test]
fn test_dispatch_event_crud() {
    let tmp = TempDir::new().unwrap();
    let store = SessionStore::new(tmp.path()).unwrap();

    // Insert event
    let event = gorp_core::session::DispatchEvent {
        id: "test-1".to_string(),
        source_room_id: "!room:matrix.org".to_string(),
        event_type: "task_completed".to_string(),
        payload: serde_json::json!({"summary": "Done!"}),
        created_at: chrono::Utc::now().to_rfc3339(),
        acknowledged_at: None,
    };
    store.insert_dispatch_event(&event).unwrap();

    // Get pending
    let pending = store.get_pending_dispatch_events().unwrap();
    assert_eq!(pending.len(), 1);

    // Acknowledge
    store.acknowledge_dispatch_event("test-1").unwrap();
    let pending = store.get_pending_dispatch_events().unwrap();
    assert_eq!(pending.len(), 0);
}
```

**Step 2: Run tests**

Run: `cargo test dispatch`
Expected: All pass

**Step 3: Commit**

```bash
git add tests/dispatch_integration.rs
git commit -m "test(dispatch): add integration tests for DISPATCH functionality"
```

---

### Task 6.3: Update documentation

**Files:**
- Modify: `docs/HELP.md`

**Step 1: Add DISPATCH section to help docs**

```markdown
## DISPATCH Control Plane

DISPATCH is your orchestration assistant that runs in your 1:1 DM with the bot.
It can monitor all your workspace rooms and help coordinate work across them.

### Activating DISPATCH

In your DM with the bot, type:
```
!dispatch
```
or just
```
dispatch
```

### DISPATCH Capabilities

- **Room monitoring**: See status of all workspace rooms
- **Task dispatch**: Send tasks to specific rooms
- **Event notifications**: Get notified of completions, errors, questions
- **Admin commands**: Create rooms, reset sessions, manage schedules

### Example Interactions

"What's happening across my projects?"
"Send 'run the tests' to the gorp-rs room"
"Create a new room called research"
"What questions are waiting for me?"
```

**Step 2: Commit**

```bash
git add docs/HELP.md
git commit -m "docs(dispatch): add DISPATCH documentation to help"
```

---

## Summary

This implementation plan covers:

1. **Database schema** (Tasks 1.1-1.3): Add columns and tables for DISPATCH functionality
2. **Room detection** (Tasks 2.1-2.2): Identify and create DISPATCH rooms in DMs
3. **MCP tools** (Tasks 3.1-3.3): Cross-room query and task dispatch tools
4. **Event routing** (Tasks 4.1-4.2): Route worker events to DISPATCH
5. **Agent integration** (Tasks 5.1-5.2): System prompt and agent invocation
6. **Testing & docs** (Tasks 6.1-6.3): Wire up, test, and document

Each task is atomic with clear deliverables and can be implemented in sequence.
