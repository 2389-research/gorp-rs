# Matrix-Claude Bridge

Rust bot that bridges Matrix room messages to Claude Code CLI with E2E encryption and persistent sessions.

## Features

- **E2E Encryption**: Full support for encrypted Matrix rooms
- **Whitelist Auth**: Only respond to approved users
- **Persistent Sessions**: Conversation context survives restarts
- **Structured Logging**: Trace message flow with tracing
- **Zero Prefix**: Responds to all messages (no `!command` needed)

## Prerequisites

- Rust 1.70+ (`rustup` recommended)
- Claude Code CLI installed and authenticated
- Matrix account for the bot
- Matrix room with E2E encryption enabled

## Setup

### 1. Configure

Copy `.env.example` to `.env` and edit with your credentials:

```bash
cp .env.example .env
# Edit .env with your Matrix and Claude settings
```

### 2. Build and Run

```bash
cargo build --release
cargo run --release
```

### 3. Verify Device (First Run Only)

The bot creates a new Matrix device on first login. Verify it from another client:

1. Open Element → Settings → Security
2. Find the new "claude-matrix-bridge" device
3. Verify using emoji verification or cross-signing

## Usage

Send any message in the configured room (as a whitelisted user). The bot responds with Claude's output. Conversation context persists across messages and bot restarts.

## Configuration

See `.env.example` for all available options:

- **MATRIX_HOME_SERVER**: Your Matrix homeserver URL
- **MATRIX_USER_ID**: Bot's Matrix user ID
- **MATRIX_ROOM_ID**: Room ID to monitor
- **MATRIX_PASSWORD**: Bot password (or use MATRIX_ACCESS_TOKEN)
- **ALLOWED_USERS**: Comma-separated list of authorized user IDs
- **CLAUDE_BINARY_PATH**: Path to claude binary (defaults to `claude`)

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
