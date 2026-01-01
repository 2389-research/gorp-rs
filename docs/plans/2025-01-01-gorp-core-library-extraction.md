# Gorp Core Library Extraction

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Extract platform-agnostic orchestration logic from gorp into a `gorp-core` library, enabling pluggable chat interfaces (Matrix, Slack, Discord, CLI, etc.)

**Architecture:** Define `ChatInterface` and `ChatRoom` traits that abstract away platform-specific messaging. Move session management, scheduling, warm sessions, and command processing into `gorp-core`. Keep `gorp` as the Matrix-specific binary that implements these traits.

**Tech Stack:** Rust, async-trait, tokio, rusqlite (existing deps)

---

## Current State Analysis

### Files by Category

**Platform-Agnostic (move to gorp-core):**
| File | Lines | Notes |
|------|-------|-------|
| `session.rs` | 647 | Channel/session SQLite persistence |
| `warm_session.rs` | 828 | AgentHandle lifecycle, no Matrix types |
| `scheduler.rs` | 1350 | Scheduled prompts, uses room_id as string |
| `config.rs` | 410 | Configuration parsing |
| `paths.rs` | 43 | Path utilities |
| `metrics.rs` | 216 | Prometheus metrics |
| `utils.rs` | 254 | Markdown, chunking utilities |

**Matrix-Specific (stay in gorp):**
| File | Lines | Notes |
|------|-------|-------|
| `matrix_client.rs` | 194 | Matrix SDK wrappers |
| `message_handler.rs` | 2466 | Heavy Matrix SDK usage |
| `onboarding.rs` | 300 | Uses Matrix Room types |
| `mcp.rs` | 1396 | Uses Matrix Client |
| `main.rs` | 1527 | Matrix sync loop |
| `webhook.rs` | 713 | Webhook server |
| `admin/` | ~500 | Web admin UI |

### Matrix SDK Operations Used

From `message_handler.rs`, these Matrix operations need abstraction:
- `room.send(RoomMessageEventContent::text_plain(...))` - send plain text
- `room.send(RoomMessageEventContent::text_html(...))` - send formatted message
- `room.typing_notice(bool)` - typing indicator
- `room.state()` - check if joined
- `room.is_direct()` - check if DM
- `room.room_id()` - get room identifier
- `client.media().get_media_content(...)` - download attachments

---

## Target Architecture

```
gorp-core/                    # NEW: Platform-agnostic library
├── Cargo.toml
├── src/
│   ├── lib.rs
│   ├── traits.rs             # ChatInterface, ChatRoom, ChatUser traits
│   ├── session.rs            # FROM: gorp/src/session.rs
│   ├── warm_session.rs       # FROM: gorp/src/warm_session.rs
│   ├── scheduler.rs          # FROM: gorp/src/scheduler.rs (modified)
│   ├── config.rs             # FROM: gorp/src/config.rs
│   ├── paths.rs              # FROM: gorp/src/paths.rs
│   ├── metrics.rs            # FROM: gorp/src/metrics.rs
│   ├── utils.rs              # FROM: gorp/src/utils.rs
│   ├── commands.rs           # NEW: Generic command parsing
│   └── orchestrator.rs       # NEW: Main message processing loop

gorp/                         # Matrix-specific binary
├── Cargo.toml                # MODIFY: depend on gorp-core
├── src/
│   ├── main.rs               # Matrix sync loop
│   ├── lib.rs
│   ├── matrix_interface.rs   # NEW: impl ChatInterface for Matrix
│   ├── matrix_client.rs      # Matrix SDK helpers
│   ├── message_handler.rs    # MODIFY: use orchestrator
│   ├── onboarding.rs         # Matrix-specific onboarding
│   ├── mcp.rs
│   ├── webhook.rs
│   └── admin/

gorp-agent/                   # Unchanged - backend abstraction
```

---

## Phase 1: Define Core Traits (gorp-core skeleton)

### Task 1.1: Create gorp-core crate

**Files:**
- Create: `gorp-core/Cargo.toml`
- Create: `gorp-core/src/lib.rs`

**Step 1: Create Cargo.toml**

```toml
[package]
name = "gorp-core"
version = "0.1.0"
edition = "2021"
description = "Platform-agnostic chat orchestration for AI agents"
license = "MIT"

[dependencies]
tokio = { version = "1", features = ["sync", "rt", "time", "fs"] }
async-trait = "0.1"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tracing = "0.1"
anyhow = "1"
rusqlite = { version = "0.37", features = ["bundled"] }
chrono = "0.4"
uuid = { version = "1", features = ["v4"] }
cron = "0.15"
regex = "1"
chrono-tz = "0.10"
metrics = "0.24"
pulldown-cmark = "0.13"

# Internal
gorp-agent = { path = "../gorp-agent", features = ["acp"] }

[dev-dependencies]
tokio = { version = "1", features = ["full", "test-util"] }
tempfile = "3"
```

**Step 2: Create lib.rs skeleton**

```rust
// ABOUTME: Platform-agnostic chat orchestration for AI agents
// ABOUTME: Provides traits and core logic for any chat interface

pub mod traits;
pub mod config;
pub mod paths;
pub mod utils;
pub mod metrics;
pub mod session;
pub mod warm_session;
pub mod scheduler;
pub mod commands;
pub mod orchestrator;

pub use traits::{ChatInterface, ChatRoom, ChatUser, MessageContent};
pub use config::Config;
pub use session::{Channel, SessionStore};
pub use warm_session::{WarmConfig, WarmSession, WarmSessionManager};
pub use scheduler::{ScheduledPrompt, SchedulerStore};
pub use orchestrator::Orchestrator;

// Re-export gorp-agent types
pub use gorp_agent::{AgentEvent, AgentHandle, AgentRegistry};
```

**Step 3: Verify crate compiles (will fail - that's expected)**

Run: `cd gorp-core && cargo check 2>&1 | head -20`
Expected: Errors about missing modules

**Step 4: Commit skeleton**

```bash
git add gorp-core/
git commit -m "feat(gorp-core): create crate skeleton"
```

---

### Task 1.2: Define ChatRoom trait

**Files:**
- Create: `gorp-core/src/traits.rs`

**Step 1: Write trait definitions**

```rust
// ABOUTME: Core traits for platform-agnostic chat interfaces
// ABOUTME: ChatRoom, ChatInterface, ChatUser abstractions

use anyhow::Result;
use async_trait::async_trait;
use std::fmt::Debug;

/// Content that can be sent to a chat room
#[derive(Debug, Clone)]
pub enum MessageContent {
    /// Plain text message
    Plain(String),
    /// Message with both plain text and HTML formatting
    Html { plain: String, html: String },
    /// Message with an attachment
    Attachment {
        filename: String,
        data: Vec<u8>,
        mime_type: String,
        caption: Option<String>,
    },
}

impl MessageContent {
    pub fn plain(text: impl Into<String>) -> Self {
        Self::Plain(text.into())
    }

    pub fn html(plain: impl Into<String>, html: impl Into<String>) -> Self {
        Self::Html {
            plain: plain.into(),
            html: html.into(),
        }
    }
}

/// Abstraction over a chat room/channel
#[async_trait]
pub trait ChatRoom: Send + Sync + Debug + Clone {
    /// Unique identifier for this room (e.g., Matrix room ID, Slack channel ID)
    fn id(&self) -> &str;

    /// Human-readable name of the room, if available
    fn name(&self) -> Option<String>;

    /// Whether this is a direct message (1:1) conversation
    async fn is_direct_message(&self) -> bool;

    /// Send a message to this room
    async fn send(&self, content: MessageContent) -> Result<()>;

    /// Set typing indicator on/off
    async fn set_typing(&self, typing: bool) -> Result<()>;

    /// Download an attachment by its source identifier
    /// Returns (filename, data, mime_type)
    async fn download_attachment(&self, source_id: &str) -> Result<(String, Vec<u8>, String)>;
}

/// Identity of a chat user
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ChatUser {
    /// Unique identifier (e.g., @user:matrix.org, U12345678)
    pub id: String,
    /// Display name
    pub display_name: Option<String>,
}

impl ChatUser {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            display_name: None,
        }
    }

    pub fn with_name(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            display_name: Some(name.into()),
        }
    }
}

/// Incoming message from a chat platform
#[derive(Debug, Clone)]
pub struct IncomingMessage {
    /// The room this message was sent in
    pub room_id: String,
    /// The user who sent the message
    pub sender: ChatUser,
    /// Message body (text content)
    pub body: String,
    /// Whether this message is formatted (HTML, markdown, etc.)
    pub formatted: bool,
    /// Attachment info if present: (source_id, filename, mime_type)
    pub attachment: Option<(String, String, String)>,
    /// Platform-specific event ID
    pub event_id: String,
    /// Timestamp in seconds since Unix epoch
    pub timestamp: i64,
}

/// Interface for a chat platform (Matrix, Slack, Discord, etc.)
#[async_trait]
pub trait ChatInterface: Send + Sync {
    /// The room type for this platform
    type Room: ChatRoom;

    /// Get a room by its ID
    async fn get_room(&self, room_id: &str) -> Option<Self::Room>;

    /// Get the bot's own user ID
    fn bot_user_id(&self) -> &str;

    /// Check if a user ID is the bot itself
    fn is_self(&self, user_id: &str) -> bool {
        user_id == self.bot_user_id()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_content_plain() {
        let content = MessageContent::plain("Hello");
        assert!(matches!(content, MessageContent::Plain(s) if s == "Hello"));
    }

    #[test]
    fn test_message_content_html() {
        let content = MessageContent::html("Hello", "<b>Hello</b>");
        assert!(matches!(content, MessageContent::Html { plain, html }
            if plain == "Hello" && html == "<b>Hello</b>"));
    }

    #[test]
    fn test_chat_user_new() {
        let user = ChatUser::new("@test:example.com");
        assert_eq!(user.id, "@test:example.com");
        assert!(user.display_name.is_none());
    }

    #[test]
    fn test_chat_user_with_name() {
        let user = ChatUser::with_name("@test:example.com", "Test User");
        assert_eq!(user.id, "@test:example.com");
        assert_eq!(user.display_name, Some("Test User".to_string()));
    }
}
```

**Step 2: Verify traits compile**

Run: `cd gorp-core && cargo check`
Expected: May have errors about missing modules, but traits.rs should be valid

**Step 3: Commit**

```bash
git add gorp-core/src/traits.rs
git commit -m "feat(gorp-core): define ChatRoom, ChatInterface, ChatUser traits"
```

---

## Phase 2: Move Platform-Agnostic Modules

### Task 2.1: Move paths.rs

**Files:**
- Copy: `src/paths.rs` → `gorp-core/src/paths.rs`
- Modify: `gorp-core/src/lib.rs`

**Step 1: Copy file**

```bash
cp src/paths.rs gorp-core/src/paths.rs
```

**Step 2: Verify it compiles standalone**

Run: `cd gorp-core && cargo check`
Expected: PASS (paths.rs has no external deps)

**Step 3: Commit**

```bash
git add gorp-core/src/paths.rs
git commit -m "feat(gorp-core): move paths module"
```

---

### Task 2.2: Move utils.rs

**Files:**
- Copy: `src/utils.rs` → `gorp-core/src/utils.rs`
- Modify: `gorp-core/src/lib.rs`

**Step 1: Copy file**

```bash
cp src/utils.rs gorp-core/src/utils.rs
```

**Step 2: Verify compilation**

Run: `cd gorp-core && cargo check`
Expected: PASS (utils uses pulldown-cmark which is in deps)

**Step 3: Commit**

```bash
git add gorp-core/src/utils.rs
git commit -m "feat(gorp-core): move utils module"
```

---

### Task 2.3: Move metrics.rs

**Files:**
- Copy: `src/metrics.rs` → `gorp-core/src/metrics.rs`

**Step 1: Copy file**

```bash
cp src/metrics.rs gorp-core/src/metrics.rs
```

**Step 2: Verify compilation**

Run: `cd gorp-core && cargo check`
Expected: PASS

**Step 3: Commit**

```bash
git add gorp-core/src/metrics.rs
git commit -m "feat(gorp-core): move metrics module"
```

---

### Task 2.4: Move config.rs

**Files:**
- Copy: `src/config.rs` → `gorp-core/src/config.rs`
- Modify: Update imports

**Step 1: Copy and modify**

```bash
cp src/config.rs gorp-core/src/config.rs
```

**Step 2: Update imports in gorp-core/src/config.rs**

Replace `use crate::paths;` with `use crate::paths;` (same, but verify)

**Step 3: Verify compilation**

Run: `cd gorp-core && cargo check`
Expected: PASS or minor fixes needed

**Step 4: Commit**

```bash
git add gorp-core/src/config.rs
git commit -m "feat(gorp-core): move config module"
```

---

### Task 2.5: Move session.rs

**Files:**
- Copy: `src/session.rs` → `gorp-core/src/session.rs`

**Step 1: Copy file**

```bash
cp src/session.rs gorp-core/src/session.rs
```

**Step 2: Review for Matrix dependencies**

The file uses `room_id: String` which is platform-agnostic (just a string identifier).
No Matrix SDK imports should be present.

**Step 3: Verify compilation**

Run: `cd gorp-core && cargo check`
Expected: PASS

**Step 4: Commit**

```bash
git add gorp-core/src/session.rs
git commit -m "feat(gorp-core): move session module"
```

---

### Task 2.6: Move warm_session.rs

**Files:**
- Copy: `src/warm_session.rs` → `gorp-core/src/warm_session.rs`
- Modify: Update config imports

**Step 1: Copy file**

```bash
cp src/warm_session.rs gorp-core/src/warm_session.rs
```

**Step 2: Update imports**

Change `use crate::config::McpServerConfig;` to match new location.
Change `use crate::session::Channel;` to `use crate::session::Channel;`

**Step 3: Verify compilation**

Run: `cd gorp-core && cargo check`
Expected: PASS

**Step 4: Commit**

```bash
git add gorp-core/src/warm_session.rs
git commit -m "feat(gorp-core): move warm_session module"
```

---

### Task 2.7: Move scheduler.rs (with modifications)

**Files:**
- Copy: `src/scheduler.rs` → `gorp-core/src/scheduler.rs`
- Modify: Remove Matrix-specific execution, add trait-based callback

**Step 1: Copy file**

```bash
cp src/scheduler.rs gorp-core/src/scheduler.rs
```

**Step 2: Modify to use callback trait instead of Matrix**

The `start_scheduler` function currently:
1. Gets scheduled prompts
2. Executes them via warm_session
3. Sends results to Matrix room

Modify to:
- Keep scheduling logic
- Return a `SchedulerEvent` that the platform handles
- Platform implements how to send results

Add this trait to scheduler.rs:

```rust
/// Callback for when scheduler needs to send results
#[async_trait]
pub trait SchedulerCallback: Send + Sync {
    async fn send_result(&self, room_id: &str, result: &str) -> Result<()>;
    async fn send_error(&self, room_id: &str, error: &str) -> Result<()>;
}
```

**Step 3: Verify compilation**

Run: `cd gorp-core && cargo check`
Expected: PASS after modifications

**Step 4: Commit**

```bash
git add gorp-core/src/scheduler.rs
git commit -m "feat(gorp-core): move scheduler with callback trait"
```

---

## Phase 3: Create Commands Module

### Task 3.1: Extract generic command parsing

**Files:**
- Create: `gorp-core/src/commands.rs`

**Step 1: Create commands module**

Extract command parsing logic from message_handler.rs:

```rust
// ABOUTME: Generic command parsing for chat bot commands
// ABOUTME: Platform-agnostic !command handling

use anyhow::Result;

/// Parsed command from user input
#[derive(Debug, Clone)]
pub struct Command {
    /// The command name (e.g., "help", "status", "schedule")
    pub name: String,
    /// Command arguments
    pub args: Vec<String>,
    /// Raw argument string (everything after command name)
    pub raw_args: String,
}

/// Result of parsing a message
#[derive(Debug)]
pub enum ParseResult {
    /// This is a command (starts with ! or !claude)
    Command(Command),
    /// This is a regular message to send to the agent
    Message(String),
    /// Message should be ignored (empty, bot's own message, etc.)
    Ignore,
}

/// Parse a message body into a command or regular message
pub fn parse_message(body: &str, bot_prefix: &str) -> ParseResult {
    let body = body.trim();

    if body.is_empty() {
        return ParseResult::Ignore;
    }

    // Check for command prefix
    let is_command = body.starts_with(&format!("{} ", bot_prefix))
        || (body.starts_with("!")
            && !body.starts_with("!!")
            && body.len() > 1
            && body.chars().nth(1).map(|c| c.is_alphabetic()).unwrap_or(false));

    if !is_command {
        return ParseResult::Message(body.to_string());
    }

    // Parse command
    let command_parts: Vec<&str> = if body.starts_with(&format!("{} ", bot_prefix)) {
        body[bot_prefix.len()..].trim().split_whitespace().collect()
    } else if body.starts_with("!") {
        body[1..].split_whitespace().collect()
    } else {
        return ParseResult::Message(body.to_string());
    };

    if command_parts.is_empty() {
        return ParseResult::Message(body.to_string());
    }

    let name = command_parts[0].to_lowercase();
    let args: Vec<String> = command_parts[1..].iter().map(|s| s.to_string()).collect();
    let raw_args = if command_parts.len() > 1 {
        command_parts[1..].join(" ")
    } else {
        String::new()
    };

    ParseResult::Command(Command { name, args, raw_args })
}

/// Standard commands that all platforms should support
#[derive(Debug, Clone, PartialEq)]
pub enum StandardCommand {
    Help,
    Status,
    Reset,
    Schedule(ScheduleSubcommand),
    Backend(BackendSubcommand),
    Debug(DebugSubcommand),
    Unknown(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum ScheduleSubcommand {
    List,
    Add { time: String, prompt: String },
    Remove { id: String },
    Pause { id: String },
    Resume { id: String },
}

#[derive(Debug, Clone, PartialEq)]
pub enum BackendSubcommand {
    Get,
    List,
    Set { backend: String },
    Reset,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DebugSubcommand {
    Enable,
    Disable,
    Status,
}

impl Command {
    /// Try to parse as a standard command
    pub fn as_standard(&self) -> StandardCommand {
        match self.name.as_str() {
            "help" | "h" => StandardCommand::Help,
            "status" => StandardCommand::Status,
            "reset" => StandardCommand::Reset,
            "schedule" | "sched" => self.parse_schedule(),
            "backend" => self.parse_backend(),
            "debug" => self.parse_debug(),
            other => StandardCommand::Unknown(other.to_string()),
        }
    }

    fn parse_schedule(&self) -> StandardCommand {
        let sub = self.args.first().map(|s| s.as_str()).unwrap_or("list");
        match sub {
            "list" | "ls" => StandardCommand::Schedule(ScheduleSubcommand::List),
            "add" | "create" => {
                // Parse "at <time> <prompt>"
                if self.args.len() >= 3 && self.args[1] == "at" {
                    let time = self.args[2].clone();
                    let prompt = self.args[3..].join(" ");
                    StandardCommand::Schedule(ScheduleSubcommand::Add { time, prompt })
                } else {
                    StandardCommand::Unknown("schedule".to_string())
                }
            }
            "remove" | "rm" | "delete" => {
                let id = self.args.get(1).cloned().unwrap_or_default();
                StandardCommand::Schedule(ScheduleSubcommand::Remove { id })
            }
            "pause" => {
                let id = self.args.get(1).cloned().unwrap_or_default();
                StandardCommand::Schedule(ScheduleSubcommand::Pause { id })
            }
            "resume" => {
                let id = self.args.get(1).cloned().unwrap_or_default();
                StandardCommand::Schedule(ScheduleSubcommand::Resume { id })
            }
            _ => StandardCommand::Schedule(ScheduleSubcommand::List),
        }
    }

    fn parse_backend(&self) -> StandardCommand {
        let sub = self.args.first().map(|s| s.as_str());
        match sub {
            None | Some("get") | Some("status") => {
                StandardCommand::Backend(BackendSubcommand::Get)
            }
            Some("list") | Some("ls") => StandardCommand::Backend(BackendSubcommand::List),
            Some("set") => {
                let backend = self.args.get(1).cloned().unwrap_or_default();
                StandardCommand::Backend(BackendSubcommand::Set { backend })
            }
            Some("reset") | Some("default") => {
                StandardCommand::Backend(BackendSubcommand::Reset)
            }
            _ => StandardCommand::Backend(BackendSubcommand::Get),
        }
    }

    fn parse_debug(&self) -> StandardCommand {
        let sub = self.args.first().map(|s| s.as_str()).unwrap_or("status");
        match sub {
            "on" | "enable" => StandardCommand::Debug(DebugSubcommand::Enable),
            "off" | "disable" => StandardCommand::Debug(DebugSubcommand::Disable),
            _ => StandardCommand::Debug(DebugSubcommand::Status),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_help_command() {
        let result = parse_message("!help", "!claude");
        assert!(matches!(result, ParseResult::Command(cmd) if cmd.name == "help"));
    }

    #[test]
    fn test_parse_claude_prefix() {
        let result = parse_message("!claude help", "!claude");
        assert!(matches!(result, ParseResult::Command(cmd) if cmd.name == "help"));
    }

    #[test]
    fn test_parse_regular_message() {
        let result = parse_message("Hello world", "!claude");
        assert!(matches!(result, ParseResult::Message(s) if s == "Hello world"));
    }

    #[test]
    fn test_parse_schedule_add() {
        let result = parse_message("!schedule add at 9am check emails", "!claude");
        if let ParseResult::Command(cmd) = result {
            assert_eq!(cmd.name, "schedule");
            let std = cmd.as_standard();
            assert!(matches!(std, StandardCommand::Schedule(ScheduleSubcommand::Add { .. })));
        } else {
            panic!("Expected command");
        }
    }

    #[test]
    fn test_backend_set() {
        let result = parse_message("!backend set mux", "!claude");
        if let ParseResult::Command(cmd) = result {
            let std = cmd.as_standard();
            assert!(matches!(std, StandardCommand::Backend(BackendSubcommand::Set { backend }) if backend == "mux"));
        } else {
            panic!("Expected command");
        }
    }
}
```

**Step 2: Run tests**

Run: `cd gorp-core && cargo test commands`
Expected: PASS

**Step 3: Commit**

```bash
git add gorp-core/src/commands.rs
git commit -m "feat(gorp-core): add generic command parsing module"
```

---

## Phase 4: Create Orchestrator

### Task 4.1: Create message orchestrator

**Files:**
- Create: `gorp-core/src/orchestrator.rs`

**Step 1: Create orchestrator with trait-based design**

```rust
// ABOUTME: Core message orchestration loop for AI agent interactions
// ABOUTME: Platform-agnostic message handling using ChatInterface trait

use crate::{
    commands::{parse_message, Command, ParseResult, StandardCommand},
    config::Config,
    metrics,
    session::{Channel, SessionStore},
    traits::{ChatInterface, ChatRoom, ChatUser, IncomingMessage, MessageContent},
    utils::{chunk_message, markdown_to_html, strip_function_calls, MAX_CHUNK_SIZE},
    warm_session::{prepare_session_async, SharedWarmSessionManager, WarmConfig},
};
use anyhow::Result;
use gorp_agent::AgentEvent;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Configuration for the orchestrator
#[derive(Debug, Clone)]
pub struct OrchestratorConfig {
    /// Bot command prefix (e.g., "!claude")
    pub bot_prefix: String,
    /// Allowed user IDs (empty = allow all)
    pub allowed_users: Vec<String>,
    /// Management room ID for admin commands
    pub management_room: Option<String>,
}

/// Result of handling a message
#[derive(Debug)]
pub enum HandleResult {
    /// Message was handled, response sent
    Handled,
    /// Message was ignored (not for us)
    Ignored,
    /// Error occurred
    Error(String),
}

/// Orchestrates message handling between chat interface and AI agent
pub struct Orchestrator<I: ChatInterface> {
    interface: Arc<I>,
    session_store: SessionStore,
    warm_manager: SharedWarmSessionManager,
    config: OrchestratorConfig,
    warm_config: WarmConfig,
}

impl<I: ChatInterface> Orchestrator<I> {
    pub fn new(
        interface: Arc<I>,
        session_store: SessionStore,
        warm_manager: SharedWarmSessionManager,
        config: OrchestratorConfig,
        warm_config: WarmConfig,
    ) -> Self {
        Self {
            interface,
            session_store,
            warm_manager,
            config,
            warm_config,
        }
    }

    /// Handle an incoming message
    pub async fn handle_message(&self, msg: IncomingMessage) -> Result<HandleResult> {
        // Skip our own messages
        if self.interface.is_self(&msg.sender.id) {
            return Ok(HandleResult::Ignored);
        }

        // Check allowed users
        if !self.config.allowed_users.is_empty()
            && !self.config.allowed_users.contains(&msg.sender.id)
        {
            tracing::debug!(sender = %msg.sender.id, "User not in allowed list");
            return Ok(HandleResult::Ignored);
        }

        // Get the room
        let room = match self.interface.get_room(&msg.room_id).await {
            Some(r) => r,
            None => {
                tracing::warn!(room_id = %msg.room_id, "Room not found");
                return Ok(HandleResult::Ignored);
            }
        };

        // Parse the message
        let parsed = parse_message(&msg.body, &self.config.bot_prefix);

        match parsed {
            ParseResult::Ignore => Ok(HandleResult::Ignored),
            ParseResult::Command(cmd) => {
                self.handle_command(&room, &msg, cmd).await
            }
            ParseResult::Message(body) => {
                self.handle_agent_message(&room, &msg, &body).await
            }
        }
    }

    /// Handle a command
    async fn handle_command(
        &self,
        room: &I::Room,
        msg: &IncomingMessage,
        cmd: Command,
    ) -> Result<HandleResult> {
        let std_cmd = cmd.as_standard();

        match std_cmd {
            StandardCommand::Help => {
                room.send(MessageContent::plain(self.help_text())).await?;
                Ok(HandleResult::Handled)
            }
            StandardCommand::Status => {
                let status = self.get_status(room).await?;
                room.send(MessageContent::plain(status)).await?;
                Ok(HandleResult::Handled)
            }
            StandardCommand::Reset => {
                self.handle_reset(room, msg).await
            }
            StandardCommand::Backend(sub) => {
                self.handle_backend_command(room, msg, sub).await
            }
            StandardCommand::Schedule(sub) => {
                self.handle_schedule_command(room, msg, sub).await
            }
            StandardCommand::Debug(sub) => {
                self.handle_debug_command(room, msg, sub).await
            }
            StandardCommand::Unknown(name) => {
                room.send(MessageContent::plain(format!(
                    "Unknown command: {}. Try !help for available commands.",
                    name
                ))).await?;
                Ok(HandleResult::Handled)
            }
        }
    }

    /// Handle a message that should go to the AI agent
    async fn handle_agent_message(
        &self,
        room: &I::Room,
        msg: &IncomingMessage,
        body: &str,
    ) -> Result<HandleResult> {
        // Get channel for this room
        let channel = match self.session_store.get_by_room(room.id())? {
            Some(c) => c,
            None => {
                room.send(MessageContent::plain(
                    "No channel configured for this room. Use !help for setup instructions."
                )).await?;
                return Ok(HandleResult::Handled);
            }
        };

        // Start typing indicator
        room.set_typing(true).await?;

        // Prepare session and send to agent
        let handle = match prepare_session_async(
            &self.session_store,
            &channel,
            self.warm_manager.clone(),
            &self.warm_config,
        ).await {
            Ok(h) => h,
            Err(e) => {
                room.set_typing(false).await?;
                room.send(MessageContent::plain(format!("Error preparing session: {}", e))).await?;
                return Ok(HandleResult::Error(e.to_string()));
            }
        };

        // Send prompt to agent
        if let Err(e) = handle.send_prompt(body).await {
            room.set_typing(false).await?;
            room.send(MessageContent::plain(format!("Error sending prompt: {}", e))).await?;
            return Ok(HandleResult::Error(e.to_string()));
        }

        // Process agent events
        let mut full_response = String::new();
        let mut event_rx = handle.events();

        while let Ok(event) = event_rx.recv().await {
            match event {
                AgentEvent::Text(text) => {
                    full_response.push_str(&text);
                }
                AgentEvent::ToolUse { name, input } => {
                    tracing::debug!(tool = %name, "Agent using tool");
                }
                AgentEvent::ToolResult { name, output } => {
                    tracing::debug!(tool = %name, "Tool completed");
                }
                AgentEvent::Completed { session_id, .. } => {
                    // Update session ID if changed
                    if let Err(e) = self.session_store.update_session_id(room.id(), &session_id) {
                        tracing::warn!(error = %e, "Failed to update session ID");
                    }
                    break;
                }
                AgentEvent::Error(e) => {
                    room.set_typing(false).await?;
                    room.send(MessageContent::plain(format!("Agent error: {}", e))).await?;
                    return Ok(HandleResult::Error(e));
                }
                _ => {}
            }
        }

        room.set_typing(false).await?;

        // Send response, chunking if necessary
        let response = strip_function_calls(&full_response);
        if response.len() <= MAX_CHUNK_SIZE {
            let html = markdown_to_html(&response);
            room.send(MessageContent::html(&response, &html)).await?;
        } else {
            for chunk in chunk_message(&response, MAX_CHUNK_SIZE) {
                let html = markdown_to_html(&chunk);
                room.send(MessageContent::html(&chunk, &html)).await?;
            }
        }

        Ok(HandleResult::Handled)
    }

    fn help_text(&self) -> String {
        format!(
            "Available commands:\n\
            • !help - Show this help\n\
            • !status - Show channel status\n\
            • !reset - Reset Claude session\n\
            • !backend [list|set <type>] - Manage backend\n\
            • !schedule [list|add|remove] - Manage schedules\n\
            • !debug [on|off] - Toggle debug mode\n\
            \n\
            Or just type a message to chat with Claude."
        )
    }

    async fn get_status(&self, room: &I::Room) -> Result<String> {
        match self.session_store.get_by_room(room.id())? {
            Some(channel) => Ok(format!(
                "Channel: {}\nSession: {}\nBackend: {}\nDirectory: {}",
                channel.channel_name,
                channel.session_id,
                channel.backend_type.as_deref().unwrap_or("default"),
                channel.directory
            )),
            None => Ok("No channel configured for this room.".to_string()),
        }
    }

    async fn handle_reset(&self, room: &I::Room, _msg: &IncomingMessage) -> Result<HandleResult> {
        if let Some(channel) = self.session_store.get_by_room(room.id())? {
            // Invalidate warm session
            let mut mgr = self.warm_manager.write().await;
            mgr.invalidate_session(&channel.channel_name);

            // Reset in database
            self.session_store.reset_orphaned_session(room.id())?;

            room.send(MessageContent::plain("Session reset. Next message will start fresh.")).await?;
        } else {
            room.send(MessageContent::plain("No channel to reset.")).await?;
        }
        Ok(HandleResult::Handled)
    }

    async fn handle_backend_command(
        &self,
        room: &I::Room,
        _msg: &IncomingMessage,
        sub: crate::commands::BackendSubcommand,
    ) -> Result<HandleResult> {
        use crate::commands::BackendSubcommand;

        match sub {
            BackendSubcommand::Get => {
                if let Some(channel) = self.session_store.get_by_room(room.id())? {
                    let backend = channel.backend_type.as_deref().unwrap_or("default");
                    room.send(MessageContent::plain(format!("Current backend: {}", backend))).await?;
                } else {
                    room.send(MessageContent::plain("No channel configured.")).await?;
                }
            }
            BackendSubcommand::List => {
                room.send(MessageContent::plain(
                    "Available backends: acp, mux, direct"
                )).await?;
            }
            BackendSubcommand::Set { backend } => {
                if let Some(channel) = self.session_store.get_by_room(room.id())? {
                    self.session_store.update_backend_type(&channel.channel_name, Some(&backend))?;
                    let mut mgr = self.warm_manager.write().await;
                    mgr.invalidate_session(&channel.channel_name);
                    room.send(MessageContent::plain(format!("Backend changed to: {}", backend))).await?;
                } else {
                    room.send(MessageContent::plain("No channel configured.")).await?;
                }
            }
            BackendSubcommand::Reset => {
                if let Some(channel) = self.session_store.get_by_room(room.id())? {
                    self.session_store.update_backend_type(&channel.channel_name, None)?;
                    let mut mgr = self.warm_manager.write().await;
                    mgr.invalidate_session(&channel.channel_name);
                    room.send(MessageContent::plain("Backend reset to default.")).await?;
                } else {
                    room.send(MessageContent::plain("No channel configured.")).await?;
                }
            }
        }
        Ok(HandleResult::Handled)
    }

    async fn handle_schedule_command(
        &self,
        room: &I::Room,
        _msg: &IncomingMessage,
        _sub: crate::commands::ScheduleSubcommand,
    ) -> Result<HandleResult> {
        // TODO: Implement schedule commands
        room.send(MessageContent::plain("Schedule commands not yet implemented in core.")).await?;
        Ok(HandleResult::Handled)
    }

    async fn handle_debug_command(
        &self,
        room: &I::Room,
        _msg: &IncomingMessage,
        _sub: crate::commands::DebugSubcommand,
    ) -> Result<HandleResult> {
        // TODO: Implement debug commands
        room.send(MessageContent::plain("Debug commands not yet implemented in core.")).await?;
        Ok(HandleResult::Handled)
    }
}
```

**Step 2: Verify compilation**

Run: `cd gorp-core && cargo check`
Expected: May need adjustments for imports

**Step 3: Commit**

```bash
git add gorp-core/src/orchestrator.rs
git commit -m "feat(gorp-core): add message orchestrator with trait-based design"
```

---

## Phase 5: Implement Matrix Interface

### Task 5.1: Create MatrixRoom implementation

**Files:**
- Create: `src/matrix_interface.rs`

**Step 1: Implement ChatRoom for Matrix**

```rust
// ABOUTME: Matrix implementation of ChatRoom trait from gorp-core
// ABOUTME: Wraps matrix_sdk::Room with platform-agnostic interface

use anyhow::Result;
use async_trait::async_trait;
use gorp_core::traits::{ChatRoom, MessageContent};
use matrix_sdk::{
    media::{MediaFormat, MediaRequestParameters},
    room::Room,
    ruma::events::room::message::RoomMessageEventContent,
    Client,
};
use std::sync::Arc;

/// Matrix implementation of ChatRoom
#[derive(Debug, Clone)]
pub struct MatrixRoom {
    room: Room,
    client: Arc<Client>,
}

impl MatrixRoom {
    pub fn new(room: Room, client: Arc<Client>) -> Self {
        Self { room, client }
    }

    /// Get the underlying Matrix room
    pub fn inner(&self) -> &Room {
        &self.room
    }
}

#[async_trait]
impl ChatRoom for MatrixRoom {
    fn id(&self) -> &str {
        self.room.room_id().as_str()
    }

    fn name(&self) -> Option<String> {
        self.room.name()
    }

    async fn is_direct_message(&self) -> bool {
        self.room.is_direct().await.unwrap_or(false)
    }

    async fn send(&self, content: MessageContent) -> Result<()> {
        match content {
            MessageContent::Plain(text) => {
                self.room
                    .send(RoomMessageEventContent::text_plain(text))
                    .await?;
            }
            MessageContent::Html { plain, html } => {
                self.room
                    .send(RoomMessageEventContent::text_html(plain, html))
                    .await?;
            }
            MessageContent::Attachment {
                filename,
                data,
                mime_type,
                caption,
            } => {
                // TODO: Implement attachment sending
                tracing::warn!("Attachment sending not yet implemented");
            }
        }
        Ok(())
    }

    async fn set_typing(&self, typing: bool) -> Result<()> {
        self.room.typing_notice(typing).await?;
        Ok(())
    }

    async fn download_attachment(&self, source_id: &str) -> Result<(String, Vec<u8>, String)> {
        // Parse source_id as MXC URI
        let mxc_uri: matrix_sdk::ruma::OwnedMxcUri = source_id.parse()?;

        let request = MediaRequestParameters {
            source: matrix_sdk::media::MediaSource::Plain(mxc_uri),
            format: MediaFormat::File,
        };

        let data = self.client.media().get_media_content(&request, true).await?;

        // Extract filename from source_id or use default
        let filename = source_id
            .split('/')
            .last()
            .unwrap_or("attachment")
            .to_string();

        Ok((filename, data, "application/octet-stream".to_string()))
    }
}
```

**Step 2: Implement ChatInterface for Matrix**

```rust
/// Matrix implementation of ChatInterface
pub struct MatrixInterface {
    client: Arc<Client>,
    user_id: String,
}

impl MatrixInterface {
    pub fn new(client: Client) -> Self {
        let user_id = client.user_id().map(|u| u.to_string()).unwrap_or_default();
        Self {
            client: Arc::new(client),
            user_id,
        }
    }
}

#[async_trait]
impl gorp_core::traits::ChatInterface for MatrixInterface {
    type Room = MatrixRoom;

    async fn get_room(&self, room_id: &str) -> Option<Self::Room> {
        let room_id: matrix_sdk::ruma::OwnedRoomId = room_id.parse().ok()?;
        self.client
            .get_room(&room_id)
            .map(|room| MatrixRoom::new(room, Arc::clone(&self.client)))
    }

    fn bot_user_id(&self) -> &str {
        &self.user_id
    }
}
```

**Step 3: Commit**

```bash
git add src/matrix_interface.rs
git commit -m "feat: implement ChatRoom and ChatInterface for Matrix"
```

---

### Task 5.2: Update gorp Cargo.toml

**Files:**
- Modify: `Cargo.toml`

**Step 1: Add gorp-core dependency**

Add to dependencies:

```toml
gorp-core = { path = "gorp-core" }
```

**Step 2: Verify build**

Run: `cargo build`
Expected: PASS

**Step 3: Commit**

```bash
git add Cargo.toml
git commit -m "feat: add gorp-core dependency"
```

---

## Phase 6: Integration and Testing

### Task 6.1: Create integration tests for gorp-core

**Files:**
- Create: `gorp-core/tests/orchestrator_tests.rs`

**Step 1: Create mock ChatInterface for testing**

```rust
// ABOUTME: Integration tests for gorp-core orchestrator
// ABOUTME: Uses mock ChatInterface implementation

use async_trait::async_trait;
use gorp_core::traits::{ChatInterface, ChatRoom, ChatUser, IncomingMessage, MessageContent};
use std::sync::{Arc, Mutex};

/// Mock room that records sent messages
#[derive(Debug, Clone)]
pub struct MockRoom {
    id: String,
    name: Option<String>,
    is_dm: bool,
    sent_messages: Arc<Mutex<Vec<MessageContent>>>,
    typing_state: Arc<Mutex<bool>>,
}

impl MockRoom {
    pub fn new(id: &str) -> Self {
        Self {
            id: id.to_string(),
            name: Some(format!("Room {}", id)),
            is_dm: false,
            sent_messages: Arc::new(Mutex::new(Vec::new())),
            typing_state: Arc::new(Mutex::new(false)),
        }
    }

    pub fn sent_messages(&self) -> Vec<MessageContent> {
        self.sent_messages.lock().unwrap().clone()
    }
}

#[async_trait]
impl ChatRoom for MockRoom {
    fn id(&self) -> &str {
        &self.id
    }

    fn name(&self) -> Option<String> {
        self.name.clone()
    }

    async fn is_direct_message(&self) -> bool {
        self.is_dm
    }

    async fn send(&self, content: MessageContent) -> anyhow::Result<()> {
        self.sent_messages.lock().unwrap().push(content);
        Ok(())
    }

    async fn set_typing(&self, typing: bool) -> anyhow::Result<()> {
        *self.typing_state.lock().unwrap() = typing;
        Ok(())
    }

    async fn download_attachment(&self, _source_id: &str) -> anyhow::Result<(String, Vec<u8>, String)> {
        Ok(("test.txt".to_string(), vec![1, 2, 3], "text/plain".to_string()))
    }
}

/// Mock interface for testing
pub struct MockInterface {
    rooms: std::collections::HashMap<String, MockRoom>,
    bot_id: String,
}

impl MockInterface {
    pub fn new() -> Self {
        Self {
            rooms: std::collections::HashMap::new(),
            bot_id: "@bot:test.com".to_string(),
        }
    }

    pub fn add_room(&mut self, room: MockRoom) {
        self.rooms.insert(room.id.clone(), room);
    }
}

#[async_trait]
impl ChatInterface for MockInterface {
    type Room = MockRoom;

    async fn get_room(&self, room_id: &str) -> Option<Self::Room> {
        self.rooms.get(room_id).cloned()
    }

    fn bot_user_id(&self) -> &str {
        &self.bot_id
    }
}

#[tokio::test]
async fn test_help_command() {
    let mut interface = MockInterface::new();
    let room = MockRoom::new("!test:example.com");
    interface.add_room(room.clone());

    // Create orchestrator and handle help command
    // ... (requires full setup with session_store etc.)
}
```

**Step 2: Run tests**

Run: `cd gorp-core && cargo test`
Expected: PASS

**Step 3: Commit**

```bash
git add gorp-core/tests/
git commit -m "test(gorp-core): add integration tests with mock interface"
```

---

## Phase 7: Gradual Migration

### Task 7.1: Update message_handler to use orchestrator

This is the gradual migration phase where we:
1. Keep existing message_handler working
2. Add new code paths using orchestrator
3. Feature-flag the switch
4. Eventually remove old code

**Files:**
- Modify: `src/message_handler.rs`
- Modify: `src/lib.rs`

This is a larger refactoring task that should be done incrementally.

---

## Summary

### Crate Structure After Completion

```
gorp-rs/
├── gorp-core/           # Platform-agnostic library
│   ├── src/
│   │   ├── lib.rs
│   │   ├── traits.rs    # ChatInterface, ChatRoom, ChatUser
│   │   ├── commands.rs  # Generic command parsing
│   │   ├── orchestrator.rs
│   │   ├── session.rs
│   │   ├── warm_session.rs
│   │   ├── scheduler.rs
│   │   ├── config.rs
│   │   ├── paths.rs
│   │   ├── metrics.rs
│   │   └── utils.rs
│   └── tests/
├── gorp-agent/          # Backend abstraction (unchanged)
└── src/                 # Matrix-specific binary
    ├── main.rs
    ├── matrix_interface.rs  # impl ChatInterface
    ├── message_handler.rs   # Uses orchestrator
    └── ...
```

### Future Platform Implementations

With this architecture, adding new platforms is straightforward:

```rust
// gorp-slack/src/lib.rs
pub struct SlackRoom { /* ... */ }
impl ChatRoom for SlackRoom { /* ... */ }

pub struct SlackInterface { /* ... */ }
impl ChatInterface for SlackInterface { /* ... */ }
```

### Breaking Changes

- `gorp` binary CLI unchanged
- Configuration unchanged
- Database schema unchanged
- All existing functionality preserved

---

## Execution Checklist

- [ ] Phase 1: Create gorp-core crate skeleton
- [ ] Phase 1: Define core traits
- [ ] Phase 2: Move paths.rs
- [ ] Phase 2: Move utils.rs
- [ ] Phase 2: Move metrics.rs
- [ ] Phase 2: Move config.rs
- [ ] Phase 2: Move session.rs
- [ ] Phase 2: Move warm_session.rs
- [ ] Phase 2: Move scheduler.rs
- [ ] Phase 3: Create commands module
- [ ] Phase 4: Create orchestrator
- [ ] Phase 5: Implement Matrix interface
- [ ] Phase 5: Update gorp dependencies
- [ ] Phase 6: Integration tests
- [ ] Phase 7: Gradual migration
