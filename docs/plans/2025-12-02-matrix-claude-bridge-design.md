# Matrix-Claude Bridge Design

**Date:** 2025-12-02
**Status:** Approved
**Language:** Rust

## Overview

This Rust Matrix bot bridges messages from one encrypted room to Claude Code's CLI and maintains conversation context across sessions.

## Core Requirements

- Listen to one specific Matrix room
- Respond only to whitelisted users (silently ignore unauthorized users)
- Respond to every message from authorized users (no command prefix required)
- Enable E2E encryption
- Spawn `claude` CLI for each message
- Persist session state per room
- Log structured events

## Architecture

### Dependencies

- **`matrix-sdk`** (with `e2e-encryption` feature) - Matrix protocol + E2E crypto
- **`tokio`** - Async runtime
- **`tracing`** + **`tracing-subscriber`** - Structured logging
- **`dotenvy`** - Environment variable loading
- **`serde`** + **`serde_json`** - Parse Claude CLI JSON output
- **`sled`** - Embedded database for session persistence

### Main Loop

1. Load configuration from `.env`
2. Log in to Matrix (password or access token)
3. Initialize E2E encryption with `SqliteCryptoStore`
4. Join configured room
5. Listen to sync stream
6. For each message:
   - Verify room matches `MATRIX_ROOM_ID`
   - Verify sender appears in `ALLOWED_USERS` whitelist
   - Skip bot's own messages
   - Load session from sled (create new UUID if missing)
   - Set typing indicator
   - Spawn `claude` CLI with session args
   - Parse JSON response
   - Send response to Matrix
   - Save session state

## Configuration

Environment variables (Rust-idiomatic naming):

### Matrix Settings
- `MATRIX_HOME_SERVER` - Homeserver URL (required)
- `MATRIX_USER_ID` - Bot's user ID (required)
- `MATRIX_ROOM_ID` - Target room ID (required)
- `MATRIX_PASSWORD` - Login password (optional if using token)
- `MATRIX_ACCESS_TOKEN` - Alternative to password
- `MATRIX_DEVICE_NAME` - Device name (default: "claude-matrix-bridge")

### Access Control
- `ALLOWED_USERS` - Comma-separated authorized user IDs (required)

### Claude Settings
- `CLAUDE_BINARY_PATH` - Path to `claude` CLI (default: "claude")
- `CLAUDE_SDK_URL` - Optional SDK server URL

### Logging
- `RUST_LOG` - Log filter (e.g., `info`, `debug`)

## Message Handling Flow

1. **Room filter** - Ignore messages from other rooms
2. **Authorization check** - Verify sender in `ALLOWED_USERS` HashSet
3. **Self-message filter** - Ignore bot's own messages
4. **Session lookup** - Load from sled or create new UUID
5. **Typing indicator** - Signal bot is working
6. **Spawn Claude CLI**:
   - First message: `--session-id <uuid>`
   - Follow-ups: `--resume <uuid>`
7. **Parse response** - Extract text from JSON content blocks
8. **Send to Matrix** - Post as `m.room.message`
9. **Clear typing** - Stop typing indicator
10. **Update session** - Mark as started, save to sled

## E2E Encryption

- Enable `e2e-encryption` feature in Cargo.toml
- Create `SqliteCryptoStore` at `./crypto_store/`
- First run requires manual device verification (Element, etc.)
- Matrix SDK handles encryption/decryption automatically
- Crypto state persists across restarts
- Log decryption failures (usually means unverified device)

**Recovery:** Delete `./crypto_store/` and re-verify device if corrupted

## Error Handling

### Error Types

1. **Configuration errors** - Missing/invalid env vars → log and exit
2. **Matrix login failures** - Bad credentials → log and exit
3. **Matrix sync failures** - Network issues → log, retry with exponential backoff (max 60s)
4. **E2E decryption failures** - Missing keys → log warning, send error to room
5. **Claude CLI failures** - Non-zero exit → log stderr, send error to room
6. **Message send failures** - Can't post response → log only (can't notify user)

### User-Facing Error Messages

- Decryption failure: "⚠️ Cannot decrypt message (verify device first)"
- Claude error: "⚠️ Claude error: <brief description>"

### Logging Strategy

**Spans for request tracing:**
```rust
tracing::info_span!("handle_message", room_id = %room.room_id(), sender = %event.sender())
```

**Log levels:**
- `ERROR` - Fatal startup issues
- `WARN` - Recoverable issues (decryption, Claude errors)
- `INFO` - Key events (messages, responses, login)
- `DEBUG` - Detailed flow (sessions, CLI args, parsing)
- `TRACE` - Matrix SDK internals

## Project Structure

```
matrix-productivity/
├── Cargo.toml
├── .env.example
├── .gitignore
├── src/
│   ├── main.rs               # Entry point, setup
│   ├── config.rs             # Env parsing, validation
│   ├── matrix_client.rs      # Login, sync loop
│   ├── message_handler.rs    # Auth checks, processing
│   ├── claude.rs             # CLI spawning, JSON parsing
│   └── session.rs            # Session state, sled DB
├── crypto_store/             # E2E crypto state (gitignored)
├── sessions_db/              # Session persistence (gitignored)
└── docs/
    └── examples/
        └── matrix-bridge/    # Python reference
```

## Testing Strategy

### Unit Tests
- `config.rs` - Env parsing, validation, error cases
- `claude.rs` - JSON parsing with various outputs
- `session.rs` - CRUD operations, persistence

### Integration Tests (Manual)
- Set up test Matrix room with E2E
- Add test user to whitelist
- Verify Claude responses appear
- Verify unauthorized users get silently ignored
- Verify session persistence across restarts

**Real dependencies:** Tests use actual sled, Matrix SDK, and Claude CLI

## Success Criteria

- Bot successfully logs in and joins room
- E2E encryption works (can decrypt/send encrypted messages)
- Only whitelisted users get responses
- Unauthorized users are silently ignored
- Claude sessions persist across messages
- Bot restarts recover session state from disk
- Errors are logged and reported to users appropriately
- All structured logs contain relevant context
