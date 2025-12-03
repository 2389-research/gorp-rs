# Matrix-Claude Bridge Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Build a Rust Matrix bot that bridges one encrypted room to Claude Code CLI with persistent sessions and whitelist-based access control.

**Architecture:** Async Rust binary using matrix-sdk for E2E encrypted Matrix messaging, sled for session persistence, and tokio::process to spawn claude CLI. Each message triggers Claude invocation with session context, responses return to Matrix.

**Tech Stack:** Rust, tokio, matrix-sdk (e2e-encryption), sled, tracing, dotenvy, serde

---

## Task 1: Project Initialization

**Files:**
- Create: `Cargo.toml`
- Create: `.env.example`
- Create: `.gitignore`
- Create: `README.md`

**Step 1: Create Cargo.toml with dependencies**

Create `Cargo.toml`:

```toml
[package]
name = "matrix-bridge"
version = "0.1.0"
edition = "2021"

[dependencies]
tokio = { version = "1", features = ["full"] }
matrix-sdk = { version = "0.7", features = ["e2e-encryption"] }
sled = "0.34"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
dotenvy = "0.15"
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
anyhow = "1.0"
```

**Step 2: Create .env.example**

Create `.env.example`:

```bash
# Matrix Settings
MATRIX_HOME_SERVER=https://matrix.example.com
MATRIX_USER_ID=@bot:example.com
MATRIX_ROOM_ID=!abc123:example.com
MATRIX_PASSWORD=your_password_here
# MATRIX_ACCESS_TOKEN=optional_token_instead_of_password
MATRIX_DEVICE_NAME=claude-matrix-bridge

# Access Control
ALLOWED_USERS=@you:example.com,@friend:example.com

# Claude Settings
CLAUDE_BINARY_PATH=claude
# CLAUDE_SDK_URL=http://localhost:8080

# Logging
RUST_LOG=info
```

**Step 3: Create .gitignore**

Create `.gitignore`:

```
/target
.env
crypto_store/
sessions_db/
*.swp
*.swo
*~
.DS_Store
```

**Step 4: Create README.md**

Create `README.md`:

```markdown
# Matrix-Claude Bridge

Rust bot that bridges Matrix room messages to Claude Code CLI.

## Setup

1. Copy `.env.example` to `.env` and configure
2. Build: `cargo build --release`
3. Run: `cargo run --release`

## Configuration

See `.env.example` for all options.

## First Run

The bot creates a new Matrix device on first login. You must verify this device from another Matrix client (Element, etc.) using emoji verification or cross-signing.
```

**Step 5: Verify project builds**

Run:
```bash
cargo check
```

Expected: Project compiles without errors

**Step 6: Commit project scaffolding**

```bash
git add Cargo.toml .env.example .gitignore README.md
git commit -m "chore: initialize rust project with dependencies"
```

---

## Task 2: Config Module (TDD)

**Files:**
- Create: `src/config.rs`
- Create: `tests/config_tests.rs`

**Step 1: Write failing test for config loading**

Create `tests/config_tests.rs`:

```rust
use std::collections::HashSet;

#[test]
fn test_config_loads_from_env() {
    std::env::set_var("MATRIX_HOME_SERVER", "https://test.com");
    std::env::set_var("MATRIX_USER_ID", "@bot:test.com");
    std::env::set_var("MATRIX_ROOM_ID", "!room:test.com");
    std::env::set_var("MATRIX_PASSWORD", "secret");
    std::env::set_var("ALLOWED_USERS", "@user1:test.com,@user2:test.com");

    let config = matrix_bridge::config::Config::from_env().unwrap();

    assert_eq!(config.matrix_home_server, "https://test.com");
    assert_eq!(config.matrix_user_id, "@bot:test.com");
    assert_eq!(config.matrix_room_id, "!room:test.com");
    assert_eq!(config.matrix_password, Some("secret".to_string()));
    assert_eq!(config.allowed_users.len(), 2);
    assert!(config.allowed_users.contains("@user1:test.com"));
}

#[test]
fn test_config_fails_on_missing_required_field() {
    std::env::remove_var("MATRIX_HOME_SERVER");
    std::env::remove_var("MATRIX_USER_ID");

    let result = matrix_bridge::config::Config::from_env();

    assert!(result.is_err());
}
```

**Step 2: Run test to verify it fails**

Run:
```bash
cargo test config_loads_from_env
```

Expected: FAIL with "no `config` module"

**Step 3: Write minimal config implementation**

Create `src/config.rs`:

```rust
use anyhow::{Context, Result};
use std::collections::HashSet;

#[derive(Debug, Clone)]
pub struct Config {
    pub matrix_home_server: String,
    pub matrix_user_id: String,
    pub matrix_room_id: String,
    pub matrix_password: Option<String>,
    pub matrix_access_token: Option<String>,
    pub matrix_device_name: String,
    pub allowed_users: HashSet<String>,
    pub claude_binary_path: String,
    pub claude_sdk_url: Option<String>,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let matrix_home_server = std::env::var("MATRIX_HOME_SERVER")
            .context("MATRIX_HOME_SERVER is required")?;
        let matrix_user_id = std::env::var("MATRIX_USER_ID")
            .context("MATRIX_USER_ID is required")?;
        let matrix_room_id = std::env::var("MATRIX_ROOM_ID")
            .context("MATRIX_ROOM_ID is required")?;
        let matrix_password = std::env::var("MATRIX_PASSWORD").ok();
        let matrix_access_token = std::env::var("MATRIX_ACCESS_TOKEN").ok();
        let matrix_device_name = std::env::var("MATRIX_DEVICE_NAME")
            .unwrap_or_else(|_| "claude-matrix-bridge".to_string());

        let allowed_users_str = std::env::var("ALLOWED_USERS")
            .context("ALLOWED_USERS is required")?;
        let allowed_users: HashSet<String> = allowed_users_str
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        let claude_binary_path = std::env::var("CLAUDE_BINARY_PATH")
            .unwrap_or_else(|_| "claude".to_string());
        let claude_sdk_url = std::env::var("CLAUDE_SDK_URL").ok();

        Ok(Config {
            matrix_home_server,
            matrix_user_id,
            matrix_room_id,
            matrix_password,
            matrix_access_token,
            matrix_device_name,
            allowed_users,
            claude_binary_path,
            claude_sdk_url,
        })
    }
}
```

Create `src/lib.rs`:

```rust
pub mod config;
```

**Step 4: Run test to verify it passes**

Run:
```bash
cargo test config_loads_from_env
```

Expected: PASS (both tests)

**Step 5: Commit config module**

```bash
git add src/lib.rs src/config.rs tests/config_tests.rs
git commit -m "feat(config): add environment variable parsing with validation"
```

---

## Task 3: Session Module (TDD)

**Files:**
- Create: `src/session.rs`
- Create: `tests/session_tests.rs`

**Step 1: Write failing test for session persistence**

Create `tests/session_tests.rs`:

```rust
use std::path::PathBuf;

#[test]
fn test_session_create_and_load() {
    let temp_dir = std::env::temp_dir().join("matrix-bridge-test-sessions");
    let _ = std::fs::remove_dir_all(&temp_dir);

    let store = matrix_bridge::session::SessionStore::new(&temp_dir).unwrap();
    let room_id = "!test:example.com";

    // First load creates new session
    let session1 = store.get_or_create(room_id).unwrap();
    assert!(!session1.started);

    // Mark as started and save
    store.mark_started(room_id).unwrap();

    // Second load returns same session, marked as started
    let session2 = store.get_or_create(room_id).unwrap();
    assert_eq!(session1.session_id, session2.session_id);
    assert!(session2.started);

    // Cleanup
    std::fs::remove_dir_all(&temp_dir).unwrap();
}

#[test]
fn test_session_cli_args_first_message() {
    let session = matrix_bridge::session::Session {
        session_id: "test-uuid".to_string(),
        started: false,
    };

    let args = session.cli_args();

    assert_eq!(args, vec!["--session-id", "test-uuid"]);
}

#[test]
fn test_session_cli_args_continuation() {
    let session = matrix_bridge::session::Session {
        session_id: "test-uuid".to_string(),
        started: true,
    };

    let args = session.cli_args();

    assert_eq!(args, vec!["--resume", "test-uuid"]);
}
```

**Step 2: Run test to verify it fails**

Run:
```bash
cargo test session_create_and_load
```

Expected: FAIL with "no `session` module"

**Step 3: Write minimal session implementation**

Create `src/session.rs`:

```rust
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub session_id: String,
    pub started: bool,
}

impl Session {
    pub fn cli_args(&self) -> Vec<&str> {
        if self.started {
            vec!["--resume", &self.session_id]
        } else {
            vec!["--session-id", &self.session_id]
        }
    }
}

pub struct SessionStore {
    db: sled::Db,
}

impl SessionStore {
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        let db = sled::open(path)?;
        Ok(SessionStore { db })
    }

    pub fn get_or_create(&self, room_id: &str) -> Result<Session> {
        if let Some(data) = self.db.get(room_id)? {
            let session: Session = serde_json::from_slice(&data)?;
            Ok(session)
        } else {
            let session = Session {
                session_id: uuid::Uuid::new_v4().to_string(),
                started: false,
            };
            self.save(room_id, &session)?;
            Ok(session)
        }
    }

    pub fn mark_started(&self, room_id: &str) -> Result<()> {
        let mut session = self.get_or_create(room_id)?;
        session.started = true;
        self.save(room_id, &session)?;
        Ok(())
    }

    fn save(&self, room_id: &str, session: &Session) -> Result<()> {
        let data = serde_json::to_vec(session)?;
        self.db.insert(room_id, data)?;
        Ok(())
    }
}
```

Update `Cargo.toml` to add uuid dependency:

```toml
uuid = { version = "1.6", features = ["v4"] }
```

Update `src/lib.rs`:

```rust
pub mod config;
pub mod session;
```

**Step 4: Run test to verify it passes**

Run:
```bash
cargo test session_
```

Expected: PASS (all 3 tests)

**Step 5: Commit session module**

```bash
git add src/lib.rs src/session.rs tests/session_tests.rs Cargo.toml
git commit -m "feat(session): add persistent session storage with sled"
```

---

## Task 4: Claude Module (TDD)

**Files:**
- Create: `src/claude.rs`
- Create: `tests/claude_tests.rs`

**Step 1: Write failing test for JSON parsing**

Create `tests/claude_tests.rs`:

```rust
#[test]
fn test_parse_claude_response_success() {
    let json = r#"{
        "content": [
            {"type": "text", "text": "Hello, "},
            {"type": "text", "text": "world!"}
        ]
    }"#;

    let result = matrix_bridge::claude::parse_response(json).unwrap();

    assert_eq!(result, "Hello, world!");
}

#[test]
fn test_parse_claude_response_empty() {
    let json = r#"{"content": []}"#;

    let result = matrix_bridge::claude::parse_response(json).unwrap();

    assert_eq!(result, "");
}

#[test]
fn test_parse_claude_response_malformed() {
    let json = "not valid json";

    let result = matrix_bridge::claude::parse_response(json);

    assert!(result.is_err());
}
```

**Step 2: Run test to verify it fails**

Run:
```bash
cargo test claude_
```

Expected: FAIL with "no `claude` module"

**Step 3: Write minimal claude implementation**

Create `src/claude.rs`:

```rust
use anyhow::{Context, Result};
use serde::Deserialize;
use tokio::process::Command;

#[derive(Debug, Deserialize)]
struct ClaudeResponse {
    content: Vec<ContentBlock>,
}

#[derive(Debug, Deserialize)]
struct ContentBlock {
    #[serde(rename = "type")]
    block_type: String,
    text: Option<String>,
}

pub fn parse_response(json: &str) -> Result<String> {
    let response: ClaudeResponse = serde_json::from_str(json)
        .context("Failed to parse Claude JSON response")?;

    let text = response
        .content
        .iter()
        .filter_map(|block| block.text.as_deref())
        .collect::<Vec<_>>()
        .join("");

    Ok(text)
}

pub async fn invoke_claude(
    binary_path: &str,
    sdk_url: Option<&str>,
    session_args: Vec<&str>,
    prompt: &str,
) -> Result<String> {
    let mut args = vec!["--print", "--output-format", "json"];
    args.extend(session_args);

    if let Some(url) = sdk_url {
        args.extend(["--sdk-url", url]);
    }

    args.push(prompt);

    tracing::debug!(?args, "Spawning Claude CLI");

    let output = Command::new(binary_path)
        .args(&args)
        .output()
        .await
        .context("Failed to spawn claude CLI")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Claude CLI failed with exit code {:?}: {}", output.status.code(), stderr);
    }

    let stdout = String::from_utf8(output.stdout)
        .context("Claude output is not valid UTF-8")?;

    parse_response(&stdout)
}
```

Update `src/lib.rs`:

```rust
pub mod claude;
pub mod config;
pub mod session;
```

**Step 4: Run test to verify it passes**

Run:
```bash
cargo test claude_
```

Expected: PASS (all 3 tests)

**Step 5: Commit claude module**

```bash
git add src/lib.rs src/claude.rs tests/claude_tests.rs
git commit -m "feat(claude): add CLI spawning and JSON response parsing"
```

---

## Task 5: Matrix Client Module

**Files:**
- Create: `src/matrix_client.rs`

**Step 1: Write matrix client initialization**

Create `src/matrix_client.rs`:

```rust
use anyhow::{Context, Result};
use matrix_sdk::{
    config::SyncSettings,
    ruma::OwnedUserId,
    Client, SqliteCryptoStore,
};
use std::path::Path;

pub async fn create_client(homeserver: &str, user_id: &str) -> Result<Client> {
    let user_id: OwnedUserId = user_id.parse()
        .context("Invalid Matrix user ID")?;

    let crypto_store = SqliteCryptoStore::open(Path::new("./crypto_store"), None)
        .await
        .context("Failed to open crypto store")?;

    let client = Client::builder()
        .homeserver_url(homeserver)
        .sqlite_crypto_store(crypto_store)
        .build()
        .await
        .context("Failed to create Matrix client")?;

    Ok(client)
}

pub async fn login(
    client: &Client,
    user_id: &str,
    password: Option<&str>,
    access_token: Option<&str>,
    device_name: &str,
) -> Result<()> {
    if let Some(token) = access_token {
        tracing::info!("Logging in with access token");
        let user_id: OwnedUserId = user_id.parse()?;
        client.restore_session(matrix_sdk::Session {
            access_token: token.to_string(),
            user_id,
            device_id: device_name.to_string().into(),
        }).await?;
    } else if let Some(pwd) = password {
        tracing::info!("Logging in with password");
        client
            .matrix_auth()
            .login_username(user_id, pwd)
            .device_id(device_name)
            .send()
            .await
            .context("Failed to log in")?;
    } else {
        anyhow::bail!("Either MATRIX_PASSWORD or MATRIX_ACCESS_TOKEN is required");
    }

    tracing::info!(user_id = %client.user_id().unwrap(), "Logged in successfully");

    Ok(())
}
```

Update `src/lib.rs`:

```rust
pub mod claude;
pub mod config;
pub mod matrix_client;
pub mod session;
```

**Step 2: Verify it compiles**

Run:
```bash
cargo check
```

Expected: Compiles without errors

**Step 3: Commit matrix client**

```bash
git add src/lib.rs src/matrix_client.rs
git commit -m "feat(matrix): add client initialization and login"
```

---

## Task 6: Message Handler Module

**Files:**
- Create: `src/message_handler.rs`

**Step 1: Write message handler**

Create `src/message_handler.rs`:

```rust
use anyhow::Result;
use matrix_sdk::{
    room::Room,
    ruma::{events::room::message::RoomMessageEventContent, OwnedUserId},
    Client,
};
use std::collections::HashSet;

use crate::{claude, config::Config, session::SessionStore};

pub async fn handle_message(
    room: Room,
    event: matrix_sdk::event_handler::EventHandlerData,
    client: Client,
    config: Config,
    session_store: SessionStore,
) -> Result<()> {
    let Room::Joined(room) = room else {
        return Ok(());
    };

    // Only process messages from configured room
    if room.room_id().as_str() != config.matrix_room_id {
        return Ok(());
    }

    let Some(message_event) = event.event().as_original() else {
        return Ok(());
    };

    let sender = message_event.sender.as_str();
    let body = &message_event.content.body;

    // Ignore bot's own messages
    if sender == client.user_id().unwrap().as_str() {
        return Ok(());
    }

    // Check whitelist
    if !config.allowed_users.contains(sender) {
        tracing::debug!(sender, "Ignoring message from unauthorized user");
        return Ok(());
    }

    tracing::info!(sender, room_id = %room.room_id(), message_preview = &body[..body.len().min(50)], "Processing message");

    // Load session
    let session = session_store.get_or_create(room.room_id().as_str())?;
    let session_args = session.cli_args();

    // Set typing indicator
    room.typing_notice(true).await?;

    // Invoke Claude
    let response = match claude::invoke_claude(
        &config.claude_binary_path,
        config.claude_sdk_url.as_deref(),
        session_args,
        body,
    ).await {
        Ok(resp) => {
            tracing::info!(response_length = resp.len(), "Claude responded");
            resp
        }
        Err(e) => {
            tracing::error!(error = %e, "Claude invocation failed");
            let error_msg = format!("⚠️ Claude error: {}", e);
            room.typing_notice(false).await?;
            room.send(RoomMessageEventContent::text_plain(&error_msg)).await?;
            return Ok(());
        }
    };

    // Clear typing indicator
    room.typing_notice(false).await?;

    // Send response
    room.send(RoomMessageEventContent::text_plain(&response)).await?;

    // Mark session as started
    session_store.mark_started(room.room_id().as_str())?;

    tracing::info!("Response sent successfully");

    Ok(())
}
```

Update `src/lib.rs`:

```rust
pub mod claude;
pub mod config;
pub mod matrix_client;
pub mod message_handler;
pub mod session;
```

**Step 2: Verify it compiles**

Run:
```bash
cargo check
```

Expected: Compiles without errors

**Step 3: Commit message handler**

```bash
git add src/lib.rs src/message_handler.rs
git commit -m "feat(handler): add message processing with auth and Claude invocation"
```

---

## Task 7: Main Binary

**Files:**
- Create: `src/main.rs`

**Step 1: Write main entry point**

Create `src/main.rs`:

```rust
use anyhow::Result;
use matrix_bridge::{config::Config, matrix_client, message_handler, session::SessionStore};
use matrix_sdk::{
    config::SyncSettings,
    ruma::events::room::message::SyncRoomMessageEvent,
};
use std::sync::Arc;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    tracing::info!("Starting Matrix-Claude Bridge");

    // Load configuration
    dotenvy::dotenv().ok();
    let config = Config::from_env()?;

    tracing::info!(
        homeserver = %config.matrix_home_server,
        user_id = %config.matrix_user_id,
        room_id = %config.matrix_room_id,
        allowed_users = config.allowed_users.len(),
        "Configuration loaded"
    );

    // Initialize session store
    let session_store = SessionStore::new("./sessions_db")?;
    tracing::info!("Session store initialized");

    // Create Matrix client
    let client = matrix_client::create_client(
        &config.matrix_home_server,
        &config.matrix_user_id,
    ).await?;

    // Login
    matrix_client::login(
        &client,
        &config.matrix_user_id,
        config.matrix_password.as_deref(),
        config.matrix_access_token.as_deref(),
        &config.matrix_device_name,
    ).await?;

    // Join room
    let room_id: matrix_sdk::ruma::OwnedRoomId = config.matrix_room_id.parse()?;
    client.join_room_by_id(&room_id).await?;
    tracing::info!(room_id = %config.matrix_room_id, "Joined room");

    // Register message handler
    let config_clone = Arc::new(config);
    let session_store_clone = Arc::new(session_store);

    client.add_event_handler(
        move |event: SyncRoomMessageEvent, room, client| {
            let config = Arc::clone(&config_clone);
            let session_store = Arc::clone(&session_store_clone);
            async move {
                if let Err(e) = message_handler::handle_message(
                    room,
                    event.into(),
                    client,
                    (*config).clone(),
                    (*session_store).clone(),
                ).await {
                    tracing::error!(error = %e, "Error handling message");
                }
            }
        },
    );

    tracing::info!("Message handler registered, starting sync loop");

    // Sync forever
    client.sync(SyncSettings::default()).await?;

    Ok(())
}
```

**Step 2: Fix SessionStore to be cloneable**

Update `src/session.rs` to add Clone:

```rust
#[derive(Clone)]
pub struct SessionStore {
    db: sled::Db,
}
```

**Step 3: Build the binary**

Run:
```bash
cargo build --release
```

Expected: Builds successfully

**Step 4: Commit main binary**

```bash
git add src/main.rs src/session.rs
git commit -m "feat(main): add main entry point with sync loop"
```

---

## Task 8: Integration Test (Manual)

**Files:**
- Create: `docs/testing.md`

**Step 1: Create testing documentation**

Create `docs/testing.md`:

```markdown
# Testing Guide

## Setup

1. Copy `.env.example` to `.env`
2. Configure with real Matrix credentials
3. Create a test room with E2E encryption enabled
4. Add your bot user to the room
5. Add your personal user ID to `ALLOWED_USERS`

## First Run

```bash
cargo run --release
```

Expected output:
```
INFO Starting Matrix-Claude Bridge
INFO Configuration loaded
INFO Session store initialized
INFO Logged in successfully
INFO Joined room
INFO Message handler registered, starting sync loop
```

## Device Verification

On first run, the bot creates a new device. You must verify it:

1. Open Element (or another Matrix client)
2. Go to Settings → Security → Verify this device
3. Complete emoji verification or cross-signing
4. Return to terminal and send a message

## Test Cases

### 1. Authorized User Message

Send: "Hello Claude!"

Expected:
- Bot sets typing indicator
- Claude CLI is invoked
- Response appears in room
- Logs show session ID created

### 2. Follow-up Message

Send: "What did I just say?"

Expected:
- Bot uses `--resume` with same session ID
- Claude has context from previous message
- Response references "Hello Claude!"

### 3. Unauthorized User

Add another user to room (not in whitelist)

Send message as that user

Expected:
- Bot ignores message silently
- Logs show "Ignoring message from unauthorized user"

### 4. Bot Restart Persistence

Stop bot (Ctrl+C), restart

Send message

Expected:
- Bot resumes with same session ID
- Context is preserved

### 5. Decryption Test

Send encrypted message from verified device

Expected:
- Bot decrypts and processes message normally

Send from unverified device

Expected:
- Bot logs decryption failure
- Sends error message to room
```

**Step 2: Commit testing docs**

```bash
git add docs/testing.md
git commit -m "docs: add integration testing guide"
```

---

## Task 9: Final Polish

**Files:**
- Modify: `README.md`
- Create: `.env.example` (enhance with comments)

**Step 1: Enhance README**

Update `README.md`:

```markdown
# Matrix-Claude Bridge

Rust bot that bridges Matrix room messages to Claude Code CLI with E2E encryption and persistent sessions.

## Features

- **E2E Encryption**: Full support for encrypted Matrix rooms
- **Whitelist Auth**: Only respond to approved users
- **Persistent Sessions**: Conversation context survives restarts
- **Structured Logging**: Trace message flow with tracing
- **Zero Prefix**: Responds to all messages (no `!command` needed)

## Setup

### 1. Prerequisites

- Rust 1.70+ (`rustup` recommended)
- Claude Code CLI installed and authenticated
- Matrix account for the bot
- Matrix room with E2E encryption enabled

### 2. Configuration

Copy `.env.example` to `.env` and configure:

```bash
cp .env.example .env
# Edit .env with your credentials
```

### 3. Build and Run

```bash
cargo build --release
cargo run --release
```

### 4. Device Verification (First Run Only)

The bot creates a new Matrix device on first login. Verify it from another client:

1. Open Element → Settings → Security
2. Find the new "claude-matrix-bridge" device
3. Verify using emoji verification or cross-signing

## Usage

Once running, send any message in the configured room (as a whitelisted user). The bot responds with Claude's output. Conversation context persists across messages and bot restarts.

## Troubleshooting

**Bot doesn't respond:**
- Check logs for "Ignoring message from unauthorized user"
- Verify your user ID is in `ALLOWED_USERS`
- Confirm bot joined the correct room

**Decryption failures:**
- Verify the bot's device from another client
- Check `crypto_store/` exists and has correct permissions

**Claude errors:**
- Verify `claude` binary is in PATH or set `CLAUDE_BINARY_PATH`
- Check Claude CLI is authenticated: `claude auth status`

## Architecture

- `src/config.rs` - Environment variable parsing
- `src/session.rs` - Persistent session storage (sled)
- `src/claude.rs` - CLI spawning and JSON parsing
- `src/matrix_client.rs` - Matrix login and crypto setup
- `src/message_handler.rs` - Auth checks and orchestration
- `src/main.rs` - Entry point and sync loop

## License

MIT
```

**Step 2: Commit final polish**

```bash
git add README.md
git commit -m "docs: enhance README with features and troubleshooting"
```

---

## Task 10: Final Verification

**Step 1: Clean build**

Run:
```bash
cargo clean
cargo build --release
```

Expected: Clean build succeeds

**Step 2: Run all tests**

Run:
```bash
cargo test
```

Expected: All tests pass

**Step 3: Check for warnings**

Run:
```bash
cargo clippy
```

Expected: No warnings (or only minor ones)

**Step 4: Tag release**

```bash
git tag -a v0.1.0 -m "Initial release: Matrix-Claude bridge with E2E encryption"
```

---

## Verification Checklist

Before declaring complete:

- [ ] `cargo build --release` succeeds
- [ ] `cargo test` all pass
- [ ] `cargo clippy` clean (or acceptable warnings)
- [ ] `.env.example` has all required variables documented
- [ ] README explains setup, usage, troubleshooting
- [ ] Manual test: bot responds to authorized user
- [ ] Manual test: bot ignores unauthorized user
- [ ] Manual test: session persists across restart
- [ ] Manual test: E2E encryption works

---

## Post-Implementation

After completing all tasks, perform manual integration testing per `docs/testing.md`. The bot should:

1. Start up and log in successfully
2. Join the configured room
3. Respond to whitelisted users only
4. Maintain session context across messages
5. Persist sessions across restarts
6. Handle encrypted messages after device verification
