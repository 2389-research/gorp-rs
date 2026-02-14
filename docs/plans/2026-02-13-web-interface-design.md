# Web Interface Expansion — Design Document

**Date:** 2026-02-13
**Status:** Draft
**Depends on:** [Telegram Platform Design](2026-02-13-telegram-platform-design.md) (Platform Registry), [TUI Design](2026-02-13-tui-design.md) (shared concepts: Feed, Workspace interaction)

## Summary

Expand the existing admin panel into a full web application. Adds interactive Claude chat with WebSocket streaming, a cross-platform message feed, gateway (platform) connection management, workspace selection, and a first-run setup wizard with username/password auth. Built on the existing HTMX + askama + Tailwind stack, adding a single WebSocket endpoint for real-time features.

## Decisions

| Decision | Choice | Rationale |
|---|---|---|
| Relationship to admin | Expand existing admin panel | Incremental evolution, reuses existing stack and routes |
| Frontend | HTMX + askama + Tailwind (existing) | No JS build step, no SPA complexity, already in place |
| Real-time | WebSocket + HTMX | WebSocket for streaming/live updates, HTMX for page nav and forms |
| Chat support | Full chat + monitoring | Interactive Claude chat via WebSocket, not just monitoring |
| Auth | Token/password only | Simple, platform-agnostic, works regardless of which gateways are configured |

## First-Run Setup Wizard

On first launch when no auth config exists, all web requests redirect to `/setup`. No access to anything else until setup completes.

### Step 1: Create Admin Account

```
┌─────────────────────────────────────────────────┐
│  gorp setup                            step 1/3 │
│                                                  │
│  Welcome to gorp! Let's get you set up.         │
│                                                  │
│  Create your admin account:                      │
│                                                  │
│  Username:  [harper____________]                 │
│  Password:  [••••••••••_______]                  │
│  Confirm:   [••••••••••_______]                  │
│                                                  │
│                            [Next →]              │
└─────────────────────────────────────────────────┘
```

### Step 2: API Token

```
┌─────────────────────────────────────────────────┐
│  gorp setup                            step 2/3 │
│                                                  │
│  Your API token (save this somewhere safe):      │
│                                                  │
│  ┌────────────────────────────────────────────┐  │
│  │ gorp_tk_a1b2c3d4e5f6...                   │  │
│  └────────────────────────────────────────────┘  │
│  [Copy to clipboard]                             │
│                                                  │
│  Use this token for API access:                  │
│  X-API-Key: gorp_tk_a1b2c3d4e5f6...            │
│                                                  │
│                            [Next →]              │
└─────────────────────────────────────────────────┘
```

### Step 3: Connect Platforms (Optional)

```
┌─────────────────────────────────────────────────┐
│  gorp setup                            step 3/3 │
│                                                  │
│  Connect your first platform (optional):         │
│                                                  │
│  [ ] Matrix     [Configure...]                   │
│  [ ] Telegram   [Configure...]                   │
│  [ ] Slack      [Configure...]                   │
│  [ ] WhatsApp   [Configure...]                   │
│                                                  │
│  You can always add platforms later from          │
│  Settings → Gateways.                            │
│                                                  │
│              [Skip]        [Finish →]            │
└─────────────────────────────────────────────────┘
```

### Auth Implementation

```rust
// src/admin/auth.rs
pub struct AuthConfig {
    pub username: String,
    pub password_hash: String,       // argon2 hashed
    pub api_token: String,           // gorp_tk_<random hex>
    pub setup_complete: bool,
}
```

- Auth config stored in `data/auth.toml` (not in main config — separate concern)
- On startup, if `data/auth.toml` doesn't exist → `setup_complete = false`
- Setup wizard middleware intercepts ALL requests and redirects to `/setup` until complete
- Password hashed with argon2 before storing
- API token generated as `gorp_tk_` + 32 random hex chars

**Auth flow after setup:**

```
Browser request → check session cookie
    ├── valid session → proceed
    └── no session → redirect to /login
        ├── username/password match → set session cookie, redirect to /admin
        └── wrong → show error

API request → check X-API-Key header
    ├── matches api_token → proceed
    └── no match → 401
```

## Gateway Management

A "Gateways" section for configuring, connecting, disconnecting, and monitoring platform connections.

### Overview Page

```
┌─────────────────────────────────────────────────────────┐
│  Gateways                                               │
│                                                         │
│  ┌─ Matrix ──────────────────────── ● Connected ──────┐ │
│  │  Homeserver: matrix.example.com                     │ │
│  │  User: @gorp:example.com                            │ │
│  │  Rooms: 12  │  Encrypted: yes                       │ │
│  │  [Disconnect]  [Edit]                               │ │
│  └─────────────────────────────────────────────────────┘ │
│                                                         │
│  ┌─ Telegram ────────────────────── ● Connected ──────┐ │
│  │  Bot: @gorp_bot (ID: 123456789)                     │ │
│  │  Chats: 5  │  Mode: long polling                    │ │
│  │  [Disconnect]  [Edit]                               │ │
│  └─────────────────────────────────────────────────────┘ │
│                                                         │
│  ┌─ Slack ───────────────────────── ○ Not configured ─┐ │
│  │  [Configure...]                                     │ │
│  └─────────────────────────────────────────────────────┘ │
│                                                         │
│  ┌─ WhatsApp ────────────────────── ○ Not configured ─┐ │
│  │  [Configure...]                                     │ │
│  └─────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────┘
```

### Platform Config Modals

Each platform gets a config form specific to its needs. Example for Telegram:

```
┌─ Configure Telegram ────────────────────────────┐
│                                                  │
│  Bot Token:  [123456:ABC-DEF..._________]       │
│                                                  │
│  Allowed Users (Telegram IDs, one per line):     │
│  ┌──────────────────────────────────────┐        │
│  │ 12345678                             │        │
│  │ 87654321                             │        │
│  └──────────────────────────────────────┘        │
│                                                  │
│  [ ] Allow all chats bot is in                   │
│                                                  │
│         [Cancel]        [Save & Connect]         │
└──────────────────────────────────────────────────┘
```

### WhatsApp QR Pairing

WhatsApp gets special treatment for the QR code auth flow:

```
┌─ Configure WhatsApp ────────────────────────────┐
│                                                  │
│  Status: Waiting for QR scan...                  │
│                                                  │
│  ┌──────────────┐                                │
│  │  ██ ▄▄ ██ ▄  │  Scan this QR code with       │
│  │  ▄▄ ██ ▄▄ █  │  WhatsApp on your phone:      │
│  │  ██ ▄▄ ██ ▄  │                                │
│  │  ▄▄ ██ ▄▄ █  │  1. Open WhatsApp              │
│  │  ██ ▄▄ ██ ▄  │  2. Tap Linked Devices         │
│  └──────────────┘  3. Scan this code             │
│                                                  │
│  Auto-refreshes every 20s                        │
│                                                  │
│  Allowed Users (phone numbers):                  │
│  ┌──────────────────────────────────────┐        │
│  │ +15551234567                         │        │
│  └──────────────────────────────────────┘        │
│                                                  │
│  Group → Workspace Mapping:                      │
│  ┌──────────────────────────────────────┐        │
│  │ 120363012345@g.us → research         │ [x]    │
│  │ 120363067890@g.us → news             │ [x]    │
│  └──────────────────────────────────────┘        │
│  [+ Add mapping]                                 │
│                                                  │
│         [Cancel]        [Save]                   │
└──────────────────────────────────────────────────┘
```

### Gateway Routes

| Endpoint | Method | Purpose |
|---|---|---|
| `/admin/gateways` | GET | Gateway overview page |
| `/admin/gateways/:platform` | GET | Platform detail/config form |
| `/admin/gateways/:platform/save` | POST | Save platform config |
| `/admin/gateways/:platform/connect` | POST | Connect to platform |
| `/admin/gateways/:platform/disconnect` | POST | Disconnect from platform |
| `/admin/gateways/whatsapp/qr` | GET | WhatsApp QR code (HTMX polling) |

### Hot-Reload

Gateway configs write to the main `config.toml`. When you save & connect, gorp hot-reloads the platform config and registers it with the PlatformRegistry at runtime — no restart required.

```rust
impl PlatformRegistry {
    pub async fn register(&mut self, platform: Box<dyn ChatPlatform>);
    pub async fn unregister(&mut self, platform_id: &str);  // For disconnect
    // merged_event_stream automatically picks up changes
}
```

## Navigation Structure

The current 8-link flat nav reorganizes into logical groups:

```
MONITOR
  Dashboard         /admin
  Feed              /admin/feed
  Messages          /admin/messages
  Logs              /admin/health

INTERACT
  Chat              /admin/chat
  Workspaces        /admin/workspaces

MANAGE
  Gateways          /admin/gateways
  Schedules         /admin/schedules
  Channels          /admin/channels

SYSTEM
  Config            /admin/config
  Browse            /admin/browse
  Health            /admin/health
```

## WebSocket Protocol

One WebSocket connection at `/admin/ws` handles all real-time features. The connection persists across HTMX page navigations (nav is inside the page content, base layout with WebSocket stays).

### Client → Server

```json
{"type": "subscribe", "channels": ["feed", "status"]}
{"type": "unsubscribe", "channels": ["feed"]}
{"type": "chat.send", "workspace": "research", "body": "What papers came in?"}
{"type": "chat.cancel", "workspace": "research"}
{"type": "chat.select_workspace", "workspace": "research"}
```

### Server → Client

```json
{"type": "feed.message", "html": "<div class=\"feed-msg\" ...>...</div>", "data": {"platform": "matrix", "channel_id": "!abc:matrix.org"}}
{"type": "status.platform", "data": {"platform": "telegram", "state": "connected"}}
{"type": "status.platform", "data": {"platform": "whatsapp", "state": "qr_needed"}}
{"type": "chat.chunk", "data": {"workspace": "research", "text": "I found"}}
{"type": "chat.chunk", "data": {"workspace": "research", "text": " 3 new papers"}}
{"type": "chat.tool_use", "data": {"workspace": "research", "tool": "Read", "input": "arxiv_feed.json"}}
{"type": "chat.complete", "data": {"workspace": "research", "usage": {"input": 1200, "output": 450}}}
{"type": "chat.error", "data": {"workspace": "research", "error": "Backend timeout"}}
```

### Subscription Model

Different pages subscribe to different channels:

| Page | Subscriptions |
|---|---|
| Dashboard | `status` |
| Feed | `feed`, `status` |
| Chat | `chat`, `status` |
| Gateways | `status` |
| Everything else | None (pure HTMX) |

### Client-Side JavaScript

Minimal — one file, ~80 lines:

```javascript
// static/ws.js
class GorpSocket {
    constructor() {
        this.ws = null;
        this.subscriptions = new Set();
        this.handlers = {};
        this.connect();
    }

    connect() {
        this.ws = new WebSocket(`ws://${location.host}/admin/ws`);
        this.ws.onmessage = (e) => this.dispatch(JSON.parse(e.data));
        this.ws.onclose = () => setTimeout(() => this.connect(), 2000);
    }

    subscribe(channels) { /* send subscribe message, track in set */ }
    unsubscribe(channels) { /* send unsubscribe message */ }
    send(msg) { this.ws.send(JSON.stringify(msg)); }

    dispatch(msg) {
        // Route to registered handlers
        // Handlers update DOM directly
    }
}

window.gorp = new GorpSocket();
```

Pages register handlers via small inline `<script>` blocks in askama templates. When HTMX swaps page content, old handlers are gone and the new page registers its own.

### Server-Side WebSocket

```rust
// src/admin/websocket.rs
pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<AdminState>,
    session: Session,
) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_ws(socket, state))
}

async fn handle_ws(socket: WebSocket, state: AdminState) {
    let (mut sink, mut stream) = socket.split();
    let (tx, mut rx) = mpsc::channel::<WsMessage>(64);

    // Reader task: client messages → parse, handle subscriptions, route chat
    // Writer task: rx → serialize → send to client
    // Feed bridge: PlatformRegistry events → tx (if subscribed)
    // Status bridge: platform status changes → tx (if subscribed)
    // Chat bridge: AgentHandle events → tx (if subscribed)
}
```

## Chat Page

Interactive Claude chat via WebSocket streaming. Web equivalent of the TUI Workspace view.

```
┌─────────────────────────────────────────────────────────┐
│  Chat                              research ▾    [acp]  │
│─────────────────────────────────────────────────────────│
│                                                         │
│  ┌─ you ──────────────────────────── 14:02 ───────────┐ │
│  │ What papers came in this week?                      │ │
│  └─────────────────────────────────────────────────────┘ │
│                                                         │
│  ┌─ claude ───────────────────────── 14:02 ───────────┐ │
│  │ I found 3 new papers in the arxiv feed:             │ │
│  │                                                     │ │
│  │ 1. "Scaling Laws for..." - Chen et al.              │ │
│  │ 2. "Attention Is All You..." - Wu et al.            │ │
│  │ 3. "On the Geometry of..." - Park et al.            │ │
│  │                                                     │ │
│  │ Shall I summarize any of these?                     │ │
│  └─────────────────────────────────────────────────────┘ │
│                                                         │
│  ┌─ claude ──────────────────── streaming... ─────────┐ │
│  │ The paper examines scaling behavior across▌         │ │
│  │                                                     │ │
│  │ 🔧 Read arxiv_cache/2401.12345.json                │ │
│  └─────────────────────────────────────────────────────┘ │
│                                                         │
│─────────────────────────────────────────────────────────│
│  ┌─────────────────────────────────────────┐  [Send]   │
│  │ _                                       │  Ctrl+Enter│
│  └─────────────────────────────────────────┘           │
└─────────────────────────────────────────────────────────┘
```

**Features:**
- Workspace selector dropdown — switches workspace, loads conversation history
- Messages rendered as chat bubbles with markdown (server-rendered via askama)
- Claude responses stream via WebSocket `chat.chunk` messages — JS appends text to active bubble
- Tool use shown inline with tool name
- Token usage displayed on completion
- `Ctrl+Enter` or Send button to submit (textarea allows multiline)
- Cancel button appears during streaming — sends `chat.cancel`
- File upload for attachments (multipart POST to `/admin/chat/:workspace/upload`)

**Conversation history:** On workspace selection, HTMX GET to `/admin/chat/:workspace` returns server-rendered history. WebSocket handles real-time streaming for new messages. History is server-rendered HTML; live interaction is WebSocket.

### Chat Routes

| Endpoint | Method | Purpose |
|---|---|---|
| `/admin/chat` | GET | Chat page with workspace selector |
| `/admin/chat/:workspace` | GET | HTMX partial — conversation history |
| `/admin/chat/:workspace/upload` | POST | File upload, returns attachment reference |

## Feed Page

Cross-platform message stream, same concept as TUI Feed but rendered as HTML with WebSocket updates.

```
┌─────────────────────────────────────────────────────────┐
│  Feed                    [all ▾] [matrix ▾] [telegram ▾]│
│─────────────────────────────────────────────────────────│
│                                                         │
│  ● matrix  @harper → #research              2m ago      │
│  Can you summarize the latest paper?                    │
│                                                         │
│  ● telegram  Harper → DM                    5m ago      │
│  Schedule the news digest for 6pm                       │
│                                                         │
│  ● matrix  gorp-bot → #research             2m ago      │
│  Here's a summary of the paper...                       │
│                                                         │
│  ● whatsapp  +1555... → Group:news          8m ago      │
│  What's trending today?                                 │
│                                                         │
│─────────────────────────────────────────────────────────│
│  Showing 142 messages today  │  Auto-scroll: ON         │
└─────────────────────────────────────────────────────────┘
```

**Implementation:**
- Initial load: HTMX GET returns last 50 messages, server-rendered
- WebSocket subscription to `feed` channel pushes new messages
- Server sends pre-rendered HTML fragments over WebSocket — JS just calls `insertAdjacentHTML`
- Platform filter via `data-platform` attribute, client-side show/hide
- Click message → navigate to Chat view for that channel
- Auto-scroll sticks to bottom unless user has scrolled up

**Feed WebSocket messages include pre-rendered HTML:**

```json
{
    "type": "feed.message",
    "html": "<div class=\"feed-msg\" data-platform=\"matrix\">...</div>",
    "data": {"platform": "matrix", "channel_id": "!abc:matrix.org"}
}
```

## Full URL Structure

```
/setup                        # First-run wizard (redirects here if no auth config)
/login                        # Login page

/admin                        # Dashboard (existing)
/admin/feed                   # Cross-platform feed (NEW)
/admin/chat                   # Workspace chat (NEW)
/admin/chat/:workspace        # Chat in specific workspace (NEW)
/admin/chat/:workspace/upload # File upload (NEW)
/admin/workspaces             # Workspace list (NEW)
/admin/gateways               # Gateway management (NEW)
/admin/gateways/:platform     # Platform config (NEW)
/admin/gateways/:platform/save       # Save config (NEW)
/admin/gateways/:platform/connect    # Connect (NEW)
/admin/gateways/:platform/disconnect # Disconnect (NEW)
/admin/gateways/whatsapp/qr  # WhatsApp QR (NEW)
/admin/ws                     # WebSocket endpoint (NEW)

/admin/channels               # (existing)
/admin/channels/:name         # (existing)
/admin/channels/:name/logs    # (existing)
/admin/messages               # (existing)
/admin/schedules              # (existing)
/admin/schedules/new          # (existing)
/admin/config                 # (existing)
/admin/browse                 # (existing)
/admin/search                 # (existing)
/admin/health                 # (existing)
```

## Files Modified

| File | Change |
|---|---|
| `src/admin/mod.rs` | Add new modules (websocket, setup, gateways) |
| `src/admin/routes.rs` | Add new routes (feed, chat, workspaces, gateways) |
| `src/admin/auth.rs` | Replace API key auth with username/password + token + session cookies + setup wizard middleware |
| `src/admin/templates.rs` | Add template structs for new pages |
| `src/webhook.rs` | Mount setup/login routes outside auth middleware, add WebSocket endpoint |
| `templates/base.html` | Expanded navigation (Monitor/Interact/Manage/System groups), WebSocket script include |

## Files Created

| File | Purpose |
|---|---|
| `src/admin/websocket.rs` | WebSocket handler, subscription management, bridge to platform events + agent |
| `src/admin/setup.rs` | First-run wizard routes and logic |
| `src/admin/gateways.rs` | Gateway CRUD routes, platform hot-reload |
| `templates/setup/step1.html` | Username/password form |
| `templates/setup/step2.html` | API token display |
| `templates/setup/step3.html` | Platform quick-connect |
| `templates/admin/feed.html` | Cross-platform feed page |
| `templates/admin/chat.html` | Workspace chat page |
| `templates/admin/chat/history.html` | HTMX partial — conversation history |
| `templates/admin/chat/message.html` | HTMX partial — single message bubble |
| `templates/admin/workspaces.html` | Workspace list/management |
| `templates/admin/gateways/overview.html` | Gateway overview |
| `templates/admin/gateways/matrix.html` | Matrix config form |
| `templates/admin/gateways/telegram.html` | Telegram config form |
| `templates/admin/gateways/slack.html` | Slack config form |
| `templates/admin/gateways/whatsapp.html` | WhatsApp config + QR pairing |
| `templates/login.html` | Login page |
| `static/ws.js` | WebSocket client (~80 lines) |
| `static/chat.js` | Chat page interactions (~60 lines) |
| `data/auth.toml` | Auth config (generated by setup wizard, not checked in) |

## Files Untouched

Everything outside the admin module — gorp-core, gorp-agent, gorp-ffi, all platform implementations, TUI, GUI, message handler, scheduler, webhooks. The web expansion is contained within `src/admin/` and `templates/`.

## Dependencies

No new Rust crate dependencies. axum already supports WebSocket, askama and HTMX are already in use. One addition for password hashing:

```toml
argon2 = { version = "0.5", optional = true }  # Behind admin feature
```
