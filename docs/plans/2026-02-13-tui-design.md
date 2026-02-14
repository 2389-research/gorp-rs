# TUI Interface — Design Document

**Date:** 2026-02-13
**Status:** Draft
**Depends on:** [Telegram Platform Design](2026-02-13-telegram-platform-design.md) (Platform Registry)

## Summary

Add a terminal user interface to gorp via `gorp tui`. The TUI serves as both an operator console (monitoring messages across all platforms, managing schedules, viewing logs) and a direct Claude interaction client (selecting workspaces and chatting with Claude via the Tier 3 LocalInterface). Built with ratatui + crossterm, feature-gated in the same binary, embedded server model matching the existing GUI pattern.

## Decisions

| Decision | Choice | Rationale |
|---|---|---|
| Framework | ratatui + crossterm | Dominant Rust TUI framework, immediate-mode rendering, large widget ecosystem |
| Scope | Full app (monitoring + interaction) | Operator console AND direct Claude chat in one interface |
| Platform awareness | Both Feed and Workspace views | Feed shows cross-platform messages, Workspace is direct Tier 3 access |
| Binary | Same binary: `gorp tui` | Feature-gated like GUI, shares all code, single build artifact |
| Server model | Embedded (like GUI) | Starts full gorp server internally, same pattern as `gorp gui` |

## Feature Gate & Entry Point

```toml
# Cargo.toml
[features]
default = ["gui", "admin"]
gui = ["dep:iced", "dep:tray-icon", "dep:global-hotkey"]
tui = ["dep:ratatui", "dep:crossterm"]
admin = ["dep:askama", "dep:tower-sessions"]
```

```rust
// main.rs
#[cfg(feature = "tui")]
Commands::Tui => gorp::tui::run_tui().await,

#[cfg(not(feature = "tui"))]
Commands::Tui => eprintln!("TUI not available - compile with --features tui"),
```

Initialization mirrors the GUI — calls `ServerState::initialize()`, sets up `PlatformRegistry`, then hands off to the TUI event loop.

## View Structure

Six views:

```rust
pub enum View {
    Dashboard,                          // Server status, platform connections, stats
    Feed,                               // Cross-platform message feed
    Workspace { name: String },         // Direct Claude interaction (Tier 3)
    Chat { channel_id: String },        // Platform channel chat
    Schedules,                          // Schedule management
    Logs,                               // Log viewer with filtering
}
```

## Layout

Three-pane design:

```
┌──────────────┬────────────────────────────────────────────┐
│  Navigation  │  Main Content                              │
│              │                                            │
│  [D]ashboard │  (varies by view)                          │
│  [F]eed      │                                            │
│  [W]orkspace │                                            │
│  [C]hannels  │                                            │
│  [S]chedules │                                            │
│  [L]ogs      │                                            │
│              │                                            │
│──────────────│                                            │
│  Status      │                                            │
│  ● Matrix    │                                            │
│  ● Telegram  │                                            │
│  ○ WhatsApp  │                                            │
│              ├────────────────────────────────────────────│
│              │  Input / Status Bar                        │
└──────────────┴────────────────────────────────────────────┘
```

- **Left sidebar** (~20 chars wide): Navigation + platform connection status
- **Main pane**: View-specific content
- **Bottom bar**: Input field (in chat/workspace views) or status info

## Feed View — Cross-Platform Message Stream

Unified view across all connected platforms. This is unique to the TUI — neither the GUI nor individual platforms provide this.

```
┌──────────────┬────────────────────────────────────────────┐
│  Navigation  │  Feed                          [filter: all]│
│              │                                            │
│ ▸ Dashboard  │  ┌─ matrix ─────────────────────────────┐  │
│ ▸ Feed       │  │ @harper  #research        2m ago     │  │
│   Workspace  │  │ Can you summarize the latest paper?  │  │
│   Channels   │  └──────────────────────────────────────┘  │
│   Schedules  │  ┌─ telegram ───────────────────────────┐  │
│   Logs       │  │ Harper   DM                 5m ago   │  │
│              │  │ Schedule the news digest for 6pm     │  │
│──────────────│  └──────────────────────────────────────┘  │
│  Platforms   │  ┌─ matrix ─────────────────────────────┐  │
│  ● Matrix    │  │ gorp-bot #research          2m ago   │  │
│  ● Telegram  │  │ Here's a summary of the paper...    │  │
│  ○ Slack     │  └──────────────────────────────────────┘  │
│  ○ WhatsApp  │  ┌─ whatsapp ──────────────────────────┐  │
│              │  │ +1555... Group:news         8m ago   │  │
│              │  │ What's trending today?               │  │
│              │  └──────────────────────────────────────┘  │
│              ├────────────────────────────────────────────│
│              │  [r]eply  [f]ilter  [j/k]scroll  [?]help  │
└──────────────┴────────────────────────────────────────────┘
```

**Features:**
- Messages color-coded by platform (matrix=blue, telegram=cyan, slack=purple, whatsapp=green)
- Each message shows: platform badge, sender, channel/chat name, relative timestamp, message preview
- Filter by platform: `f` then select
- `Enter` on a message opens it in the Chat view for that channel
- `r` on a message starts an inline reply — routes through the originating platform
- `j/k` or arrow keys to scroll, `g/G` for top/bottom

**Data source:** Consumes the `PlatformRegistry.merged_event_stream()` alongside the message handler. Messages display in the feed AND get processed by the agent pipeline.

```rust
pub struct FeedState {
    messages: VecDeque<FeedMessage>,   // Ring buffer, max 500
    selected: usize,
    platform_filter: Option<String>,
    scroll_offset: usize,
}

pub struct FeedMessage {
    pub platform_id: String,
    pub channel_name: String,
    pub sender: String,
    pub body: String,
    pub timestamp: i64,
    pub channel_id: String,           // For navigating to Chat view
    pub is_bot: bool,                 // Dim bot responses
}
```

## Workspace View — Direct Claude Interaction

Tier 3 `LocalInterface` in action. No platform middleman — pick a workspace, talk to Claude directly.

```
┌──────────────┬────────────────────────────────────────────┐
│  Navigation  │  Workspace: research            [acp]     │
│              │                                            │
│   Dashboard  │  you: What papers came in this week?       │
│   Feed       │                                            │
│ ▸ Workspace  │  claude: I found 3 new papers in the       │
│   Channels   │  arxiv feed:                               │
│   Schedules  │                                            │
│   Logs       │  1. "Scaling Laws for..." - Chen et al.    │
│              │  2. "Attention Is All You..." - Wu et al.  │
│──────────────│  3. "On the Geometry of..." - Park et al.  │
│  Workspaces  │                                            │
│ ▸ research   │  Shall I summarize any of these?           │
│   news       │                                            │
│   pa         │  you: Summarize #1                         │
│   weather    │                                            │
│              │  claude: ▌                                 │
│              │  (streaming...)                            │
│              │                                            │
│              ├────────────────────────────────────────────│
│              │ > _                                        │
└──────────────┴────────────────────────────────────────────┘
```

**Features:**
- Left sidebar changes contextually — shows workspace list instead of navigation
- `Tab` switches between nav list and workspace list
- Select workspace with arrow keys + Enter
- Input at the bottom — `Enter` to send, `\` at end of line for multiline
- Claude responses stream in character-by-character
- Streaming cursor `▌` while response is in progress
- `Ctrl+C` cancels an in-progress response
- Conversation history scrollable with `PgUp/PgDn`
- Backend indicator in top-right (`[acp]`, `[mux]`, `[direct]`)

### LocalInterface Implementation

```rust
pub struct TuiLocalInterface {
    server: Arc<ServerState>,
    active_workspace: Option<String>,
    agent_handle: Option<AgentHandle>,
}

impl LocalInterface for TuiLocalInterface {
    async fn list_workspaces(&self) -> Result<Vec<WorkspaceInfo>> {
        // Scan workspace.path directory
        // Return name, path, active flag
    }

    async fn select_workspace(&self, name: &str) -> Result<()> {
        // Set active workspace
        // Initialize or resume agent session for this workspace
        // Load conversation history from session store
    }

    fn active_workspace(&self) -> Option<&str> {
        self.active_workspace.as_deref()
    }

    async fn dispatch(&self, command: &str) -> Result<String> {
        // Route to DISPATCH control plane
        // Used when no workspace is selected
    }
}
```

### Agent Interaction

Uses gorp-agent directly — no platform layer involved:

```rust
async fn send_to_workspace(&mut self, input: &str) -> Result<()> {
    let handle = self.agent_handle.as_ref().unwrap();
    let mut stream = handle.prompt(input).await?;

    while let Some(event) = stream.next().await {
        match event {
            AgentEvent::Text(chunk) => {
                // Push chunk to TUI display, trigger re-render
            }
            AgentEvent::ToolUse { name, input } => {
                // Show tool usage indicator
            }
            AgentEvent::Complete { usage } => {
                // Show token usage in status bar
            }
            AgentEvent::Error(e) => {
                // Display error inline
            }
        }
    }
    Ok(())
}
```

## Event Loop & Rendering Architecture

ratatui immediate-mode rendering with three async event sources merged into one channel:

```rust
pub struct TuiApp {
    server: Arc<ServerState>,
    registry: Arc<PlatformRegistry>,
    local_interface: TuiLocalInterface,
    view: View,
    feed: FeedState,
    should_quit: bool,
    dashboard: DashboardState,
    chat: ChatState,
    workspace: WorkspaceState,
    schedules: SchedulesState,
    logs: LogsState,
}

pub enum TuiEvent {
    Key(KeyEvent),                         // Keyboard input
    Tick,                                  // 100ms render tick
    Platform(IncomingMessage),             // Message from any platform
    Agent(AgentEvent),                     // Streaming response from Claude
    ServerStatus(ServerStatusUpdate),      // Connection changes, errors
}
```

### Event Loop

```rust
pub async fn run_tui(config: Config) -> Result<()> {
    // 1. Initialize
    let server = ServerState::initialize(config).await?;
    let registry = setup_platforms(&server).await?;
    let mut app = TuiApp::new(server, registry);

    // 2. Setup terminal
    let mut terminal = ratatui::init();

    // 3. Event sources merged into one channel
    let (tx, mut rx) = mpsc::channel::<TuiEvent>(256);

    // Keyboard input task (poll every 50ms)
    let tx_key = tx.clone();
    tokio::spawn(async move {
        loop {
            if crossterm::event::poll(Duration::from_millis(50)).unwrap() {
                if let Ok(Event::Key(key)) = crossterm::event::read() {
                    tx_key.send(TuiEvent::Key(key)).await.ok();
                }
            }
        }
    });

    // Render tick (100ms)
    let tx_tick = tx.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(100));
        loop {
            interval.tick().await;
            tx_tick.send(TuiEvent::Tick).await.ok();
        }
    });

    // Platform events from PlatformRegistry
    let tx_platform = tx.clone();
    let mut events = registry.merged_event_stream();
    tokio::spawn(async move {
        while let Some(msg) = events.next().await {
            tx_platform.send(TuiEvent::Platform(msg)).await.ok();
        }
    });

    // 4. Main loop
    loop {
        terminal.draw(|frame| app.render(frame))?;
        if let Some(event) = rx.recv().await {
            app.handle_event(event)?;
        }
        if app.should_quit {
            break;
        }
    }

    // 5. Cleanup
    ratatui::restore();
    Ok(())
}
```

### Render Dispatch

```rust
impl TuiApp {
    fn render(&self, frame: &mut Frame) {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(20),    // Sidebar
                Constraint::Min(40),      // Main content
            ])
            .split(frame.area());

        self.render_sidebar(frame, chunks[0]);

        match &self.view {
            View::Dashboard => self.dashboard.render(frame, chunks[1]),
            View::Feed => self.feed.render(frame, chunks[1]),
            View::Workspace { .. } => self.workspace.render(frame, chunks[1]),
            View::Chat { .. } => self.chat.render(frame, chunks[1]),
            View::Schedules => self.schedules.render(frame, chunks[1]),
            View::Logs => self.logs.render(frame, chunks[1]),
        }
    }
}
```

## Other Views

### Dashboard

```
┌──────────────────────────────────────────────┐
│  gorp v0.x.x                    uptime: 4h  │
│                                              │
│  Platforms           Sessions                │
│  ● Matrix   synced   Active: 3              │
│  ● Telegram polling  Warm:   2              │
│  ○ Slack    --        Total:  47             │
│  ○ WhatsApp --                               │
│                                              │
│  Today                                       │
│  Messages received:  142                     │
│  Messages sent:      89                      │
│  Tasks executed:     12                      │
│  Schedules run:      6                       │
│                                              │
│  Recent Activity                             │
│  14:02  research  Completed news summary     │
│  13:45  pa        Sent email draft           │
│  13:30  DISPATCH  Created workspace "test"   │
└──────────────────────────────────────────────┘
```

Data from `ServerState` — session counts, scheduler stats, platform connection status. Refreshes every tick.

### Schedules

```
┌──────────────────────────────────────────────┐
│  Schedules                        [n]ew      │
│                                              │
│  Workspace   Schedule      Next Run   Status │
│  ──────────  ────────────  ────────   ────── │
│  news        daily digest  18:00      ● on   │
│  research    arxiv scan    06:00      ● on   │
│  weather     forecast      07:00      ○ off  │
│  pa          email check   */30m      ● on   │
│                                              │
│  [Enter] edit  [d]isable  [n]ew  [x]delete   │
└──────────────────────────────────────────────┘
```

Table widget with row selection. `n` opens inline form for creating schedules. Data from `SchedulerStore`.

### Logs

```
┌──────────────────────────────────────────────┐
│  Logs                 [level: INFO] [all ws] │
│                                              │
│  14:02:31 INFO  research  Agent response...  │
│  14:02:28 INFO  research  Prompt received... │
│  14:01:55 WARN  matrix    Rate limited, ...  │
│  14:01:02 INFO  DISPATCH  Schedule trigger.. │
│  14:00:01 DEBUG telegram  Polling cycle...   │
│                                              │
│  [1]ERROR [2]WARN [3]INFO [4]DEBUG  [f]ilter │
└──────────────────────────────────────────────┘
```

Tails tracing subscriber output. Filter by level with number keys, filter by workspace with `f`.

### Chat (Platform Channel)

Same layout as Workspace view but routes through the platform's `ChatChannel::send()` instead of the agent directly. Used when selecting a message from the Feed to interact in that platform channel.

## What the TUI Shares with the GUI

- `ServerState` initialization and lifecycle
- `PlatformRegistry` and merged event stream
- `SessionStore`, `SchedulerStore` — all data access
- Agent backends (ACP, mux, direct) — same `AgentHandle` API
- Config loading, logging setup

## What the TUI Does Differently

- ratatui + crossterm instead of iced
- Feed view (cross-platform unified stream) — GUI doesn't have this
- Workspace view (Tier 3 LocalInterface) — GUI doesn't have this
- No system tray, no global hotkeys
- Keyboard-only (no mouse interaction)

## Keybinding Summary

| Key | Context | Action |
|---|---|---|
| `1-6` | Global | Switch views |
| `d/f/w/c/s/l` | Global | Switch views by first letter |
| `q` | Global | Quit |
| `?` | Global | Show help overlay |
| `j/k` | Lists/scrollable | Scroll up/down |
| `g/G` | Lists/scrollable | Jump to top/bottom |
| `Enter` | Lists | Select item / open |
| `Esc` | Anywhere | Back / close modal |
| `Tab` | Workspace view | Toggle nav sidebar / workspace list |
| `r` | Feed | Reply to selected message |
| `f` | Feed / Logs | Filter |
| `n` | Schedules | Create schedule |
| `Ctrl+C` | Workspace / Chat | Cancel streaming response |
| `PgUp/PgDn` | Chat / Workspace | Scroll conversation history |

## Files Created

| File | Purpose |
|---|---|
| `src/tui/mod.rs` | `run_tui()` entry point, terminal setup/teardown |
| `src/tui/app.rs` | `TuiApp` struct, event loop, top-level render dispatch |
| `src/tui/event.rs` | `TuiEvent` enum, event source merging |
| `src/tui/sidebar.rs` | Navigation sidebar + platform status |
| `src/tui/views/mod.rs` | `View` enum |
| `src/tui/views/dashboard.rs` | Dashboard view |
| `src/tui/views/feed.rs` | Cross-platform message feed |
| `src/tui/views/workspace.rs` | Direct Claude interaction via `LocalInterface` |
| `src/tui/views/chat.rs` | Platform channel chat |
| `src/tui/views/schedules.rs` | Schedule management |
| `src/tui/views/logs.rs` | Log viewer with filtering |
| `src/tui/theme.rs` | Terminal color scheme, platform colors |

## Files Modified

| File | Change |
|---|---|
| `Cargo.toml` | Add `tui` feature flag, `ratatui` + `crossterm` deps |
| `src/main.rs` | Add `gorp tui` subcommand |
| `src/lib.rs` | Add `pub mod tui` behind feature gate |

## Files Untouched

Everything else — gorp-core, gorp-agent, gorp-ffi, all platform implementations, GUI, admin panel, message handler, scheduler, webhooks. The TUI is purely additive.

## Dependencies

```toml
[dependencies]
ratatui = { version = "0.29", optional = true }
crossterm = { version = "0.28", optional = true }
```
