# ACP Migration Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Replace Claude CLI spawning with ACP protocol for cleaner architecture and agent flexibility.

**Architecture:** Spawn `claude-code-acp` per channel, communicate via ACP JSON-RPC over stdio. Implement `acp::Client` trait to handle session notifications and auto-approve permissions.

**Tech Stack:** `agent-client-protocol` crate, `tokio`, `tokio-util` (compat), `claude-code-acp` npm package

---

## Task 1: Add Dependencies

**Files:**
- Modify: `Cargo.toml`

**Step 1: Add agent-client-protocol and tokio-util**

```toml
[dependencies]
# ... existing deps ...
agent-client-protocol = "0.9"
tokio-util = { version = "0.7", features = ["compat"] }
```

**Step 2: Run cargo check**

Run: `cargo check`
Expected: Compiles successfully, new deps downloaded

**Step 3: Commit**

```bash
git add Cargo.toml Cargo.lock
git commit -m "chore: add agent-client-protocol dependency"
```

---

## Task 2: Create ACP Client Module Skeleton

**Files:**
- Create: `src/acp_client.rs`
- Modify: `src/lib.rs`

**Step 1: Write the failing test**

Create `src/acp_client.rs`:

```rust
// ABOUTME: This module provides an ACP (Agent Client Protocol) client for communicating with AI agents.
// ABOUTME: It replaces direct Claude CLI spawning with the standardized ACP protocol over stdio.

use agent_client_protocol as acp;
use anyhow::Result;
use std::path::Path;
use tokio::sync::mpsc;

/// Events emitted during ACP agent execution
#[derive(Debug, Clone)]
pub enum AcpEvent {
    /// Agent is calling a tool
    ToolUse { name: String, input_preview: String },
    /// Text chunk from agent
    Text(String),
    /// Final result with optional usage stats
    Result { text: String },
    /// Error occurred
    Error(String),
    /// Session is invalid/orphaned
    InvalidSession,
}

/// ACP client for communicating with an agent process
pub struct AcpClient {
    // TODO: implement
}

impl AcpClient {
    /// Spawn a new agent process and establish ACP connection
    pub async fn spawn(_working_dir: &Path, _agent_binary: &str) -> Result<Self> {
        todo!("implement spawn")
    }

    /// Initialize the ACP connection
    pub async fn initialize(&self) -> Result<()> {
        todo!("implement initialize")
    }

    /// Create a new session
    pub async fn new_session(&self) -> Result<String> {
        todo!("implement new_session")
    }

    /// Load an existing session by ID
    pub async fn load_session(&self, _session_id: &str) -> Result<()> {
        todo!("implement load_session")
    }

    /// Send a prompt and receive streaming events
    pub async fn prompt(&self, _session_id: &str, _text: &str) -> Result<mpsc::Receiver<AcpEvent>> {
        todo!("implement prompt")
    }

    /// Cancel the current operation
    pub async fn cancel(&self) -> Result<()> {
        todo!("implement cancel")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_acp_event_variants() {
        let event = AcpEvent::ToolUse {
            name: "Read".to_string(),
            input_preview: "file.txt".to_string(),
        };
        assert!(matches!(event, AcpEvent::ToolUse { .. }));

        let event = AcpEvent::Result { text: "done".to_string() };
        assert!(matches!(event, AcpEvent::Result { .. }));
    }
}
```

**Step 2: Add module to lib.rs**

In `src/lib.rs`, add:

```rust
pub mod acp_client;
```

**Step 3: Run test to verify it compiles**

Run: `cargo test acp_client::tests::test_acp_event_variants`
Expected: PASS (the enum test should work)

**Step 4: Commit**

```bash
git add src/acp_client.rs src/lib.rs
git commit -m "feat(acp): add acp_client module skeleton"
```

---

## Task 3: Implement ACP Client Handler

**Files:**
- Modify: `src/acp_client.rs`

**Step 1: Write the Client trait implementation**

Add to `src/acp_client.rs`:

```rust
use acp::Agent as _;
use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Handler for ACP client-side callbacks
struct AcpClientHandler {
    event_tx: Arc<Mutex<Option<mpsc::Sender<AcpEvent>>>>,
}

impl AcpClientHandler {
    fn new() -> Self {
        Self {
            event_tx: Arc::new(Mutex::new(None)),
        }
    }

    fn set_event_sender(&self, tx: mpsc::Sender<AcpEvent>) {
        if let Ok(mut guard) = self.event_tx.try_lock() {
            *guard = Some(tx);
        }
    }

    async fn send_event(&self, event: AcpEvent) {
        if let Some(tx) = self.event_tx.lock().await.as_ref() {
            let _ = tx.send(event).await;
        }
    }
}

#[async_trait(?Send)]
impl acp::Client for AcpClientHandler {
    async fn request_permission(
        &self,
        _args: acp::RequestPermissionRequest,
    ) -> acp::Result<acp::RequestPermissionResponse> {
        // Auto-approve all permission requests
        tracing::debug!("Auto-approving permission request");
        Ok(acp::RequestPermissionResponse { approved: true })
    }

    async fn write_text_file(
        &self,
        _args: acp::WriteTextFileRequest,
    ) -> acp::Result<acp::WriteTextFileResponse> {
        Err(acp::Error::method_not_found())
    }

    async fn read_text_file(
        &self,
        _args: acp::ReadTextFileRequest,
    ) -> acp::Result<acp::ReadTextFileResponse> {
        Err(acp::Error::method_not_found())
    }

    async fn create_terminal(
        &self,
        _args: acp::CreateTerminalRequest,
    ) -> acp::Result<acp::CreateTerminalResponse> {
        Err(acp::Error::method_not_found())
    }

    async fn terminal_output(
        &self,
        _args: acp::TerminalOutputRequest,
    ) -> acp::Result<acp::TerminalOutputResponse> {
        Err(acp::Error::method_not_found())
    }

    async fn release_terminal(
        &self,
        _args: acp::ReleaseTerminalRequest,
    ) -> acp::Result<acp::ReleaseTerminalResponse> {
        Err(acp::Error::method_not_found())
    }

    async fn wait_for_terminal_exit(
        &self,
        _args: acp::WaitForTerminalExitRequest,
    ) -> acp::Result<acp::WaitForTerminalExitResponse> {
        Err(acp::Error::method_not_found())
    }

    async fn kill_terminal_command(
        &self,
        _args: acp::KillTerminalCommandRequest,
    ) -> acp::Result<acp::KillTerminalCommandResponse> {
        Err(acp::Error::method_not_found())
    }

    async fn session_notification(
        &self,
        args: acp::SessionNotification,
    ) -> acp::Result<(), acp::Error> {
        match args.update {
            acp::SessionUpdate::AgentMessageChunk(chunk) => {
                let text = match chunk.content {
                    acp::ContentBlock::Text(t) => t.text,
                    acp::ContentBlock::Image(_) => "<image>".into(),
                    acp::ContentBlock::Audio(_) => "<audio>".into(),
                    acp::ContentBlock::ResourceLink(r) => r.uri,
                    acp::ContentBlock::Resource(_) => "<resource>".into(),
                };
                if !text.is_empty() {
                    self.send_event(AcpEvent::Text(text)).await;
                }
            }
            acp::SessionUpdate::ToolCall(tool_call) => {
                let name = tool_call.name.clone();
                let preview = tool_call
                    .input
                    .as_object()
                    .and_then(|o| o.get("command").or(o.get("file_path")).or(o.get("pattern")))
                    .and_then(|v| v.as_str())
                    .map(|s| s.chars().take(50).collect())
                    .unwrap_or_default();
                self.send_event(AcpEvent::ToolUse {
                    name,
                    input_preview: preview,
                })
                .await;
            }
            _ => {}
        }
        Ok(())
    }

    async fn ext_method(&self, _args: acp::ExtRequest) -> acp::Result<acp::ExtResponse> {
        Err(acp::Error::method_not_found())
    }

    async fn ext_notification(&self, _args: acp::ExtNotification) -> acp::Result<()> {
        Err(acp::Error::method_not_found())
    }
}
```

**Step 2: Run cargo check**

Run: `cargo check`
Expected: Compiles (may have warnings about unused)

**Step 3: Commit**

```bash
git add src/acp_client.rs
git commit -m "feat(acp): implement Client trait with auto-approve permissions"
```

---

## Task 4: Implement AcpClient::spawn

**Files:**
- Modify: `src/acp_client.rs`

**Step 1: Add spawn implementation**

Update `AcpClient` struct and `spawn`:

```rust
use tokio::process::{Child, Command};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};
use std::path::PathBuf;

pub struct AcpClient {
    _child: Child,
    conn: acp::ClientSideConnection<AcpClientHandler>,
    handler: Arc<AcpClientHandler>,
    working_dir: PathBuf,
}

impl AcpClient {
    pub async fn spawn(working_dir: &Path, agent_binary: &str) -> Result<Self> {
        // Validate inputs
        if agent_binary.contains("..") || agent_binary.contains('\0') {
            anyhow::bail!("Invalid agent binary path");
        }
        if !working_dir.exists() {
            anyhow::bail!("Working directory does not exist: {}", working_dir.display());
        }

        tracing::info!(binary = %agent_binary, cwd = %working_dir.display(), "Spawning ACP agent");

        let mut child = Command::new(agent_binary)
            .current_dir(working_dir)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .context("Failed to spawn ACP agent")?;

        let stdin = child.stdin.take().context("Failed to get stdin")?;
        let stdout = child.stdout.take().context("Failed to get stdout")?;

        let handler = Arc::new(AcpClientHandler::new());
        let handler_clone = Arc::clone(&handler);

        // Create ACP connection
        let (conn, handle_io) = acp::ClientSideConnection::new(
            (*handler_clone).clone(), // Handler needs to be cloneable
            stdin.compat_write(),
            stdout.compat(),
            |fut| {
                tokio::task::spawn_local(fut);
            },
        );

        // Spawn I/O handler
        tokio::task::spawn_local(handle_io);

        Ok(Self {
            _child: child,
            conn,
            handler,
            working_dir: working_dir.to_path_buf(),
        })
    }
}
```

**Step 2: Make handler Clone**

Add `#[derive(Clone)]` to `AcpClientHandler` and wrap internals appropriately.

**Step 3: Run cargo check**

Run: `cargo check`
Expected: Compiles

**Step 4: Commit**

```bash
git add src/acp_client.rs
git commit -m "feat(acp): implement AcpClient::spawn"
```

---

## Task 5: Implement Initialize, NewSession, LoadSession

**Files:**
- Modify: `src/acp_client.rs`

**Step 1: Implement initialize**

```rust
impl AcpClient {
    pub async fn initialize(&self) -> Result<()> {
        self.conn
            .initialize(acp::InitializeRequest {
                protocol_version: acp::V1,
                client_capabilities: acp::ClientCapabilities::default(),
                client_info: Some(acp::Implementation {
                    name: "gorp-acp".to_string(),
                    title: Some("Matrix-Claude Bridge".to_string()),
                    version: env!("CARGO_PKG_VERSION").to_string(),
                }),
                meta: None,
            })
            .await
            .context("ACP initialization failed")?;

        tracing::info!("ACP connection initialized");
        Ok(())
    }

    pub async fn new_session(&self) -> Result<String> {
        let response = self
            .conn
            .new_session(acp::NewSessionRequest {
                mcp_servers: Vec::new(),
                cwd: self.working_dir.clone(),
                meta: None,
            })
            .await
            .context("Failed to create new ACP session")?;

        let session_id = response.session_id.to_string();
        tracing::info!(session_id = %session_id, "Created new ACP session");
        Ok(session_id)
    }

    pub async fn load_session(&self, session_id: &str) -> Result<()> {
        self.conn
            .load_session(acp::LoadSessionRequest::new(
                acp::SessionId::from(session_id.to_string()),
                self.working_dir.clone(),
            ))
            .await
            .context("Failed to load ACP session")?;

        tracing::info!(session_id = %session_id, "Loaded existing ACP session");
        Ok(())
    }
}
```

**Step 2: Run cargo check**

Run: `cargo check`
Expected: Compiles

**Step 3: Commit**

```bash
git add src/acp_client.rs
git commit -m "feat(acp): implement initialize, new_session, load_session"
```

---

## Task 6: Implement Prompt with Event Streaming

**Files:**
- Modify: `src/acp_client.rs`

**Step 1: Implement prompt**

```rust
impl AcpClient {
    pub async fn prompt(&self, session_id: &str, text: &str) -> Result<mpsc::Receiver<AcpEvent>> {
        let (tx, rx) = mpsc::channel(32);
        self.handler.set_event_sender(tx.clone());

        tracing::debug!(session_id = %session_id, prompt_len = text.len(), "Sending prompt");

        let result = self
            .conn
            .prompt(acp::PromptRequest {
                session_id: acp::SessionId::from(session_id.to_string()),
                prompt: vec![acp::ContentBlock::Text(acp::TextContent {
                    text: text.to_string(),
                })],
                meta: None,
            })
            .await;

        match result {
            Ok(response) => {
                // Collect final response text
                let final_text = response
                    .content
                    .iter()
                    .filter_map(|block| match block {
                        acp::ContentBlock::Text(t) => Some(t.text.clone()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n");

                let _ = tx.send(AcpEvent::Result { text: final_text }).await;
            }
            Err(e) => {
                let error_msg = format!("ACP prompt error: {}", e);
                tracing::error!(%error_msg);
                let _ = tx.send(AcpEvent::Error(error_msg)).await;
            }
        }

        Ok(rx)
    }

    pub async fn cancel(&self) -> Result<()> {
        self.conn
            .cancel(acp::CancelRequest { meta: None })
            .await
            .context("Failed to cancel ACP operation")?;
        Ok(())
    }
}
```

**Step 2: Run cargo check**

Run: `cargo check`
Expected: Compiles

**Step 3: Commit**

```bash
git add src/acp_client.rs
git commit -m "feat(acp): implement prompt and cancel"
```

---

## Task 7: Add ACP Logging to .gorp/acp-messages.jsonl

**Files:**
- Modify: `src/acp_client.rs`

**Step 1: Add logging infrastructure**

This requires intercepting the JSON-RPC messages. For now, we'll log at the event level:

```rust
impl AcpClientHandler {
    async fn log_event(&self, working_dir: &Path, event: &AcpEvent) {
        let gorp_dir = working_dir.join(".gorp");
        if tokio::fs::create_dir_all(&gorp_dir).await.is_err() {
            return;
        }

        let log_path = gorp_dir.join("acp-messages.jsonl");
        let line = match serde_json::to_string(&event) {
            Ok(json) => format!("{}\n", json),
            Err(_) => return,
        };

        if let Ok(mut file) = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .await
        {
            use tokio::io::AsyncWriteExt;
            let _ = file.write_all(line.as_bytes()).await;
        }
    }
}
```

**Step 2: Add Serialize derive to AcpEvent**

```rust
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub enum AcpEvent {
    // ... variants
}
```

**Step 3: Run cargo check**

Run: `cargo check`
Expected: Compiles

**Step 4: Commit**

```bash
git add src/acp_client.rs
git commit -m "feat(acp): add event logging to acp-messages.jsonl"
```

---

## Task 8: Update Config for ACP

**Files:**
- Modify: `src/config.rs`

**Step 1: Read current config**

Read `src/config.rs` to understand current structure.

**Step 2: Replace claude config with acp config**

Change:
```rust
#[derive(Debug, Deserialize)]
pub struct ClaudeConfig {
    pub binary_path: Option<String>,
    pub sdk_url: Option<String>,
}
```

To:
```rust
#[derive(Debug, Deserialize)]
pub struct AcpConfig {
    pub agent_binary: Option<String>,
}

impl Default for AcpConfig {
    fn default() -> Self {
        Self {
            agent_binary: Some("claude-code-acp".to_string()),
        }
    }
}
```

Update the main config struct to use `AcpConfig` instead of `ClaudeConfig`.

**Step 3: Update config.toml.example**

Change `[claude]` section to `[acp]`.

**Step 4: Run cargo check**

Run: `cargo check`
Expected: Compiles (may have errors in message_handler.rs that we'll fix next)

**Step 5: Commit**

```bash
git add src/config.rs config.toml.example
git commit -m "feat(config): replace claude config with acp config"
```

---

## Task 9: Update Message Handler to Use ACP

**Files:**
- Modify: `src/message_handler.rs`

**Step 1: Read current message_handler.rs**

Understand how it currently uses `claude.rs`.

**Step 2: Replace claude invocations with acp_client**

This is the main integration work. Replace:
- `invoke_claude_streaming()` calls → `AcpClient::spawn()` + `prompt()`
- `ClaudeEvent` handling → `AcpEvent` handling
- Session management remains similar

**Step 3: Run cargo check**

Run: `cargo check`
Expected: Compiles

**Step 4: Run tests**

Run: `cargo test`
Expected: All tests pass

**Step 5: Commit**

```bash
git add src/message_handler.rs
git commit -m "feat: integrate ACP client into message handler"
```

---

## Task 10: Delete claude.rs

**Files:**
- Delete: `src/claude.rs`
- Modify: `src/lib.rs`

**Step 1: Remove claude module from lib.rs**

Remove `pub mod claude;` line.

**Step 2: Delete claude.rs**

```bash
rm src/claude.rs
```

**Step 3: Run cargo check**

Run: `cargo check`
Expected: Compiles with no references to deleted module

**Step 4: Run tests**

Run: `cargo test`
Expected: All tests pass

**Step 5: Commit**

```bash
git add -A
git commit -m "refactor: remove legacy claude.rs module"
```

---

## Task 11: Update Dockerfile

**Files:**
- Modify: `Dockerfile`

**Step 1: Add Node.js and claude-code-acp**

Add to Dockerfile:

```dockerfile
# Install Node.js for claude-code-acp
RUN apt-get update && apt-get install -y nodejs npm && rm -rf /var/lib/apt/lists/*

# Install claude-code-acp globally
RUN npm install -g @anthropic/claude-code-acp
```

**Step 2: Build Docker image**

Run: `docker build -t gorp-acp:test .`
Expected: Builds successfully

**Step 3: Commit**

```bash
git add Dockerfile
git commit -m "feat(docker): add claude-code-acp to image"
```

---

## Task 12: Write Scenario Tests

**Files:**
- Create: `.scratch/test-acp-new-session.sh`
- Create: `.scratch/test-acp-prompt.sh`
- Modify: `.gitignore`

**Step 1: Ensure .scratch is gitignored**

Add to `.gitignore`:
```
.scratch/
```

**Step 2: Create scenario test for new session**

Create `.scratch/test-acp-new-session.sh`:

```bash
#!/bin/bash
set -e

# Test: Create new channel, verify ACP session created
# Requires: Running gorp-acp instance, claude-code-acp installed

echo "Testing ACP new session flow..."

# Create a test channel via Matrix or webhook
curl -X POST http://localhost:13000/webhook/test-channel \
  -H "Content-Type: application/json" \
  -d '{"prompt": "Say hello"}'

# Check .gorp/acp-messages.jsonl exists
if [ -f "workspace/test-channel/.gorp/acp-messages.jsonl" ]; then
  echo "✓ ACP log file created"
else
  echo "✗ ACP log file missing"
  exit 1
fi

echo "✓ New session test passed"
```

**Step 3: Create scenario test for prompt**

Create `.scratch/test-acp-prompt.sh`:

```bash
#!/bin/bash
set -e

echo "Testing ACP prompt flow..."

# Send prompt and verify response
RESPONSE=$(curl -s -X POST http://localhost:13000/webhook/test-channel \
  -H "Content-Type: application/json" \
  -d '{"prompt": "What is 2+2?"}')

if echo "$RESPONSE" | grep -q "4"; then
  echo "✓ Prompt response received"
else
  echo "✗ No valid response"
  exit 1
fi

echo "✓ Prompt test passed"
```

**Step 4: Commit gitignore only**

```bash
git add .gitignore
git commit -m "chore: add .scratch to gitignore for scenario tests"
```

---

## Task 13: End-to-End Test

**Step 1: Start gorp-acp locally**

Run: `cargo run --release`

**Step 2: Run scenario tests**

```bash
chmod +x .scratch/*.sh
.scratch/test-acp-new-session.sh
.scratch/test-acp-prompt.sh
```

**Step 3: Verify Matrix messages work**

Send a message in a test Matrix channel, verify response arrives.

**Step 4: Check logs**

Verify `.gorp/acp-messages.jsonl` contains ACP events.

---

## Task 14: Extract Patterns to scenarios.jsonl

**Files:**
- Create: `scenarios.jsonl`

**Step 1: Document validated scenarios**

Create `scenarios.jsonl`:

```jsonl
{"name": "acp-new-session", "given": "fresh channel with no session", "when": "first message sent", "then": "ACP session created and logged to acp-messages.jsonl", "validates": "ACP Initialize + NewSession flow"}
{"name": "acp-prompt-response", "given": "existing ACP session", "when": "prompt sent via webhook", "then": "response received in Matrix", "validates": "ACP PromptRequest flow"}
{"name": "acp-tool-events", "given": "prompt requiring tool use", "when": "agent uses Read tool", "then": "ToolUse event emitted and logged", "validates": "SessionUpdate streaming with ToolCall"}
{"name": "acp-session-resume", "given": "existing session ID in DB", "when": "gorp-acp restarts and prompt sent", "then": "conversation context preserved", "validates": "LoadSession with persisted ID"}
```

**Step 2: Commit**

```bash
git add scenarios.jsonl
git commit -m "docs: add scenario test specifications"
```

---

## Task 15: Final Cleanup and PR

**Step 1: Run full test suite**

```bash
cargo test
cargo clippy
cargo fmt --check
```

**Step 2: Update README if needed**

Update any references to Claude CLI configuration.

**Step 3: Create final commit**

```bash
git add -A
git commit -m "feat: complete ACP migration

- Replace Claude CLI spawning with ACP protocol
- Add agent-client-protocol crate
- Implement AcpClient with auto-approve permissions
- Log events to .gorp/acp-messages.jsonl
- Update Docker image with claude-code-acp
- Add scenario test specifications

BREAKING CHANGE: config.toml [claude] section replaced with [acp]"
```

**Step 4: Push and create PR**

```bash
git push -u origin main
```
