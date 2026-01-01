# DISPATCH: Control Plane Agent Design

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** A control plane agent in the 1:1 DM that orchestrates and monitors all workspace rooms.

**Architecture:** DISPATCH is a mux-powered agent with cross-room visibility. It initiates work in rooms but never executes it directly. Workers emit events that gorp routes to DISPATCH for user notification.

**Tech Stack:** Rust, mux backend, Matrix, SQLite for cross-room state

---

## Conceptual Model

```
┌─────────────────────────────────────────────────┐
│  1:1 DM with Bot                                │
│  DISPATCH - Control Plane Agent                 │
│  ┌─────────────────────────────────────────┐    │
│  │ • Sees all rooms and their status       │    │
│  │ • Dispatches tasks to workers           │    │
│  │ • Receives completion events            │    │
│  │ • Summarizes/prioritizes for user       │    │
│  │ • Does NOT execute work directly        │    │
│  └─────────────────────────────────────────┘    │
└─────────────────────────────────────────────────┘
         │           │           │
         ▼           ▼           ▼
    ┌─────────┐ ┌─────────┐ ┌─────────┐
    │#project │ │#research│ │ #ops    │
    │~/code   │ │~/papers │ │~/infra  │
    │(worker) │ │(worker) │ │(worker) │
    └─────────┘ └─────────┘ └─────────┘
```

## Core Principles

1. **DISPATCH orchestrates, workers execute** - DISPATCH never runs code or modifies files in worker workspaces
2. **Event-driven awareness** - Workers emit events, gorp routes them to DISPATCH
3. **Cross-room read access** - DISPATCH can query any room's message history
4. **Pure mux backend** - Same agent architecture as workers, just with special tools
5. **User's single pane of glass** - Interact with everything through one conversation

## DISPATCH Toolset

### Query Tools

```rust
/// List all active workspace rooms
fn list_rooms() -> Vec<RoomInfo>;

struct RoomInfo {
    room_id: String,
    channel_name: String,
    workspace_path: String,
    last_activity: DateTime,
    agent_status: AgentStatus,  // idle, working, waiting_input, error
}

/// Get detailed status of a specific room
fn get_room_status(room_id: String) -> RoomStatus;

struct RoomStatus {
    info: RoomInfo,
    current_task: Option<String>,
    session_id: Option<String>,
    recent_messages: u32,
    pending_events: Vec<Event>,
}

/// Read message history from any room
fn read_room_history(room_id: String, limit: u32) -> Vec<Message>;
```

### Action Tools

```rust
/// Dispatch a task to a worker room
fn dispatch_task(room_id: String, prompt: String) -> TaskId;

/// Check status of a dispatched task
fn check_task(task_id: String) -> TaskStatus;

enum TaskStatus {
    Pending,
    InProgress { progress: Option<String> },
    Completed { summary: String },
    Failed { error: String },
}

/// Interrupt/cancel work in a room
fn interrupt_room(room_id: String) -> bool;
```

## Event Routing

### Worker → DISPATCH Events

Workers emit `AgentEvent::Custom` events that gorp intercepts and routes:

```rust
// Events workers can emit
enum DispatchEvent {
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
```

### Routing Logic in gorp

```rust
// In message handler
if let AgentEvent::Custom { kind, payload } = event {
    if kind.starts_with("dispatch:") {
        route_to_dispatch(payload).await;
    }
}

async fn route_to_dispatch(event: DispatchEvent) {
    // Find DISPATCH room (1:1 DM)
    // Format event as message
    // Inject into DISPATCH's context
}
```

## DISPATCH System Prompt

```
You are DISPATCH, the control plane for this workspace grid.

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

Available rooms: {dynamic list}
Recent events: {event queue}

When dispatching work, match the task to the right room based on:
- Workspace path and purpose
- Current room status
- Task requirements
```

## Database Schema Additions

```sql
-- Track DISPATCH events
CREATE TABLE dispatch_events (
    id TEXT PRIMARY KEY,
    source_room_id TEXT NOT NULL,
    event_type TEXT NOT NULL,
    payload TEXT NOT NULL,  -- JSON
    created_at TEXT NOT NULL,
    acknowledged_at TEXT,   -- When DISPATCH processed it
    FOREIGN KEY (source_room_id) REFERENCES channels(room_id)
);

-- Track dispatched tasks
CREATE TABLE dispatch_tasks (
    id TEXT PRIMARY KEY,
    target_room_id TEXT NOT NULL,
    prompt TEXT NOT NULL,
    status TEXT NOT NULL,  -- pending, in_progress, completed, failed
    created_at TEXT NOT NULL,
    completed_at TEXT,
    result_summary TEXT,
    FOREIGN KEY (target_room_id) REFERENCES channels(room_id)
);
```

## Implementation Tasks

### Phase 1: Foundation
1. Add DISPATCH room detection (is_dispatch_room flag in channels table)
2. Implement cross-room message history query
3. Add dispatch_events and dispatch_tasks tables
4. Create DISPATCH-specific mux tools (list_rooms, get_room_status)

### Phase 2: Event Routing
5. Define DispatchEvent enum and serialization
6. Add event emission helper for workers
7. Implement gorp event routing to DISPATCH room
8. Add event queue processing in DISPATCH context

### Phase 3: Task Dispatch
9. Implement dispatch_task tool
10. Add task tracking and status updates
11. Connect worker completion to task status
12. Implement interrupt_room tool

### Phase 4: Polish
13. DISPATCH system prompt with dynamic room list
14. Batch notification logic (don't spam)
15. Integration tests
16. Documentation

## Open Questions

1. **DISPATCH workspace path** - What directory does DISPATCH operate in?
   - Option A: Special sentinel like `~/.dispatch` or `/dev/null`
   - Option B: User's home directory (read-only awareness)
   - Option C: No workspace, pure coordination

2. **Event batching** - How long should DISPATCH wait before notifying?
   - Immediate for errors/questions
   - Batched for progress/completions?

3. **Multi-user** - Does each user get their own DISPATCH, or is there one per bot?
   - Probably per-user (1:1 DM implies this)

4. **Room creation** - Should DISPATCH be able to create new rooms?
   - Currently !create is in the DM, so maybe yes

---

## Example Interaction

```
User: What's happening across my projects?

DISPATCH: Here's the current status:

📁 #gorp-rs (~/code/gorp-rs) - Idle
   Last activity: 2 hours ago

📁 #research (~/research/papers) - Working
   Current: Summarizing arxiv papers on RAG
   Progress: 3/7 papers processed

📁 #infra (~/ops/servers) - Waiting
   Question: "Should I proceed with the nginx restart?"

Would you like me to check on #research progress or respond to #infra?

User: Tell infra yes, proceed. And when research is done, summarize findings.

DISPATCH: Got it.
✓ Dispatched approval to #infra
✓ Queued summary request for #research (will notify on completion)
```
