# Matrix-Claude Bridge

Rust bot that creates dedicated Claude Code channels via Matrix, each backed by persistent workspace directories and webhook automation support.

## Features

- **Channel-Based Architecture**: Each channel = dedicated Matrix room + Claude session + workspace directory
- **Smart Session Persistence**: Channels automatically reuse the same Claude conversation every time
- **Webhook Integration**: External triggers (cron, CI/CD) can inject prompts into channels via HTTP
- **E2E Encryption**: Full support for encrypted Matrix rooms
- **Whitelist Auth**: Only respond to approved users
- **Workspace Management**: Organized directory structure for all channels

## Prerequisites

- Rust 1.70+ (`rustup` recommended)
- Claude Code CLI installed and authenticated
- Matrix account for the bot
- Matrix room with E2E encryption enabled

## Setup

### 1. Configure

Create a `config.toml` file in the project root:

```bash
cp config.toml.example config.toml
# Edit config.toml with your Matrix credentials
```

Example `config.toml`:

```toml
[matrix]
home_server = "https://matrix.org"
user_id = "@yourbot:matrix.org"
password = "your-password"  # or use access_token
device_name = "claude-matrix-bridge"
allowed_users = ["@you:matrix.org"]

[claude]
binary_path = "claude"

[webhook]
port = 13000

[workspace]
path = "./workspace"
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

### Creating a Channel

1. DM the bot: `!create PA` (creates a "PA" channel)
2. Bot creates:
   - `workspace/PA/` directory
   - Matrix room "Claude: PA"
   - Persistent Claude session
3. Join the room and start chatting!

### Channel Template (Optional)

Populate `workspace/template/` with files you want in every new channel:

```bash
workspace/template/
├── CLAUDE.md              # Project-specific instructions
├── .mcp-servers.json      # MCP server configs
└── .gitignore             # Channel gitignore
```

When you create a new channel, all template contents are automatically copied to the new channel's directory. Perfect for:
- Standardized CLAUDE.md instructions
- Pre-configured MCP servers
- Project boilerplate files
- Shared tooling configs

### Channel Commands

**DM Commands (Orchestrator):**
- `!create <name>` - Create a new channel
- `!list` - Show all your channels
- `!help` - Show help

**Room Commands:**
- `!status` - Show channel info (includes webhook URL and session ID)
- `!help` - Show help

### Webhook Integration

Each channel has a webhook for automation:

```bash
# Example: Daily news at 5am
0 5 * * * curl -X POST http://localhost:13000/webhook/session/<session-id> \
  -H "Content-Type: application/json" \
  -d '{"prompt": "send me today'\''s tech news"}'
```

Get your session ID with `!status` in the channel room.

## Configuration

Configuration is loaded from `config.toml` with optional environment variable overrides:

**Matrix Settings:**
- `matrix.home_server` (required) - Your Matrix homeserver URL
- `matrix.user_id` (required) - Bot's Matrix user ID
- `matrix.password` - Bot password (or use `access_token`)
- `matrix.access_token` - Bot access token (alternative to password)
- `matrix.device_name` - Device name (default: "claude-matrix-bridge")
- `matrix.allowed_users` - Array of authorized user IDs

**Claude Settings:**
- `claude.binary_path` - Path to claude binary (default: "claude")
- `claude.sdk_url` - Optional custom Claude SDK URL

**Webhook Settings:**
- `webhook.port` - HTTP server port (default: 13000)

**Workspace Settings:**
- `workspace.path` - Directory for channel workspaces (default: "./workspace")

Environment variables override config file values:
- `MATRIX_HOME_SERVER`, `MATRIX_USER_ID`, `MATRIX_PASSWORD`, etc.

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

```
workspace/
├── sessions.db          # SQLite: channel_name ↔ room_id ↔ session_id ↔ directory
├── PA/                  # Channel workspace directories
├── dev-help/
└── news-bot/
```

**Code Structure:**
- `src/config.rs` - TOML config loading with env var overrides
- `src/session.rs` - SQLite-backed channel management
- `src/webhook.rs` - HTTP server for external triggers
- `src/claude.rs` - CLI spawning and JSON parsing
- `src/matrix_client.rs` - Matrix login and crypto setup
- `src/message_handler.rs` - Auth checks, channel commands, orchestration
- `src/main.rs` - Entry point, sync loop, webhook server spawn

## License

MIT
