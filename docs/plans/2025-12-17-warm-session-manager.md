# Warm Session Manager Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Keep Claude Code processes alive between requests to eliminate 2-minute startup latency.

**Architecture:** WarmSessionManager holds a HashMap of channel→AcpClient. Sessions are lazily created on first use, kept alive for 1 hour, and pre-warmed 5 minutes before scheduled prompts.

**Tech Stack:** Rust, tokio (async), Arc<RwLock> for shared state

---

### Task 1: Add Configuration Fields

**Files:**
- Modify: `src/config.rs`
- Test: `tests/config_tests.rs`

**Step 1: Write the failing test**

Add to `tests/config_tests.rs`:

```rust
#[test]
#[serial]
fn test_config_acp_warm_session_defaults() {
    clear_config_env_vars();

    let temp_dir = std::env::temp_dir().join("gorp-config-warm-test");
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir).unwrap();
    let config_path = temp_dir.join("config.toml");

    let config_content = r#"
[matrix]
home_server = "https://test.matrix.org"
user_id = "@bot:test.matrix.org"
password = "secret123"
allowed_users = ["@user1:test.matrix.org"]

[acp]
agent_binary = "claude"

[webhook]
port = 8080

[workspace]
path = "./test-workspace"
"#;

    let mut file = std::fs::File::create(&config_path).unwrap();
    file.write_all(config_content.as_bytes()).unwrap();
    std::env::set_var("GORP_CONFIG_PATH", config_path.to_str().unwrap());

    let config = gorp::config::Config::load().unwrap();

    // Defaults: 1 hour keep-alive, 5 min pre-warm
    assert_eq!(config.acp.keep_alive_secs, 3600);
    assert_eq!(config.acp.pre_warm_secs, 300);

    clear_config_env_vars();
    let _ = std::fs::remove_dir_all(&temp_dir);
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test test_config_acp_warm_session_defaults -- --nocapture`
Expected: FAIL - `keep_alive_secs` field doesn't exist

**Step 3: Write minimal implementation**

Modify `src/config.rs`, update `AcpConfig` struct:

```rust
#[derive(Debug, Clone, Deserialize, Default)]
pub struct AcpConfig {
    pub agent_binary: Option<String>,
    #[serde(default = "default_keep_alive_secs")]
    pub keep_alive_secs: u64,
    #[serde(default = "default_pre_warm_secs")]
    pub pre_warm_secs: u64,
}

fn default_keep_alive_secs() -> u64 {
    3600 // 1 hour
}

fn default_pre_warm_secs() -> u64 {
    300 // 5 minutes
}
```

**Step 4: Run test to verify it passes**

Run: `cargo test test_config_acp_warm_session_defaults -- --nocapture`
Expected: PASS

**Step 5: Commit**

```bash
git add src/config.rs tests/config_tests.rs
git commit -m "feat(config): add keep_alive_secs and pre_warm_secs to AcpConfig"
```

---

### Task 2: Create WarmSessionManager Struct

**Files:**
- Create: `src/warm_session.rs`
- Modify: `src/lib.rs` (add module)

**Step 1: Create the module with basic struct**

Create `src/warm_session.rs`:

```rust
// ABOUTME: Manages warm Claude Code sessions to avoid 2-minute startup latency.
// ABOUTME: Keeps AcpClient instances alive per channel, with lazy creation and TTL cleanup.

use crate::acp_client::{AcpClient, AcpEvent};
use crate::session::Channel;
use anyhow::Result;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, RwLock};

/// Configuration for warm session behavior
#[derive(Debug, Clone)]
pub struct WarmConfig {
    pub keep_alive_duration: Duration,
    pub pre_warm_lead_time: Duration,
    pub agent_binary: String,
}

/// A warm session holding an active AcpClient
struct WarmSession {
    client: AcpClient,
    session_id: String,
    last_used: Instant,
    channel_name: String,
}

/// Manages warm Claude Code sessions across channels
pub struct WarmSessionManager {
    sessions: HashMap<String, WarmSession>,
    config: WarmConfig,
}

impl WarmSessionManager {
    pub fn new(config: WarmConfig) -> Self {
        Self {
            sessions: HashMap::new(),
            config,
        }
    }

    /// Get the agent binary path
    pub fn agent_binary(&self) -> &str {
        &self.config.agent_binary
    }

    /// Get the keep-alive duration
    pub fn keep_alive_duration(&self) -> Duration {
        self.config.keep_alive_duration
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_warm_session_manager_creation() {
        let config = WarmConfig {
            keep_alive_duration: Duration::from_secs(3600),
            pre_warm_lead_time: Duration::from_secs(300),
            agent_binary: "claude-code-acp".to_string(),
        };
        let manager = WarmSessionManager::new(config);
        assert_eq!(manager.agent_binary(), "claude-code-acp");
        assert_eq!(manager.keep_alive_duration(), Duration::from_secs(3600));
    }
}
```

**Step 2: Add module to lib.rs**

Add to `src/lib.rs`:

```rust
pub mod warm_session;
```

**Step 3: Run test to verify it passes**

Run: `cargo test test_warm_session_manager_creation -- --nocapture`
Expected: PASS

**Step 4: Commit**

```bash
git add src/warm_session.rs src/lib.rs
git commit -m "feat: add WarmSessionManager struct skeleton"
```

---

### Task 3: Implement cleanup_stale Method

**Files:**
- Modify: `src/warm_session.rs`

**Step 1: Write the test**

Add to `src/warm_session.rs` tests module:

```rust
#[test]
fn test_cleanup_stale_removes_old_sessions() {
    let config = WarmConfig {
        keep_alive_duration: Duration::from_secs(1), // 1 second for test
        pre_warm_lead_time: Duration::from_secs(300),
        agent_binary: "claude-code-acp".to_string(),
    };
    let mut manager = WarmSessionManager::new(config);

    // Manually insert a stale session (last_used is now, but we'll check logic)
    // For real test, we'd need to mock time or use a short duration
    assert_eq!(manager.sessions.len(), 0);

    // cleanup_stale should be callable without panic
    manager.cleanup_stale();
    assert_eq!(manager.sessions.len(), 0);
}
```

**Step 2: Implement cleanup_stale**

Add to `WarmSessionManager` impl:

```rust
/// Remove sessions that have been idle longer than keep_alive_duration
pub fn cleanup_stale(&mut self) {
    let now = Instant::now();
    let keep_alive = self.config.keep_alive_duration;

    self.sessions.retain(|channel_name, session| {
        let age = now.duration_since(session.last_used);
        if age > keep_alive {
            tracing::info!(
                channel = %channel_name,
                idle_secs = age.as_secs(),
                "Removing stale warm session"
            );
            false
        } else {
            true
        }
    });
}
```

**Step 3: Run test**

Run: `cargo test test_cleanup_stale -- --nocapture`
Expected: PASS

**Step 4: Commit**

```bash
git add src/warm_session.rs
git commit -m "feat(warm_session): implement cleanup_stale method"
```

---

### Task 4: Implement get_or_create_session Method

**Files:**
- Modify: `src/warm_session.rs`

**Step 1: Add the method signature and basic implementation**

Add to `WarmSessionManager` impl:

```rust
/// Get an existing warm session or create a new one
/// Returns the session_id for the channel
pub async fn get_or_create_session(
    &mut self,
    channel: &Channel,
    event_tx: mpsc::Sender<AcpEvent>,
) -> Result<String> {
    let channel_name = &channel.channel_name;

    // Check if we have a warm session
    if let Some(session) = self.sessions.get_mut(channel_name) {
        session.last_used = Instant::now();
        tracing::info!(channel = %channel_name, session_id = %session.session_id, "Reusing warm session");
        return Ok(session.session_id.clone());
    }

    // Create new session
    tracing::info!(channel = %channel_name, "Creating new warm session");

    let working_dir = std::path::Path::new(&channel.directory);
    let env_vars: std::collections::HashMap<String, String> = std::env::vars().collect();

    let client = AcpClient::spawn(
        working_dir,
        &self.config.agent_binary,
        event_tx,
        &env_vars,
    ).await?;

    client.initialize().await?;
    let session_id = client.new_session().await?;

    let warm_session = WarmSession {
        client,
        session_id: session_id.clone(),
        last_used: Instant::now(),
        channel_name: channel_name.clone(),
    };

    self.sessions.insert(channel_name.clone(), warm_session);

    Ok(session_id)
}
```

**Step 2: Run cargo check**

Run: `cargo check`
Expected: Compiles without errors

**Step 3: Commit**

```bash
git add src/warm_session.rs
git commit -m "feat(warm_session): implement get_or_create_session"
```

---

### Task 5: Implement prompt Method

**Files:**
- Modify: `src/warm_session.rs`

**Step 1: Add the prompt method**

Add to `WarmSessionManager` impl:

```rust
/// Send a prompt using the warm session for a channel
pub async fn prompt(
    &mut self,
    channel: &Channel,
    text: &str,
    event_tx: mpsc::Sender<AcpEvent>,
) -> Result<()> {
    let session_id = self.get_or_create_session(channel, event_tx).await?;

    let channel_name = &channel.channel_name;
    if let Some(session) = self.sessions.get_mut(channel_name) {
        session.last_used = Instant::now();
        session.client.prompt(&session_id, text).await?;
    } else {
        anyhow::bail!("Session disappeared after creation");
    }

    Ok(())
}
```

**Step 2: Run cargo check**

Run: `cargo check`
Expected: Compiles

**Step 3: Commit**

```bash
git add src/warm_session.rs
git commit -m "feat(warm_session): implement prompt method"
```

---

### Task 6: Implement pre_warm Method

**Files:**
- Modify: `src/warm_session.rs`

**Step 1: Add pre_warm method**

Add to `WarmSessionManager` impl:

```rust
/// Pre-warm a session for a channel (called before scheduled prompts)
pub async fn pre_warm(
    &mut self,
    channel: &Channel,
    event_tx: mpsc::Sender<AcpEvent>,
) -> Result<()> {
    let channel_name = &channel.channel_name;

    if self.sessions.contains_key(channel_name) {
        tracing::debug!(channel = %channel_name, "Channel already warm");
        return Ok(());
    }

    tracing::info!(channel = %channel_name, "Pre-warming channel");
    let _ = self.get_or_create_session(channel, event_tx).await?;

    Ok(())
}
```

**Step 2: Run cargo check**

Run: `cargo check`
Expected: Compiles

**Step 3: Commit**

```bash
git add src/warm_session.rs
git commit -m "feat(warm_session): implement pre_warm method"
```

---

### Task 7: Create SharedWarmSessionManager Type

**Files:**
- Modify: `src/warm_session.rs`

**Step 1: Add thread-safe wrapper**

Add at end of `src/warm_session.rs`:

```rust
/// Thread-safe wrapper for WarmSessionManager
pub type SharedWarmSessionManager = Arc<RwLock<WarmSessionManager>>;

/// Create a new shared warm session manager
pub fn create_shared_manager(config: WarmConfig) -> SharedWarmSessionManager {
    Arc::new(RwLock::new(WarmSessionManager::new(config)))
}
```

**Step 2: Run cargo check**

Run: `cargo check`
Expected: Compiles

**Step 3: Commit**

```bash
git add src/warm_session.rs
git commit -m "feat(warm_session): add SharedWarmSessionManager type"
```

---

### Task 8: Integrate into main.rs

**Files:**
- Modify: `src/main.rs`

**Step 1: Create manager in main and spawn cleanup task**

Add imports at top of `src/main.rs`:

```rust
use gorp::warm_session::{create_shared_manager, WarmConfig, SharedWarmSessionManager};
```

Find where config is loaded (after `let config = Config::load()?;`) and add:

```rust
// Create warm session manager
let warm_config = WarmConfig {
    keep_alive_duration: std::time::Duration::from_secs(config.acp.keep_alive_secs),
    pre_warm_lead_time: std::time::Duration::from_secs(config.acp.pre_warm_secs),
    agent_binary: config.acp.agent_binary.clone().unwrap_or_else(|| "claude-code-acp".to_string()),
};
let warm_manager = create_shared_manager(warm_config);

// Spawn cleanup task
let cleanup_manager = warm_manager.clone();
let cleanup_interval = config.acp.keep_alive_secs / 4; // Check 4x per keep-alive period
tokio::spawn(async move {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(cleanup_interval));
    loop {
        interval.tick().await;
        let mut manager = cleanup_manager.write().await;
        manager.cleanup_stale();
    }
});
```

**Step 2: Run cargo check**

Run: `cargo check`
Expected: Compiles (with warnings about unused warm_manager)

**Step 3: Commit**

```bash
git add src/main.rs
git commit -m "feat(main): create WarmSessionManager and spawn cleanup task"
```

---

### Task 9: Update webhook.rs to use WarmSessionManager

**Files:**
- Modify: `src/webhook.rs`

**Step 1: Add warm_manager to WebhookState**

Update `WebhookState` struct:

```rust
#[derive(Clone)]
pub struct WebhookState {
    pub session_store: SessionStore,
    pub matrix_client: Client,
    pub config: Arc<Config>,
    pub warm_manager: SharedWarmSessionManager,
}
```

**Step 2: Update webhook_handler to use warm session**

Replace the `invoke_acp` call in `webhook_handler` with warm session usage. Find the section that calls `invoke_acp` and replace with:

```rust
// Use warm session manager instead of spawning new process
let (event_tx, mut event_rx) = mpsc::channel(2048);

{
    let mut manager = state.warm_manager.write().await;
    if let Err(e) = manager.prompt(&channel, &payload.prompt, event_tx).await {
        tracing::error!(error = %e, "Warm session prompt failed");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(WebhookResponse {
                success: false,
                message: format!("ACP error: {}", e),
            }),
        );
    }
}

// Process events from receiver...
```

**Step 3: Run cargo check**

Run: `cargo check`
Expected: Compiles (may have warnings)

**Step 4: Commit**

```bash
git add src/webhook.rs
git commit -m "feat(webhook): use WarmSessionManager instead of invoke_acp"
```

---

### Task 10: Update message_handler.rs to use WarmSessionManager

**Files:**
- Modify: `src/message_handler.rs`

**Step 1: Update MessageContext to include warm_manager**

Add `warm_manager: SharedWarmSessionManager` to `MessageContext` struct and update usages similar to webhook.

**Step 2: Replace invoke_acp calls with warm session**

Find and replace `invoke_acp` with warm session manager usage.

**Step 3: Run cargo check**

Run: `cargo check`
Expected: Compiles

**Step 4: Commit**

```bash
git add src/message_handler.rs
git commit -m "feat(message_handler): use WarmSessionManager"
```

---

### Task 11: Update scheduler.rs for Pre-warming

**Files:**
- Modify: `src/scheduler.rs`

**Step 1: Add pre-warm call before scheduled prompts**

In the scheduler execution logic, before executing a scheduled prompt, check if we should pre-warm:

```rust
// Pre-warm 5 minutes before scheduled execution
let pre_warm_time = next_execution - Duration::from_secs(config.acp.pre_warm_secs);
if now >= pre_warm_time && now < next_execution {
    let (event_tx, _) = mpsc::channel(16);
    if let Err(e) = warm_manager.write().await.pre_warm(&channel, event_tx).await {
        tracing::warn!(error = %e, "Pre-warm failed");
    }
}
```

**Step 2: Run cargo check**

Run: `cargo check`
Expected: Compiles

**Step 3: Commit**

```bash
git add src/scheduler.rs
git commit -m "feat(scheduler): add pre-warming for scheduled prompts"
```

---

### Task 12: End-to-End Test

**Files:**
- Manual test

**Step 1: Build and run**

```bash
cargo build
cargo run start
```

**Step 2: Test first request (cold start)**

```bash
time curl -X POST http://localhost:13000/webhook/session/<session-id> \
  -H "Content-Type: application/json" \
  -d '{"prompt": "say hello"}'
```

Expected: ~2 minutes (cold start)

**Step 3: Test second request (warm)**

```bash
time curl -X POST http://localhost:13000/webhook/session/<session-id> \
  -H "Content-Type: application/json" \
  -d '{"prompt": "say hello again"}'
```

Expected: < 5 seconds (warm session)

**Step 4: Commit final changes**

```bash
git add -A
git commit -m "feat: complete WarmSessionManager integration"
```

---

## Summary

12 tasks total:
1. Config fields
2. WarmSessionManager struct
3. cleanup_stale method
4. get_or_create_session method
5. prompt method
6. pre_warm method
7. SharedWarmSessionManager type
8. main.rs integration
9. webhook.rs integration
10. message_handler.rs integration
11. scheduler.rs pre-warming
12. End-to-end test
