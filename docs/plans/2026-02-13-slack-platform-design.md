# Slack Platform Support — Design Document

**Date:** 2026-02-13
**Status:** Draft
**Depends on:** [Telegram Platform Design](2026-02-13-telegram-platform-design.md) (shared Platform Registry architecture)

## Summary

Add Slack as a chat platform alongside Matrix and Telegram. Uses the Platform Registry from the Telegram design for multi-platform runtime. Slack gets the full native treatment: Block Kit formatting, threaded conversations, slash commands, channel management, and Socket Mode for real-time events.

## Decisions

| Decision | Choice | Rationale |
|---|---|---|
| Scope | Multi-platform runtime | Builds on Platform Registry from Telegram spec |
| Slack API | Bot (Socket Mode) | WebSocket-based, no public URL needed, matches self-hosted pattern |
| Rust crate | slack-morphism | Most complete Rust Slack SDK, async, supports Socket Mode natively |
| Feature scope | Full Slack-native | Block Kit, threads, slash commands, channel management |
| Threading | Thread in channels, flat in DMs | Keeps channels clean, DMs stay conversational |

## Config

```toml
[slack]
app_token = "xapp-1-..."        # Socket Mode token (starts with xapp-)
bot_token = "xoxb-..."          # Bot OAuth token
signing_secret = "abc123..."     # For request verification
allowed_users = ["U12345678"]    # Slack user IDs
allowed_channels = []            # Empty = allow all channels bot is in
thread_in_channels = true        # Thread replies in channels, flat in DMs
```

```rust
pub struct SlackConfig {
    pub app_token: String,
    pub bot_token: String,
    pub signing_secret: String,
    pub allowed_users: Vec<String>,
    pub allowed_channels: Vec<String>,
    pub thread_in_channels: bool,
}
```

Top-level `Config`:

```rust
pub struct Config {
    pub matrix: Option<MatrixConfig>,
    pub telegram: Option<TelegramConfig>,
    pub slack: Option<SlackConfig>,       // New
    pub backend: BackendConfig,
    pub webhook: WebhookConfig,
    pub workspace: WorkspaceConfig,
    pub scheduler: SchedulerConfig,
}
```

### Required Slack App OAuth Scopes

`chat:write`, `channels:read`, `groups:read`, `im:read`, `im:history`, `channels:history`, `groups:history`, `files:read`, `files:write`, `users:read`, `commands`, `app_mentions:read`. Socket Mode must be enabled in the app settings.

## Extension Traits

New optional capabilities in `gorp-core/src/traits.rs` that Slack implements and Matrix/Telegram return `None` for:

```rust
/// Platforms that support threaded conversations
pub trait ThreadedPlatform: Send + Sync {
    async fn send_threaded(
        &self,
        channel_id: &str,
        thread_ts: &str,
        content: MessageContent,
    ) -> Result<()>;
}

/// Platforms that support slash commands
pub trait SlashCommandProvider: Send + Sync {
    fn registered_commands(&self) -> Vec<SlashCommandDef>;
    async fn handle_command(&self, cmd: SlashCommandInvocation) -> Result<MessageContent>;
}

/// Platforms that can render rich formatted output
pub trait RichFormatter: Send + Sync {
    fn format_as_blocks(&self, content: &str) -> serde_json::Value;
}

pub struct SlashCommandDef {
    pub name: String,
    pub description: String,
}

pub struct SlashCommandInvocation {
    pub command: String,
    pub text: String,
    pub channel_id: String,
    pub user_id: String,
    pub response_url: String,
}
```

`IncomingMessage` gains a thread context field:

```rust
pub struct IncomingMessage {
    pub platform_id: String,
    pub channel_id: String,
    pub thread_id: Option<String>,  // Slack thread_ts, None for unthreaded
    pub sender: ChatUser,
    pub body: String,
    pub is_direct: bool,
    pub formatted: bool,
    pub attachment: Option<AttachmentInfo>,
    pub event_id: String,
    pub timestamp: i64,
}
```

`ChatPlatform` gains optional accessors:

```rust
pub trait ChatPlatform: MessagingPlatform {
    // ... existing methods ...
    fn threading(&self) -> Option<&dyn ThreadedPlatform> { None }
    fn slash_commands(&self) -> Option<&dyn SlashCommandProvider> { None }
    fn rich_formatter(&self) -> Option<&dyn RichFormatter> { None }
}
```

## SlackPlatform Implementation

```rust
// src/platform/slack/mod.rs
pub struct SlackPlatform {
    client: SlackClient,
    bot_token: SlackApiToken,
    app_token: SlackApiToken,
    bot_user_id: String,            // Resolved at startup via auth.test
    config: SlackConfig,
}
```

### MessagingPlatform Trait

- `platform_id()` → `"slack"`
- `bot_user_id()` → bot's Slack user ID (resolved at startup via `auth.test`)
- `event_stream()` → Socket Mode WebSocket connection. Maps `message`, `app_mention`, and `slash_command` events to `IncomingMessage`. Strips `@gorp` mention prefix from app_mention body.
- `send()` → `chat.postMessage`. Applies Block Kit formatting via `rich_formatter()`. File uploads via `files.uploadV2`. Chunks at ~3,900 chars per block section.

### ChatPlatform Trait

- `get_channel()` / `joined_channels()` → wrap Slack conversations as `SlackChannel`
- `channel_creator()` → `Some(self)` — `conversations.create` and `conversations.open`
- `channel_manager()` → `Some(self)` — invite, kick, list members
- `encryption()` → `None` — Slack handles encryption at transport layer
- `threading()` → `Some(self)`
- `slash_commands()` → `Some(self)`
- `rich_formatter()` → `Some(self)`

### ThreadedPlatform

```rust
impl ThreadedPlatform for SlackPlatform {
    async fn send_threaded(
        &self,
        channel_id: &str,
        thread_ts: &str,
        content: MessageContent,
    ) -> Result<()> {
        // chat.postMessage with thread_ts parameter
        // If content is long, chunk into multiple threaded replies
    }
}
```

### SlashCommandProvider

```rust
impl SlashCommandProvider for SlackPlatform {
    fn registered_commands(&self) -> Vec<SlashCommandDef> {
        vec![
            SlashCommandDef { name: "/gorp".into(), description: "Invoke Claude".into() },
            SlashCommandDef { name: "/gorp-status".into(), description: "Check workspace status".into() },
        ]
    }

    async fn handle_command(&self, cmd: SlashCommandInvocation) -> Result<MessageContent> {
        // Route to message handler as if it were a regular message
        // Use response_url for deferred responses
    }
}
```

### RichFormatter (Block Kit)

```rust
impl RichFormatter for SlackPlatform {
    fn format_as_blocks(&self, content: &str) -> serde_json::Value {
        // Parse markdown → Block Kit blocks
        // Fallback: single section block with mrkdwn text
        match parse_markdown_to_blocks(content) {
            Ok(blocks) => blocks,
            Err(_) => fallback_mrkdwn_block(content),
        }
    }
}
```

**Markdown → Block Kit mapping:**

| Markdown | Block Kit |
|---|---|
| Paragraphs | `section` block with `mrkdwn` text |
| `# Heading` | `header` block |
| `` ```code``` `` | `rich_text` block with `rich_text_preformatted` element |
| `- list items` | `rich_text` block with `rich_text_list` element |
| `**bold**`, `*italic*`, `` `inline` `` | Slack mrkdwn equivalents (`*bold*`, `_italic_`, `` `inline` ``) |
| Tables | `section` block with monospaced text (no Block Kit table primitive) |
| `> quotes` | `rich_text` block with `rich_text_quote` element |

**Block Kit limits:**
- Max 50 blocks per message
- Max 3,000 chars per `section` text field
- Max 4,000 chars per `rich_text_preformatted`

When limits are exceeded, split into multiple messages within the same thread. Long code blocks split at line boundaries. If formatting fails, fall back to raw mrkdwn.

## SlackChannel Implementation

```rust
// src/platform/slack/channel.rs
pub struct SlackChannel {
    channel_id: SlackChannelId,
    bot: SlackClient,
    bot_token: SlackApiToken,
    channel_type: SlackChannelType,
    thread_in_channels: bool,
}

pub enum SlackChannelType {
    Public,
    Private,
    DirectMessage,
    GroupDirectMessage,
}
```

### ChatChannel Trait

- `id()` → channel ID string (C-prefixed for channels, D-prefixed for DMs)
- `name()` → channel name, or user's display name for DMs
- `is_direct()` → `true` if `DirectMessage` or `GroupDirectMessage`
- `send()` → routing logic:
  - If `is_direct()` or `!thread_in_channels` → flat `chat.postMessage`
  - If channel + `thread_in_channels` → `send_threaded()` using stored `thread_ts`
  - Block Kit formatting applied before sending
- `member_count()` → `conversations.members` API
- `typing_indicator()` → `Some(self)` but returns `Ok(())` silently (Slack has no bot typing API)
- `attachment_handler()` → `Some(self)`

### AttachmentHandler

```rust
impl AttachmentHandler for SlackChannel {
    async fn download(&self, source_id: &str) -> Result<(String, Vec<u8>, String)> {
        // source_id = Slack file's url_private
        // GET with Authorization: Bearer bot_token
        // Return (filename, bytes, mime_type)
    }
}
```

### ChannelCreator

```rust
impl ChannelCreator for SlackPlatform {
    async fn create_channel(&self, name: &str) -> Result<String> {
        // conversations.create → returns channel ID
        // Slack channel names: lowercase, no spaces, max 80 chars
    }
    async fn create_dm(&self, user_id: &str) -> Result<String> {
        // conversations.open with user ID → returns DM channel ID
    }
}
```

### ChannelManager

```rust
impl ChannelManager for SlackPlatform {
    async fn join(&self, channel_id: &str) -> Result<()>;
    async fn leave(&self, channel_id: &str) -> Result<()>;
    async fn invite(&self, channel_id: &str, user_id: &str) -> Result<()>;
    async fn members(&self, channel_id: &str) -> Result<Vec<ChatUser>>;
}
```

## Socket Mode Event Flow

```
Slack Cloud
    │
    ├─→ WebSocket (Socket Mode)
    │       │
    │       ├─→ event: message / app_mention
    │       │       → Map to IncomingMessage { platform_id: "slack", thread_id: Some(thread_ts), ... }
    │       │       → Push to PlatformRegistry merged event stream
    │       │
    │       ├─→ event: slash_command (/gorp, /gorp-status)
    │       │       → ACK immediately (3-second Slack deadline)
    │       │       → Post ephemeral "Working on it..." via chat.postEphemeral
    │       │       → Create IncomingMessage with body = command text
    │       │       → Store response_url for deferred reply
    │       │       → Push to event stream
    │       │
    │       └─→ event: interactive (button clicks, modal submissions)
    │               → Future scope, not in initial implementation
    │
    └─→ Web API (HTTPS, outbound only)
            ├─→ chat.postMessage (responses)
            ├─→ chat.postEphemeral (slash command ACKs)
            ├─→ files.uploadV2 (attachments)
            ├─→ conversations.* (channel management)
            └─→ users.info (resolve display names)
```

### Slash Command Deferred Response Flow

1. Receive `/gorp summarize this channel` via Socket Mode
2. ACK the Socket Mode envelope immediately (Slack requires response within 3 seconds)
3. Post ephemeral "Working on it..." message via `chat.postEphemeral`
4. Route to message handler as a normal `IncomingMessage`
5. When Claude responds, post the real reply via `chat.postMessage` (threaded if in channel)

### Thread Continuity

When a user replies inside an existing Slack thread, the `thread_ts` comes through on the event. Stored on `IncomingMessage.thread_id`. The message handler uses the same `thread_ts` to keep the conversation in that thread, maintaining session continuity.

## Platform Comparison

| Concern | Matrix | Telegram | Slack |
|---|---|---|---|
| Auth | Homeserver login + device verify | Single bot token | App token + bot token + signing secret |
| Encryption | Olm/Megolm | None | Transport-level (Slack handles it) |
| Message limit | ~65K chars | 4,096 chars | 40K chars but 3K per block section |
| Media download | MXC URI, public-ish | `file_id` via Bot API | `url_private` with Bearer token |
| Channel creation | Yes | No (bot can't) | Yes |
| Threading | No native concept | No | Yes, core feature |
| Rich formatting | HTML | HTML subset | Block Kit JSON |
| Slash commands | No | Bot commands (simpler) | Full slash command framework |
| Typing indicator | Real API | Real API | No bot API (noop) |
| Update delivery | Sync loop | Long polling | Socket Mode (WebSocket) |
| Interactive components | Reactions only | Inline keyboards | Buttons, modals, menus |
| User IDs | `@user:server` | Numeric | `U`-prefixed strings |
| Channel IDs | `!room:server` | Numeric | `C`/`D`-prefixed strings |

## Files Modified

Additive to the Telegram spec's changes:

| File | Change |
|---|---|
| `gorp-core/src/config.rs` | Add `SlackConfig` |
| `gorp-core/src/traits.rs` | Add `thread_id` to `IncomingMessage`, add extension traits (`ThreadedPlatform`, `SlashCommandProvider`, `RichFormatter`) |
| `src/main.rs` | Register `SlackPlatform` if configured |

## Files Created

| File | Purpose |
|---|---|
| `src/platform/slack/mod.rs` | `SlackPlatform` implementation |
| `src/platform/slack/channel.rs` | `SlackChannel` implementation |
| `src/platform/slack/blocks.rs` | Markdown-to-Block Kit converter |
| `src/platform/slack/commands.rs` | Slash command registration and handling |

## Files Untouched

Same as Telegram spec — gorp-core (except config + traits), gorp-agent, gorp-ffi, scheduler, task executor, dispatch handler, webhook system, workspace/session management.

## Dependencies

Add to `Cargo.toml`:
```toml
slack-morphism = { version = "2.5", features = ["hyper"] }
```
