# WhatsApp Platform Support (Baileys) — Design Document

**Date:** 2026-02-13
**Status:** Draft
**Depends on:** [Telegram Platform Design](2026-02-13-telegram-platform-design.md) (shared Platform Registry architecture)

## Summary

Add WhatsApp as a chat platform via Baileys, an unofficial WhatsApp Web multi-device client implemented in TypeScript. Because Baileys is JavaScript, gorp communicates with it via a sidecar Node.js process over stdin/stdout JSON-RPC. WhatsApp operates as a hybrid platform: DMs route to the DISPATCH control plane (Tier 1), while groups mapped in config route to workspaces (Tier 2). An anti-ban system simulates human-like behavior to avoid account suspension.

## Decisions

| Decision | Choice | Rationale |
|---|---|---|
| Scope | Multi-platform runtime | Builds on Platform Registry from Telegram/Slack specs |
| Bridge architecture | Sidecar process (JSON-RPC over stdin/stdout) | Clean separation, Baileys upgradeable independently, same pattern as ACP |
| QR auth | Terminal QR + admin panel | Belt and suspenders — works headless and via web |
| Ban safety | Maximum caution | Rate limiting, typing simulation, human-like delays, jitter |
| Chat scope | DMs + whitelisted groups | DMs for dispatch, groups mapped to workspaces |
| Platform tier | Hybrid (Tier 1 DMs, Tier 2 groups) | DMs → DISPATCH, groups → workspace routing |

## Config

```toml
[whatsapp]
sidecar_path = "./baileys-bridge"     # Path to Node.js sidecar
data_dir = "data/whatsapp"            # Auth state + logs
allowed_users = ["+15551234567"]      # Phone numbers (E.164 format)
allowed_groups = []                    # Group JIDs, mapped to workspaces below
node_binary = "node"                  # Override if needed

# Workspace mapping: group JID → workspace name
[whatsapp.group_workspaces]
"120363012345@g.us" = "research"
"120363067890@g.us" = "news"

# Anti-ban tuning
[whatsapp.safety]
min_typing_duration_ms = 2000         # Minimum "composing" indicator before sending
typing_chars_per_second = 30          # Simulate realistic typing speed
min_delay_between_messages_ms = 1500  # Minimum gap between consecutive sends
max_messages_per_minute = 8           # Hard cap per chat
max_messages_per_hour = 60            # Hard cap global
read_delay_ms = 1000                  # Delay before marking messages as read
jitter_percent = 30                   # Randomize all delays by +/- this %
```

```rust
pub struct WhatsAppConfig {
    pub sidecar_path: String,
    pub data_dir: String,
    pub allowed_users: Vec<String>,
    pub allowed_groups: Vec<String>,
    pub group_workspaces: HashMap<String, String>,
    pub node_binary: Option<String>,
    pub safety: WhatsAppSafetyConfig,
}

pub struct WhatsAppSafetyConfig {
    pub min_typing_duration_ms: u64,
    pub typing_chars_per_second: f32,
    pub min_delay_between_messages_ms: u64,
    pub max_messages_per_minute: u32,
    pub max_messages_per_hour: u32,
    pub read_delay_ms: u64,
    pub jitter_percent: u32,
}
```

Top-level `Config`:

```rust
pub struct Config {
    pub matrix: Option<MatrixConfig>,
    pub telegram: Option<TelegramConfig>,
    pub slack: Option<SlackConfig>,
    pub whatsapp: Option<WhatsAppConfig>,    // New
    pub backend: BackendConfig,
    pub webhook: WebhookConfig,
    pub workspace: WorkspaceConfig,
    pub scheduler: SchedulerConfig,
}
```

## Sidecar Architecture & Protocol

The Baileys sidecar is a Node.js process that gorp spawns and communicates with over stdin/stdout JSON-RPC.

```
gorp (Rust)                              baileys-bridge (Node.js)
    │                                           │
    ├─── spawn ─────────────────────────────────→│
    │                                           │
    │◄── {"event":"qr","data":"..."}            │  ← QR code for pairing
    │◄── {"event":"connected","data":{...}}     │  ← Auth success
    │◄── {"event":"message","data":{...}}       │  ← Incoming message
    │                                           │
    ├──→ {"method":"send","params":{...}}       │  → Send a message
    │◄── {"result":"ok","id":"msg123"}          │
    │                                           │
    ├──→ {"method":"download_media","params":{}}│  → Download attachment
    │◄── {"result":{"data":"base64..."},"id":1} │
    │                                           │
    ├──→ {"method":"get_group_info","params":{}} │  → Group metadata
    │◄── {"result":{...},"id":2}                │
    │                                           │
    │◄── {"event":"disconnected","data":{...}}  │  ← Connection lost
    │◄── {"event":"auth_failure","data":{...}}  │  ← Session expired
    │                                           │
```

**Protocol:**
- **Events** (sidecar → gorp): `{"event": "<type>", "data": {...}}`
- **Commands** (gorp → sidecar): `{"method": "<name>", "params": {...}, "id": <int>}`
- **Responses** (sidecar → gorp): `{"result": {...}, "id": <int>}` or `{"error": {...}, "id": <int>}`
- One JSON object per line, newline-delimited

### Event Types

| Event | Data | When |
|---|---|---|
| `qr` | QR string (terminal-renderable) | Pairing needed |
| `connected` | `{jid, name, phone}` | Auth succeeded |
| `disconnected` | `{reason}` | Connection lost |
| `auth_failure` | `{reason}` | Session expired, re-pair needed |
| `message` | `{from, chat, body, timestamp, media?, quoted?}` | Incoming message |
| `message_sent` | `{id, chat}` | Delivery confirmation |
| `presence` | `{jid, status}` | Contact online/offline |

### Command Methods

| Method | Params | Purpose |
|---|---|---|
| `send` | `{chat, body, quoted?}` | Send text message |
| `send_media` | `{chat, data_b64, mime, filename, caption?}` | Send file/image |
| `download_media` | `{message_key}` | Download media from message |
| `get_group_info` | `{jid}` | Group metadata + participants |
| `set_presence` | `{status}` | composing, available, unavailable |
| `read` | `{chat, message_ids}` | Mark messages as read |
| `shutdown` | `{}` | Graceful disconnect |

### Session Persistence

Baileys stores auth state (keys, session data) to a configurable directory. On restart, if a valid session exists, it reconnects without QR. If the session is expired, it emits a `qr` event.

```
data/whatsapp/
  ├── auth_info/           # Baileys multi-device auth state
  │   ├── creds.json
  │   └── app-state-sync-*.json
  └── bridge.log           # Sidecar logs
```

## Sidecar Handle & Lifecycle

```rust
pub struct SidecarHandle {
    child: Child,
    stdin: BufWriter<ChildStdin>,
    events_rx: mpsc::Receiver<SidecarEvent>,
    pending_commands: HashMap<u64, oneshot::Sender<serde_json::Value>>,
    next_id: AtomicU64,
}

impl SidecarHandle {
    pub async fn spawn(config: &WhatsAppConfig) -> Result<Self> {
        // 1. Check node_binary exists
        // 2. npm install if node_modules missing
        // 3. Spawn: node baileys-bridge/src/index.ts --data-dir <path>
        // 4. Spawn reader task: read stdout lines, parse JSON
        //    - Events → push to events_rx
        //    - Responses → resolve matching pending_command oneshot
        // 5. Wait for "connected" or "qr" event
    }

    pub async fn send_command(&self, method: &str, params: serde_json::Value) -> Result<serde_json::Value> {
        // Write JSON-RPC command to stdin
        // Create oneshot channel, store in pending_commands
        // Await response with timeout
    }

    pub async fn shutdown(&mut self) -> Result<()> {
        // Send {"method": "shutdown"} → graceful disconnect
        // Wait 5s, then kill child if still running
    }
}
```

### Reconnection Strategy

WhatsApp connections drop periodically. The sidecar handles reconnection internally (Baileys has built-in retry). If the sidecar process itself crashes:

1. Gorp detects child process exit
2. Log the error, wait 10 seconds (backoff)
3. Respawn the sidecar
4. If valid auth state exists on disk, reconnects automatically
5. If auth expired, emit `qr` event again → show in admin panel
6. After 5 consecutive crash-respawns within 10 minutes, stop retrying and alert via admin panel

### QR Code Flow

```
Sidecar emits: {"event": "qr", "data": "2@ABC123..."}
    │
    ├──→ gorp renders QR in terminal (via qr2term or similar)
    │     "Scan this QR code with WhatsApp on your phone"
    │
    └──→ gorp exposes QR at GET /admin/whatsapp/pair
          Admin panel shows QR image, auto-refreshes every 20s
          (Baileys regenerates QR periodically until scanned)

Phone scans QR
    │
    └──→ Sidecar emits: {"event": "connected", "data": {"jid": "15551234567@s.whatsapp.net"}}
         gorp logs "WhatsApp connected as +15551234567"
         Auth state persisted to data_dir
```

## Hybrid Platform Tier Routing

WhatsApp is both Tier 1 and Tier 2, determined by chat context:

```
User Z messages Bot Y directly (DM)
    → Tier 1: MessagingPlatform
    → Routes to DISPATCH control plane
    → "schedule the news task" / "what workspaces do I have?"

User Z messages in Group 1 (containing Z + Y)
    → Tier 2: ChatPlatform
    → Group JID looked up in group_workspaces config
    → Routes to mapped workspace ("research")
    → Full Claude session in that workspace context
```

The user creates groups manually on WhatsApp (the bot can't create them programmatically). Once a group exists and its JID is added to `group_workspaces` config, it behaves like a Tier 2 channel.

### WhatsAppPlatform Implementation

```rust
pub struct WhatsAppPlatform {
    sidecar: Arc<SidecarHandle>,
    bot_jid: String,
    config: WhatsAppConfig,
    rate_limiter: Arc<Mutex<RateLimiter>>,
}

impl MessagingPlatform for WhatsAppPlatform {
    fn platform_id(&self) -> &'static str { "whatsapp" }
    fn bot_user_id(&self) -> &str { &self.bot_jid }

    async fn event_stream(&self) -> Result<EventStream> {
        // Sidecar events → IncomingMessage
        // Set is_direct based on JID suffix: @s.whatsapp.net vs @g.us
    }

    async fn send(&self, channel_id: &str, content: MessageContent) -> Result<()> {
        // Anti-ban pipeline: read delay → typing → rate check → send
    }
}

impl ChatPlatform for WhatsAppPlatform {
    type Channel = WhatsAppChannel;

    async fn get_channel(&self, id: &str) -> Option<Self::Channel> {
        // Return channel for any known chat (DM or group)
    }

    async fn joined_channels(&self) -> Vec<Self::Channel> {
        // Only return groups in group_workspaces config
        // DMs are handled at Tier 1, not listed as "channels"
    }

    fn channel_creator(&self) -> None;      // User creates groups manually
    fn channel_manager(&self) -> None;      // No programmatic group management
    fn encryption(&self) -> None;           // Signal protocol handled by Baileys
}
```

### Message Handler Routing

```rust
async fn handle_message(msg: IncomingMessage, platform: &dyn ChatPlatform, ...) {
    if msg.platform_id == "whatsapp" {
        if msg.is_direct {
            // Tier 1: route to DISPATCH
            dispatch_handler::handle(msg, ...).await;
        } else if let Some(workspace) = config.whatsapp.group_workspaces.get(&msg.channel_id) {
            // Tier 2: route to workspace
            workspace_handler::handle(msg, workspace, ...).await;
        } else {
            // Unknown group, ignore
            return;
        }
    }
    // ... existing platform routing
}
```

## WhatsAppChannel Implementation

```rust
pub struct WhatsAppChannel {
    jid: String,
    chat_type: WhatsAppChatType,
    sidecar: Arc<SidecarHandle>,
    rate_limiter: Arc<Mutex<RateLimiter>>,
    safety: WhatsAppSafetyConfig,
}

pub enum WhatsAppChatType {
    Direct,     // @s.whatsapp.net
    Group,      // @g.us
}
```

### ChatChannel Trait

- `id()` → JID string
- `name()` → contact name or group subject (via sidecar)
- `is_direct()` → `true` if `@s.whatsapp.net`
- `send()` → full anti-ban pipeline, then sidecar command
- `member_count()` → sidecar `get_group_info` → participant count
- `typing_indicator()` → `Some(self)` — managed by anti-ban system, manual calls pass through to `set_presence("composing")`
- `attachment_handler()` → `Some(self)`

### AttachmentHandler

```rust
impl AttachmentHandler for WhatsAppChannel {
    async fn download(&self, source_id: &str) -> Result<(String, Vec<u8>, String)> {
        // source_id = opaque message key JSON from sidecar
        // sidecar.send_command("download_media", {"message_key": source_id})
        // Sidecar calls downloadMediaMessage() → decrypts → returns base64
        // Return (filename, decoded_bytes, mime_type)
    }
}
```

## Media Handling

WhatsApp encrypts media independently from the message — each attachment gets its own encryption key stored in the message metadata. Baileys handles decryption internally, but the data moves through the sidecar.

### Incoming Media Flow

```
WhatsApp sends image message
    │
    Sidecar receives message event with media key + URL
    │
    Sidecar emits event:
    {"event": "message", "data": {
        "from": "15551234567@s.whatsapp.net",
        "chat": "15551234567@s.whatsapp.net",
        "body": "",
        "media": {
            "key": "<opaque_message_key_json>",
            "mime": "image/jpeg",
            "filename": "photo.jpg",
            "size": 245000
        }
    }}
    │
    gorp maps to IncomingMessage with AttachmentInfo {
        source_id: "<opaque_message_key_json>",
        filename: "photo.jpg",
        mime_type: "image/jpeg",
        size: Some(245000),
    }
    │
    When message handler needs bytes:
    sidecar.send_command("download_media", {"message_key": "<opaque_message_key_json>"})
    → Sidecar decrypts → returns base64 → gorp decodes
```

### Outgoing Media Flow

```
MessageContent::Attachment { filename, data, mime_type, caption }
    │
    Anti-ban delay pipeline
    │
    sidecar.send_command("send_media", {
        "chat": "15551234567@s.whatsapp.net",
        "data_b64": base64::encode(&data),
        "mime": "image/jpeg",
        "filename": "photo.jpg",
        "caption": "Here's the analysis"
    })
    │
    Sidecar calls sendMessage() with media → Baileys encrypts + uploads
```

Media size limits: 16MB images, 64MB video, 100MB documents. Base64 encoding over stdin/stdout adds ~33% overhead. For large files, the sidecar could write to a temp file and pass the path instead, but base64 keeps the protocol simple and stateless for now.

## Rate Limiter & Anti-Ban System

```rust
pub struct RateLimiter {
    per_chat: HashMap<String, ChatRateState>,
    global: GlobalRateState,
    safety: WhatsAppSafetyConfig,
}

struct ChatRateState {
    last_send: Instant,
    sends_this_minute: u32,
    minute_window_start: Instant,
}

struct GlobalRateState {
    sends_this_hour: u32,
    hour_window_start: Instant,
}

impl RateLimiter {
    /// Returns how long to wait before sending, or None if clear to send
    pub fn check(&self, chat_jid: &str) -> Option<Duration>;
    pub fn record_send(&mut self, chat_jid: &str);
    fn apply_jitter(&self, duration: Duration) -> Duration;
}
```

### Full Send Pipeline

When Claude produces a 300-character response:

1. Check rate limiter — wait if over per-minute or per-hour cap
2. Wait `read_delay_ms` (± jitter) before marking user's message as read
3. Send `set_presence("composing")` to show typing indicator
4. Hold typing for `max(min_typing_duration_ms, 300 / typing_chars_per_second)` ≈ 10 seconds (± jitter)
5. Wait `min_delay_between_messages_ms` (± jitter) since last send
6. Send the message
7. Send `set_presence("available")`
8. Record send for rate limiting

For multi-chunk messages (split at ~2,000 chars to avoid suspicion), repeat steps 3-7 for each chunk with delays between.

A 300-character response takes ~10 seconds. A 2,000-character response takes ~67 seconds. This is the tradeoff for not getting banned.

## Sidecar Package Structure

```
baileys-bridge/
    ├── package.json
    ├── package-lock.json
    ├── tsconfig.json
    └── src/
        ├── index.ts          # Entry point, stdin/stdout JSON-RPC loop
        ├── connection.ts     # Baileys socket creation, reconnection logic
        ├── auth.ts           # Multi-device auth state read/write
        ├── events.ts         # Map Baileys events → JSON-RPC events
        └── commands.ts       # Handle incoming JSON-RPC commands
```

Node.js dependencies:
```json
{
  "dependencies": {
    "@whiskeysockets/baileys": "^6",
    "qrcode-terminal": "^0.12"
  }
}
```

## Platform Comparison

| Concern | Matrix | Telegram | Slack | WhatsApp |
|---|---|---|---|---|
| Auth | Homeserver login + device verify | Single bot token | App token + bot token + signing secret | QR code scan, session persists to disk |
| Language | Rust native (matrix-sdk) | Rust native (teloxide) | Rust native (slack-morphism) | Node.js sidecar (Baileys) via JSON-RPC |
| Account type | Bot or user | Bot | Bot | Full user account |
| Encryption | Olm/Megolm | None | Transport-level | Signal protocol (Baileys handles it) |
| Message limit | ~65K chars | 4,096 chars | 40K / 3K per block | ~65K but keep under 2K for safety |
| Media download | MXC URI | `file_id` via Bot API | `url_private` + Bearer token | Encrypted, decrypted by sidecar |
| Channel creation | Yes | No | Yes | No (user creates groups manually) |
| Threading | No | No | Yes, core feature | Quote-reply only |
| Rich formatting | HTML | HTML subset | Block Kit JSON | WhatsApp markdown (bold, italic, mono, strike) |
| Typing indicator | Real API | Real API | No bot API (noop) | Real, managed by anti-ban system |
| Update delivery | Sync loop | Long polling | Socket Mode (WebSocket) | WebSocket via sidecar |
| Ban risk | None | Low | None | High — requires anti-ban pipeline |
| Platform tier | Tier 2 | Tier 2 | Tier 2 | Hybrid: DM=Tier 1, Group=Tier 2 |
| User IDs | `@user:server` | Numeric | `U`-prefixed | Phone JID (`15551234567@s.whatsapp.net`) |
| Channel IDs | `!room:server` | Numeric | `C`/`D`-prefixed | Group JID (`120363012345@g.us`) |

## Files Modified

Additive to the Telegram + Slack specs:

| File | Change |
|---|---|
| `gorp-core/src/config.rs` | Add `WhatsAppConfig`, `WhatsAppSafetyConfig` |
| `src/main.rs` | Spawn sidecar, register `WhatsAppPlatform`, handle QR event → admin panel |
| `src/message_handler/mod.rs` | Add WhatsApp hybrid routing (DM → dispatch, group → workspace lookup) |
| `src/admin/routes.rs` | Add `/admin/whatsapp/pair` endpoint for QR display |
| `src/admin/templates.rs` | Add WhatsApp pairing template |

## Files Created

| File | Purpose |
|---|---|
| `src/platform/whatsapp/mod.rs` | `WhatsAppPlatform` implementation |
| `src/platform/whatsapp/channel.rs` | `WhatsAppChannel` implementation |
| `src/platform/whatsapp/sidecar.rs` | `SidecarHandle` — spawn, JSON-RPC, lifecycle |
| `src/platform/whatsapp/rate_limiter.rs` | Anti-ban rate limiting + jitter |
| `baileys-bridge/package.json` | Node.js sidecar package |
| `baileys-bridge/src/index.ts` | Sidecar entry point, JSON-RPC loop |
| `baileys-bridge/src/connection.ts` | Baileys socket, reconnection |
| `baileys-bridge/src/auth.ts` | Multi-device auth state |
| `baileys-bridge/src/events.ts` | Baileys events → JSON-RPC events |
| `baileys-bridge/src/commands.ts` | JSON-RPC command handlers |
| `templates/admin/whatsapp/pair.html` | QR code pairing page |

## Dependencies

Rust side — no new crate dependencies. Communication is raw JSON over stdin/stdout using `serde_json` (already a dependency).

## Files Untouched

Same as all other specs — gorp-core (except config), gorp-agent, gorp-ffi, scheduler, task executor, dispatch handler core logic, webhook system, workspace/session management.
