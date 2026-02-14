# Telegram Platform Support — Design Document

**Date:** 2026-02-13
**Status:** Draft
**Approach:** Platform Registry (multi-platform runtime)

## Summary

Add Telegram as a second chat platform alongside Matrix. Both platforms run simultaneously in a single gorp instance, sharing agent backends, scheduler, and workspace state. A new `PlatformRegistry` merges event streams from all configured platforms into a unified event loop.

## Decisions

| Decision | Choice | Rationale |
|---|---|---|
| Scope | Multi-platform runtime | Run Matrix and Telegram simultaneously, shared business logic |
| Telegram API | Bot API | Simpler, well-documented, sufficient for our use case |
| Update delivery | Long polling | No public URL required, matches self-hosted deployment |
| Rust crate | teloxide | Most popular, async, handles long polling natively |
| GUI | Matrix-only for now | GUI is secondary; Telegram works headless. Generalize later. |

## Architecture

### Platform Registry

New component in gorp-core that holds multiple platform instances and merges their event streams.

```rust
pub struct PlatformRegistry {
    platforms: HashMap<String, Box<dyn ChatPlatform>>,
}

impl PlatformRegistry {
    pub fn register(&mut self, platform: Box<dyn ChatPlatform>);
    pub fn get(&self, platform_id: &str) -> Option<&dyn ChatPlatform>;
    pub fn merged_event_stream(&self) -> EventStream;
}
```

At startup, `main.rs` initializes whichever platforms have config present, registers them, and consumes the merged stream:

```rust
let mut registry = PlatformRegistry::new();
if let Some(ref matrix_cfg) = config.matrix {
    registry.register(Box::new(MatrixPlatform::new(/* ... */)));
}
if let Some(ref tg_cfg) = config.telegram {
    registry.register(Box::new(TelegramPlatform::new(/* ... */)));
}

let mut events = registry.merged_event_stream();
while let Some(msg) = events.next().await {
    let platform = registry.get(&msg.platform_id).unwrap();
    handle_message(msg, platform, /* shared state */).await;
}
```

At least one platform must be configured or startup fails.

### Canonical IncomingMessage

Add `platform_id` and `thread_id` fields. This is the canonical definition — all platform specs reference this struct:

```rust
pub struct IncomingMessage {
    pub platform_id: String,              // "matrix", "telegram", "slack", "whatsapp"
    pub channel_id: String,
    pub thread_id: Option<String>,        // Slack thread_ts, WhatsApp quoted message ID, None for others
    pub sender: ChatUser,
    pub body: String,
    pub is_direct: bool,
    pub formatted: bool,
    pub attachment: Option<AttachmentInfo>,
    pub event_id: String,
    pub timestamp: i64,
}
```

### Canonical Shared Types

These types are defined in `gorp-core/src/traits.rs` and used by all platforms:

```rust
/// Metadata for an incoming file attachment
pub struct AttachmentInfo {
    pub source_id: String,     // Platform-specific ID (file_id, MXC URI, message key, url_private)
    pub filename: String,
    pub mime_type: String,
    pub size: Option<u64>,     // Bytes, if known before download
}

/// Type alias for the async stream of incoming messages from a platform
pub type EventStream = Pin<Box<dyn Stream<Item = IncomingMessage> + Send>>;

/// Connection state reported by each platform for health checks
pub enum PlatformConnectionState {
    Connected,
    Connecting,
    Disconnected { reason: String },
    AuthRequired,              // WhatsApp QR needed, Matrix device verify, etc.
    RateLimited { retry_after: Duration },
}
```

`ChatPlatform` exposes connection state for monitoring by the TUI, web admin, and health endpoints:

```rust
pub trait ChatPlatform: MessagingPlatform {
    // ... existing methods ...
    fn connection_state(&self) -> PlatformConnectionState;
}
```

## Config

```toml
# Matrix config becomes optional
[matrix]
home_server = "https://matrix.example.com"
user_id = "@bot:example.com"
password = "..."
allowed_users = ["@harper:example.com"]

# New Telegram section
[telegram]
bot_token = "123456:ABC-DEF..."
allowed_users = [12345678, 87654321]  # Numeric Telegram user IDs
allowed_chats = []                     # Empty = allow all chats bot is in
```

```rust
pub struct Config {
    pub matrix: Option<MatrixConfig>,      // Now optional
    pub telegram: Option<TelegramConfig>,  // New
    pub backend: BackendConfig,
    pub webhook: WebhookConfig,
    pub workspace: WorkspaceConfig,
    pub scheduler: SchedulerConfig,
}

pub struct TelegramConfig {
    pub bot_token: String,
    pub allowed_users: Vec<i64>,
    pub allowed_chats: Vec<i64>,
}
```

## Telegram Platform Implementation

### TelegramPlatform (Tier 2: ChatPlatform)

```rust
pub struct TelegramPlatform {
    bot: Bot,                    // teloxide Bot handle
    user_id: String,             // Bot's numeric ID as string
    config: TelegramConfig,
}
```

**MessagingPlatform trait:**
- `platform_id()` → `"telegram"`
- `bot_user_id()` → bot's numeric ID
- `event_stream()` → teloxide long polling, filtered and mapped to `IncomingMessage`
- `send()` → dispatch by `MessageContent` variant: plain text, HTML (ParseMode::Html), or file upload. Chunk messages at 4096 chars.

**ChatPlatform trait:**
- `get_channel()` / `joined_channels()` → wrap Telegram chats as `TelegramChannel`
- `channel_creator()` → `None` (Bot API cannot create groups)
- `channel_manager()` → `Some(self)` (can invite users, list members)
- `encryption()` → `None` (no E2E for bots)

### TelegramChannel (ChatChannel)

```rust
pub struct TelegramChannel {
    chat_id: ChatId,
    bot: Bot,
    chat_type: ChatType,  // Private, Group, Supergroup
}
```

**ChatChannel trait:**
- `id()` → chat ID as string
- `name()` → chat title, or user's display name for DMs
- `is_direct()` → `true` if `ChatType::Private`
- `send()` → delegate to bot API with 4096-char chunking
- `typing_indicator()` → `Some(self)` — sends `ChatAction::Typing`
- `attachment_handler()` → `Some(self)` — downloads via `bot.get_file(file_id)`

**Attachment handling:**
- Incoming: Telegram `file_id` stored as `AttachmentInfo.source_id`
- Download: `bot.get_file(file_id)` → download bytes
- Upload: `bot.send_document()` or `bot.send_photo()` depending on MIME type

### Key Differences from Matrix

| Concern | Matrix | Telegram |
|---|---|---|
| Auth | Homeserver login + device verification | Single bot token |
| Encryption | Olm/Megolm (complex setup) | None for bots |
| Message limit | ~65K chars | 4096 chars |
| Media | MXC URIs, MediaSource JSON | file_id strings |
| Channel creation | Bot can create rooms | Bot cannot create groups |
| User IDs | `@user:server` strings | Numeric IDs |
| Update delivery | Sync loop with pagination | Long polling |

## Message Handler Changes

Signature changes from concrete Matrix types to trait objects:

```rust
// Before
async fn handle_message(room: Room, event: SyncRoomMessageEvent, client: Client, ...)

// After
async fn handle_message(msg: IncomingMessage, platform: &dyn ChatPlatform, ...)
```

Specific changes:
- **Whitelist checking** — dispatch by `platform_id`, each platform config has its own allowed users list
- **Command parsing** — already text-based, no changes needed
- **Attachment downloads** — already trait-based via `channel.attachment_handler()`, no changes
- **Response routing** — already trait-based via `channel.send()`, no changes
- **Matrix-specific commands** (`!verify`, `!encrypt`) — gated behind `platform_id == "matrix"` or behind `EncryptedPlatform` trait (Telegram returns `None`)

## GUI

No GUI changes for Telegram in this phase. The GUI continues to consume Matrix events only.

Single change: handle `config.matrix == None` gracefully — show "No Matrix configured" instead of crashing.

Telegram GUI support can be added later by generalizing `MatrixEvent` to `PlatformEvent` with a `platform_id` discriminator.

## Files Modified

| File | Change |
|---|---|
| `gorp-core/src/config.rs` | Add `TelegramConfig`, make `MatrixConfig` optional |
| `gorp-core/src/traits.rs` | Add `platform_id` to `IncomingMessage` |
| `src/main.rs` | Platform registry init, unified event loop |
| `src/message_handler/mod.rs` | Signature change, platform-aware whitelist |
| `src/message_handler/matrix_commands.rs` | Gate behind platform check |
| `src/gui/sync.rs` | Handle missing Matrix config |

## Files Created

| File | Purpose |
|---|---|
| `src/platform/telegram/mod.rs` | `TelegramPlatform` implementation |
| `src/platform/telegram/channel.rs` | `TelegramChannel` implementation |
| `src/platform/registry.rs` | `PlatformRegistry` |

## Files Untouched

- **gorp-core** (except config + traits) — all other types, workspace logic
- **gorp-agent** — all agent backends (ACP, mux, direct)
- **gorp-ffi** — language bindings
- **Scheduler, task executor, dispatch handler** — platform-agnostic
- **Webhook system** — independent of chat platforms
- **Workspace/session management** — platform-agnostic

## Graceful Shutdown

All platforms follow the same shutdown protocol, coordinated by `PlatformRegistry`:

```rust
impl PlatformRegistry {
    pub async fn shutdown(&mut self) {
        // Shutdown all platforms concurrently with a 10s timeout
        let futures: Vec<_> = self.platforms.values_mut()
            .map(|p| p.shutdown())
            .collect();
        let _ = tokio::time::timeout(
            Duration::from_secs(10),
            futures::future::join_all(futures),
        ).await;
    }
}
```

Each platform's `MessagingPlatform` trait includes a shutdown method:

```rust
pub trait MessagingPlatform: Send + Sync {
    // ... existing methods ...
    async fn shutdown(&self) -> Result<()>;
}
```

For Telegram specifically: cancel the long polling task, drop the `Bot` handle. Teloxide handles cleanup internally.

The `gorp start` and `gorp tui` entry points listen for `SIGINT`/`SIGTERM` (via `tokio::signal`) and call `registry.shutdown()` before exiting. The GUI wires its close button to the same path.

## Health Check Model

`PlatformRegistry` aggregates health from all registered platforms:

```rust
impl PlatformRegistry {
    pub fn health(&self) -> Vec<PlatformHealth> {
        self.platforms.iter().map(|(id, p)| PlatformHealth {
            platform_id: id.clone(),
            state: p.connection_state(),
        }).collect()
    }
}
```

This feeds into:
- **TUI Dashboard** — platform connection indicators (green/yellow/red dots)
- **Web Admin Dashboard** — gateway status cards
- **`/admin/health` endpoint** — JSON health response for monitoring

## Logging & Observability

All platform modules use `tracing` (already a gorp dependency) with structured fields:

```rust
tracing::info!(platform = "telegram", chat_id = %chat_id, "Message received");
tracing::warn!(platform = "telegram", "Rate limited by Telegram API, retrying in {}s", delay);
tracing::error!(platform = "whatsapp", "Sidecar crashed, attempt {}/5", attempt);
```

Convention: every log line includes `platform = "<id>"` so the TUI Logs view and web admin can filter by platform. Workspace-scoped operations additionally include `workspace = "<name>"`.

The TUI Logs view subscribes to a `tracing` layer that sends formatted log events through the TUI event channel. The web admin's `/admin/health` endpoint exposes structured health data (not raw logs).

## Dependencies

Add to `Cargo.toml`:
```toml
teloxide = { version = "0.13", features = ["macros"] }
```
