# gorp Desktop GUI Design

**Date:** 2026-01-07
**Status:** Approved
**Author:** Claude + Doctor Biz

## Overview

Transform gorp from a CLI-only Matrix-Claude bridge into a native macOS desktop application with an embedded server. The app runs primarily as a menu bar/tray application, with a dashboard interface for monitoring and a full chat view when needed.

## Goals

1. **Personal agent computer** - Both client and control plane in one app
2. **Always running** - Menu bar presence, quiet background operation
3. **Self-contained** - Embedded Matrix bridge, no external server needed
4. **Pure Rust** - iced GUI framework, no UniFFI/Swift complexity
5. **Installable** - Proper .app bundle, DMG, Homebrew cask distribution

## Non-Goals

- Remote access / web UI for mobile (keep existing web admin, but not expanding it)
- Linux/Windows GUI (focus on macOS first)
- Replacing the CLI (power users keep their terminal workflow)

---

## Architecture

### Binary Modes

Single `gorp` binary supports multiple modes:

```
gorp                    # No args: launches GUI (default)
gorp start              # Headless server mode (existing behavior)
gorp --headless         # Alias for headless, daemon-friendly
gorp config/schedule/...# CLI subcommands (existing)
```

### Entry Point Logic

```rust
fn main() {
    let cli = Cli::parse();

    match cli.command {
        None => run_gui(),              // No subcommand = GUI
        Some(Commands::Start) => run_headless(),
        Some(Commands::Headless) => run_headless(),
        Some(other) => run_cli_command(other),
    }
}
```

### GUI Launch Flow

1. Parse config (same `Config::load()` as today)
2. Initialize gorp server components (Matrix client, scheduler, warm manager)
3. Launch iced application with server state shared via `Arc<ServerState>`
4. Server runs in background tokio tasks
5. Menu bar icon appears, app is "running"

### Headless Mode

Unchanged from current `run_start()` - same behavior, no GUI initialization. For launchd/systemd deployments.

---

## Application State

```rust
struct GorpApp {
    // Server components (shared with background tasks)
    server: Arc<ServerState>,

    // UI state
    view: View,
    sidebar_collapsed: bool,

    // Tray/menu bar
    tray_visible: bool,
    window_visible: bool,
}

struct ServerState {
    config: Config,
    matrix_client: Client,
    session_store: SessionStore,
    scheduler_store: SchedulerStore,
    warm_manager: SharedWarmSessionManager,
    web_admin_handle: Option<JoinHandle<()>>,  // For enable/disable
}

enum View {
    Dashboard,
    Chat { room_id: String },
    Settings,
    Schedules,
    Logs,
}
```

---

## UI Design

### View Hierarchy

```
┌─────────────────────────────────────────────────┐
│ Menu Bar Icon (always present when running)     │
└─────────────────────────────────────────────────┘
                    │
                    ▼ click
┌─────────────────────────────────────────────────┐
│ Main Window                                     │
│ ┌───────────┬─────────────────────────────────┐ │
│ │ Sidebar   │ Content Area                    │ │
│ │           │                                 │ │
│ │ Dashboard │  (varies by View)               │ │
│ │ Rooms     │                                 │ │
│ │ Schedules │                                 │ │
│ │ Logs      │                                 │ │
│ │ Settings  │                                 │ │
│ └───────────┴─────────────────────────────────┘ │
└─────────────────────────────────────────────────┘
```

### Menu Bar / Tray

Default interaction surface - app lives here most of the time.

```
┌──────────────────────────────────────┐
│  ● Connected to matrix.org           │
│  ─────────────────────────────────── │
│  3 active sessions                   │
│  1 scheduled task (in 2h)            │
│  ─────────────────────────────────── │
│  Open Dashboard          ⌘D          │
│  Quick Prompt...         ⌘N          │
│  ─────────────────────────────────── │
│  Rooms                    ▶          │  → submenu
│  ─────────────────────────────────── │
│  Settings...             ⌘,          │
│  Quit gorp               ⌘Q          │
└──────────────────────────────────────┘
```

**Icon states:**
- ◉ Green dot = connected, healthy
- ◐ Yellow = syncing/reconnecting
- ○ Empty = disconnected/error

**Behaviors:**
- Left/right click: Opens menu
- Double click: Opens dashboard window

### Dashboard View (Home)

Primary view when window is open:

- Connection status (Matrix sync state)
- Active sessions count with list
- Pending scheduled tasks
- Recent activity feed
- Quick actions (create room, run prompt)

### Chat View

Full in-app chat experience when diving into a room:

```
┌─────────────────────────────────────────────────┐
│ ← Back    gorp: research                    ⚙️  │
├─────────────────────────────────────────────────┤
│                                                 │
│  ┌─────────────────────────────────────────┐   │
│  │ You (10:32 AM)                          │   │
│  │ Find recent papers on rust GUI          │   │
│  └─────────────────────────────────────────┘   │
│                                                 │
│  ┌─────────────────────────────────────────┐   │
│  │ Claude (10:32 AM)              ◐ typing │   │
│  │ I found several relevant papers...      │   │
│  │                                         │   │
│  │ **Tool use:** web_search               │   │
│  │ **Tokens:** 1,247 in / 892 out         │   │
│  └─────────────────────────────────────────┘   │
│                                                 │
├─────────────────────────────────────────────────┤
│ ┌─────────────────────────────────┐  [Send]    │
│ │ Type a message...               │            │
│ └─────────────────────────────────┘            │
└─────────────────────────────────────────────────┘
```

**Features:**
- Markdown rendering (pulldown-cmark)
- Streaming responses as tokens arrive
- Collapsible tool call visibility
- Per-message token usage
- Room settings (⚙️): system prompt, model, MCP servers

**Data flow:**
1. User types message → sends to `message_handler`
2. Agent streams response → iced subscription receives events
3. UI updates reactively

### Quick Prompt

Floating window for fast one-off prompts (⌘N global hotkey):

```
┌────────────────────────────────────────────┐
│ Quick Prompt                          ✕    │
├────────────────────────────────────────────┤
│ ┌────────────────────────────────────────┐ │
│ │ Summarize my unread emails            │ │
│ └────────────────────────────────────────┘ │
│                              [→ DISPATCH]  │
└────────────────────────────────────────────┘
```

- Routes directly to DISPATCH room
- DISPATCH decides: handle inline, delegate, spawn task
- Response appears as notification or opens relevant room

### Settings View

```
┌─────────────────────────────────────────────────┐
│ Settings                                        │
├─────────────────────────────────────────────────┤
│                                                 │
│ ▼ Matrix Connection                             │
│   Homeserver: [matrix.org            ]          │
│   User ID:    [@bot:matrix.org       ]          │
│   Status:     ● Connected (syncing)             │
│   [Reconnect]                                   │
│                                                 │
│ ▼ Agent Backend                                 │
│   Type:       [ACP ▾]                           │
│   Binary:     [claude                ]          │
│   Model:      [claude-sonnet-4-20250514 ▾]      │
│   Keep-alive: [300] seconds                     │
│                                                 │
│ ▼ Web Admin                                     │
│   [✓] Enable web admin panel                    │
│   Port: [13000]                                 │
│   Bind: [127.0.0.1    ]                         │
│                                                 │
│ ▼ Startup                                       │
│   [✓] Launch at login                           │
│   [✓] Start minimized to tray                   │
│                                                 │
│ ▼ Notifications                                 │
│   [✓] Show notifications for mentions           │
│   [✓] Show notifications for scheduled tasks    │
│   [ ] Play sounds                               │
│                                                 │
│                          [Save] [Reset]         │
└─────────────────────────────────────────────────┘
```

**Notes:**
- Settings map to existing `Config` struct
- Changes write to `config.toml`
- Some settings need restart (indicated in UI)
- Headless mode NOT exposed here (CLI-only feature)

---

## Project Structure

```
gorp-rs/
├── Cargo.toml              # workspace root
├── src/
│   ├── main.rs             # Entry point (GUI vs headless routing)
│   ├── lib.rs              # Existing server code
│   ├── gui/                # NEW - iced UI
│   │   ├── mod.rs          # GorpApp, run_gui()
│   │   ├── app.rs          # Application state & update loop
│   │   ├── views/
│   │   │   ├── mod.rs
│   │   │   ├── dashboard.rs
│   │   │   ├── chat.rs
│   │   │   ├── settings.rs
│   │   │   ├── schedules.rs
│   │   │   └── logs.rs
│   │   ├── components/
│   │   │   ├── mod.rs
│   │   │   ├── sidebar.rs
│   │   │   ├── message.rs
│   │   │   └── tray.rs
│   │   └── theme.rs        # Styling
│   └── ... (existing modules)
├── gorp-agent/
├── gorp-core/
└── gorp-ffi/               # Can deprecate later
```

---

## Dependencies

New dependencies to add:

```toml
[dependencies]
iced = { version = "0.13", features = ["tokio", "svg", "image"] }
tray-icon = "0.14"          # Menu bar integration
global-hotkey = "0.5"       # ⌘N quick prompt
```

Existing deps already cover:
- `pulldown-cmark` - Markdown rendering
- `tokio` - Async runtime
- `directories` - Config/data paths

---

## Packaging & Distribution

### macOS .app Bundle

```
gorp.app/
├── Contents/
│   ├── Info.plist          # App metadata, version, icons
│   ├── MacOS/
│   │   └── gorp            # The single binary
│   ├── Resources/
│   │   ├── gorp.icns       # App icon
│   │   └── config.toml.example
│   └── _CodeSignature/     # For notarization
```

### Build Pipeline

```bash
# 1. Build universal binary (Intel + Apple Silicon)
cargo build --release --target x86_64-apple-darwin
cargo build --release --target aarch64-apple-darwin
lipo -create -output gorp target/*/release/gorp

# 2. Create .app bundle
cargo bundle --release  # or custom script

# 3. Sign & notarize (for Gatekeeper)
codesign --deep --sign "Developer ID" gorp.app
xcrun notarytool submit gorp.app ...

# 4. Create DMG
create-dmg gorp.app --output gorp-x.y.z.dmg
```

### Homebrew Cask

```ruby
cask "gorp" do
  version "0.4.0"
  sha256 "..."
  url "https://github.com/2389-research/gorp-rs/releases/download/v#{version}/gorp-#{version}.dmg"
  name "gorp"
  desc "Personal AI agent desktop for Matrix-Claude bridge"
  homepage "https://github.com/2389-research/gorp-rs"
  app "gorp.app"
  binary "#{appdir}/gorp.app/Contents/MacOS/gorp"
end
```

### Launch at Login

Creates LaunchAgent plist in `~/Library/LaunchAgents/com.2389.gorp.plist`:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "...">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.2389.gorp</string>
    <key>ProgramArguments</key>
    <array>
        <string>/Applications/gorp.app/Contents/MacOS/gorp</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
</dict>
</plist>
```

---

## Implementation Plan

### Phase 1: Skeleton
- Add iced dependency
- Create `src/gui/` module structure
- Implement basic window with placeholder views
- Wire up entry point routing (GUI vs headless)

### Phase 2: Tray Integration
- Menu bar icon with tray-icon crate
- Basic menu (status, open dashboard, quit)
- Icon state changes based on connection

### Phase 3: Dashboard
- Connection status display
- Active sessions list
- Scheduled tasks overview
- Recent activity feed

### Phase 4: Chat View
- Message list with markdown rendering
- Input field and send
- Streaming response display
- Tool call visibility

### Phase 5: Settings
- Config editing UI
- Web admin toggle
- Launch at login toggle
- Save/reload config

### Phase 6: Quick Prompt
- Global hotkey registration
- Floating window
- DISPATCH routing

### Phase 7: Packaging
- .app bundle creation
- Code signing & notarization
- DMG creation
- Homebrew cask formula

---

## Open Questions

1. **App icon design** - Need a good icon for menu bar and .app
2. **Notification framework** - Use native macOS notifications or iced's?
3. **Theme** - Match macOS system appearance or custom?

---

## References

- [iced documentation](https://docs.rs/iced)
- [tray-icon crate](https://docs.rs/tray-icon)
- [Apple notarization docs](https://developer.apple.com/documentation/security/notarizing_macos_software_before_distribution)
- [cargo-bundle](https://github.com/burtonageo/cargo-bundle)
