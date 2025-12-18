# Workstation Webapp Phase 2 Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add terminal access and browser viewer to the workstation webapp, enabling users to interact with their container's shell and watch Chrome automation.

**Architecture:** Gorp spawns PTY processes and manages Chrome CDP connections. WebSocket endpoints in gorp proxy terminal I/O and browser frames to the workstation webapp. xterm.js for terminal rendering, canvas for browser viewer.

**Tech Stack:** portable-pty (PTY), chromiumoxide (CDP), axum WebSockets, xterm.js, HTML5 canvas

---

## Phase 2 Features

1. **Terminal** - Shell access to user's container via WebSocket + xterm.js
2. **Browser Viewer** - Watch/control Chrome via CDP screencast

Note: Cloudflare Tunnel deferred to Phase 3 (requires deployment infrastructure decisions).

---

## Task 1: Add Terminal Dependencies to Gorp

**Files:**
- Modify: `/Cargo.toml` (workspace)
- Modify: `/gorp/Cargo.toml`

**Step 1: Add portable-pty to workspace dependencies**

Add to root `/Cargo.toml` under `[workspace.dependencies]`:

```toml
portable-pty = "0.8"
```

**Step 2: Add to gorp/Cargo.toml**

Add to `/gorp/Cargo.toml` dependencies:

```toml
portable-pty = { workspace = true }
```

**Step 3: Verify build**

```bash
cargo build -p gorp
```

Expected: BUILD SUCCESS

**Step 4: Commit**

```bash
git add -A
git commit -m "chore(gorp): add portable-pty dependency"
```

---

## Task 2: Create Terminal Module in Gorp

**Files:**
- Create: `/gorp/src/terminal.rs`
- Modify: `/gorp/src/lib.rs`

**Step 1: Create gorp/src/terminal.rs**

```rust
// ABOUTME: Terminal PTY management for spawning shells in containers.
// ABOUTME: Handles PTY creation, I/O streaming, and session lifecycle.

use anyhow::{Context, Result};
use portable_pty::{native_pty_system, CommandBuilder, PtySize, PtySystem};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex, RwLock};
use uuid::Uuid;

/// Terminal session state
pub struct TerminalSession {
    pub id: String,
    pub workspace_path: String,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    shutdown_tx: Option<mpsc::Sender<()>>,
}

impl TerminalSession {
    /// Write data to the PTY (user input)
    pub async fn write(&self, data: &[u8]) -> Result<()> {
        let mut writer = self.writer.lock().await;
        writer.write_all(data).context("Failed to write to PTY")?;
        writer.flush().context("Failed to flush PTY")?;
        Ok(())
    }

    /// Signal shutdown
    pub async fn shutdown(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(()).await;
        }
    }
}

/// Manages terminal sessions
pub struct TerminalManager {
    sessions: RwLock<HashMap<String, Arc<TerminalSession>>>,
}

impl TerminalManager {
    pub fn new() -> Self {
        Self {
            sessions: RwLock::new(HashMap::new()),
        }
    }

    /// Spawn a new terminal session
    pub async fn spawn(
        &self,
        workspace_path: String,
        output_tx: mpsc::Sender<Vec<u8>>,
    ) -> Result<Arc<TerminalSession>> {
        let session_id = Uuid::new_v4().to_string();

        // Create PTY
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            })
            .context("Failed to open PTY")?;

        // Spawn shell
        let mut cmd = CommandBuilder::new("bash");
        cmd.cwd(&workspace_path);
        cmd.env("TERM", "xterm-256color");

        let _child = pair
            .slave
            .spawn_command(cmd)
            .context("Failed to spawn shell")?;

        // Get reader and writer
        let reader = pair
            .master
            .try_clone_reader()
            .context("Failed to clone PTY reader")?;
        let writer = pair
            .master
            .take_writer()
            .context("Failed to take PTY writer")?;

        // Create shutdown channel
        let (shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(1);

        // Spawn reader task
        let session_id_clone = session_id.clone();
        tokio::task::spawn_blocking(move || {
            let mut reader = reader;
            let mut buffer = [0u8; 4096];

            loop {
                // Check for shutdown (non-blocking)
                if shutdown_rx.try_recv().is_ok() {
                    break;
                }

                match reader.read(&mut buffer) {
                    Ok(0) => break, // EOF
                    Ok(n) => {
                        let data = buffer[..n].to_vec();
                        if output_tx.blocking_send(data).is_err() {
                            break; // Channel closed
                        }
                    }
                    Err(e) => {
                        tracing::error!(session = %session_id_clone, error = %e, "PTY read error");
                        break;
                    }
                }
            }
            tracing::info!(session = %session_id_clone, "PTY reader stopped");
        });

        let session = Arc::new(TerminalSession {
            id: session_id.clone(),
            workspace_path,
            writer: Arc::new(Mutex::new(writer)),
            shutdown_tx: Some(shutdown_tx),
        });

        self.sessions.write().await.insert(session_id, session.clone());

        Ok(session)
    }

    /// Get an existing session
    pub async fn get(&self, session_id: &str) -> Option<Arc<TerminalSession>> {
        self.sessions.read().await.get(session_id).cloned()
    }

    /// Remove a session
    pub async fn remove(&self, session_id: &str) -> Option<Arc<TerminalSession>> {
        self.sessions.write().await.remove(session_id)
    }

    /// Resize a terminal
    pub async fn resize(&self, session_id: &str, rows: u16, cols: u16) -> Result<()> {
        // Note: portable-pty resize requires access to the master, which we don't store
        // For now, log and skip - can be enhanced later
        tracing::debug!(session = %session_id, rows, cols, "Terminal resize requested (not implemented)");
        Ok(())
    }
}

impl Default for TerminalManager {
    fn default() -> Self {
        Self::new()
    }
}
```

**Step 2: Export terminal module in lib.rs**

Add to `/gorp/src/lib.rs`:

```rust
pub mod terminal;
```

**Step 3: Verify build**

```bash
cargo build -p gorp
```

Expected: BUILD SUCCESS

**Step 4: Commit**

```bash
git add -A
git commit -m "feat(gorp): add terminal PTY manager"
```

---

## Task 3: Add WebSocket Terminal Endpoint to Gorp

**Files:**
- Modify: `/gorp/src/admin/routes.rs`
- Modify: `/gorp/src/admin/mod.rs`

**Step 1: Add WebSocket imports and state**

Add to `/gorp/src/admin/routes.rs` imports:

```rust
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
```

**Step 2: Add TerminalManager to AdminState**

Modify `/gorp/src/admin/mod.rs` to add TerminalManager to AdminState:

```rust
use crate::terminal::TerminalManager;
use std::sync::Arc;

pub struct AdminState {
    pub config: Config,
    pub session_store: SessionStore,
    pub scheduler: Arc<Scheduler>,
    pub terminal_manager: Arc<TerminalManager>,  // Add this
}
```

**Step 3: Add terminal routes**

Add to router in `/gorp/src/admin/routes.rs`:

```rust
.route("/api/terminal", post(api_create_terminal))
.route("/ws/terminal/:session_id", get(ws_terminal))
```

**Step 4: Add terminal handlers**

Add to `/gorp/src/admin/routes.rs`:

```rust
#[derive(serde::Deserialize)]
struct CreateTerminalRequest {
    workspace_path: Option<String>,
}

#[derive(serde::Serialize)]
struct CreateTerminalResponse {
    session_id: String,
    ws_url: String,
}

async fn api_create_terminal(
    State(state): State<AdminState>,
    axum::Json(req): axum::Json<CreateTerminalRequest>,
) -> impl IntoResponse {
    let workspace_path = req.workspace_path.unwrap_or_else(|| "./workspace".to_string());

    // Create a dummy channel for now - actual streaming happens via WebSocket
    let (tx, _rx) = mpsc::channel(1);

    match state.terminal_manager.spawn(workspace_path, tx).await {
        Ok(session) => {
            let response = CreateTerminalResponse {
                session_id: session.id.clone(),
                ws_url: format!("/admin/ws/terminal/{}", session.id),
            };
            axum::Json(response).into_response()
        }
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to create terminal: {}", e),
        )
            .into_response(),
    }
}

async fn ws_terminal(
    State(state): State<AdminState>,
    Path(session_id): Path<String>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_terminal_ws(socket, state, session_id))
}

async fn handle_terminal_ws(socket: WebSocket, state: AdminState, session_id: String) {
    let (mut ws_sender, mut ws_receiver) = socket.split();

    // Create output channel for this WebSocket connection
    let (output_tx, mut output_rx) = mpsc::channel::<Vec<u8>>(256);

    // Spawn terminal with output channel
    let workspace_path = state.config.workspace.path.clone();
    let session = match state.terminal_manager.spawn(workspace_path, output_tx).await {
        Ok(s) => s,
        Err(e) => {
            tracing::error!(error = %e, "Failed to spawn terminal");
            let _ = ws_sender.send(Message::Text(format!("Error: {}", e))).await;
            return;
        }
    };

    tracing::info!(session_id = %session.id, "Terminal WebSocket connected");

    // Task to forward PTY output to WebSocket
    let session_clone = session.clone();
    let output_task = tokio::spawn(async move {
        while let Some(data) = output_rx.recv().await {
            if ws_sender.send(Message::Binary(data)).await.is_err() {
                break;
            }
        }
        tracing::info!(session_id = %session_clone.id, "Output task ended");
    });

    // Forward WebSocket input to PTY
    while let Some(msg) = ws_receiver.next().await {
        match msg {
            Ok(Message::Binary(data)) => {
                if let Err(e) = session.write(&data).await {
                    tracing::error!(error = %e, "Failed to write to PTY");
                    break;
                }
            }
            Ok(Message::Text(text)) => {
                if let Err(e) = session.write(text.as_bytes()).await {
                    tracing::error!(error = %e, "Failed to write to PTY");
                    break;
                }
            }
            Ok(Message::Close(_)) => break,
            Err(e) => {
                tracing::error!(error = %e, "WebSocket error");
                break;
            }
            _ => {}
        }
    }

    // Cleanup
    output_task.abort();
    state.terminal_manager.remove(&session.id).await;
    tracing::info!(session_id = %session.id, "Terminal WebSocket disconnected");
}
```

**Step 5: Update main.rs to create TerminalManager**

In `/gorp/src/main.rs`, update AdminState creation to include TerminalManager:

```rust
use crate::terminal::TerminalManager;

// In the webhook/admin server setup:
let terminal_manager = Arc::new(TerminalManager::new());

let admin_state = AdminState {
    config: config.clone(),
    session_store: session_store.clone(),
    scheduler: scheduler.clone(),
    terminal_manager,
};
```

**Step 6: Verify build**

```bash
cargo build -p gorp
```

Expected: BUILD SUCCESS

**Step 7: Commit**

```bash
git add -A
git commit -m "feat(gorp): add WebSocket terminal endpoint"
```

---

## Task 4: Add Terminal Page to Workstation

**Files:**
- Create: `/workstation/templates/terminal.html`
- Modify: `/workstation/src/routes.rs`
- Modify: `/workstation/src/templates.rs`
- Modify: `/workstation/templates/base.html`

**Step 1: Add xterm.js to base.html**

Add to `/workstation/templates/base.html` in the `<head>`:

```html
<link rel="stylesheet" href="https://cdn.jsdelivr.net/npm/xterm@5.3.0/css/xterm.min.css" />
<script src="https://cdn.jsdelivr.net/npm/xterm@5.3.0/lib/xterm.min.js"></script>
<script src="https://cdn.jsdelivr.net/npm/xterm-addon-fit@0.8.0/lib/xterm-addon-fit.min.js"></script>
```

**Step 2: Create terminal.html template**

Create `/workstation/templates/terminal.html`:

```html
{% extends "base.html" %}

{% block title %}Terminal - Workstation{% endblock %}

{% block nav %}
{% match user %}
{% when Some with (u) %}
<span class="text-gray-400">{{ u }}</span>
<a href="/auth/logout" class="text-red-400 hover:text-red-300">Logout</a>
{% when None %}
<a href="/auth/login" class="bg-blue-600 hover:bg-blue-500 px-4 py-2 rounded">Login</a>
{% endmatch %}
{% endblock %}

{% block content %}
<div class="space-y-4">
    <div class="flex items-center justify-between">
        <h1 class="text-2xl font-bold">Terminal</h1>
        <button id="connect-btn" class="bg-green-600 hover:bg-green-500 px-4 py-2 rounded">
            Connect
        </button>
    </div>

    <div id="terminal-container" class="bg-black rounded-lg border border-gray-700 p-2" style="height: 500px;">
    </div>

    <div id="status" class="text-gray-400 text-sm">
        Status: Disconnected
    </div>
</div>

<script>
document.addEventListener('DOMContentLoaded', function() {
    const terminalContainer = document.getElementById('terminal-container');
    const connectBtn = document.getElementById('connect-btn');
    const statusEl = document.getElementById('status');

    let term = null;
    let ws = null;
    let fitAddon = null;

    function initTerminal() {
        term = new Terminal({
            cursorBlink: true,
            theme: {
                background: '#1a1a1a',
                foreground: '#e0e0e0',
            },
            fontSize: 14,
            fontFamily: 'Menlo, Monaco, "Courier New", monospace',
        });

        fitAddon = new FitAddon.FitAddon();
        term.loadAddon(fitAddon);

        term.open(terminalContainer);
        fitAddon.fit();

        term.onData(function(data) {
            if (ws && ws.readyState === WebSocket.OPEN) {
                ws.send(data);
            }
        });

        window.addEventListener('resize', function() {
            if (fitAddon) fitAddon.fit();
        });
    }

    async function connect() {
        if (ws && ws.readyState === WebSocket.OPEN) {
            ws.close();
            return;
        }

        statusEl.textContent = 'Status: Connecting...';

        try {
            // Create terminal session via gorp API
            const response = await fetch('{{ gorp_api_url }}/admin/api/terminal', {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({ workspace_path: '{{ workspace_path }}' }),
            });

            if (!response.ok) {
                throw new Error('Failed to create terminal session');
            }

            const data = await response.json();
            const wsUrl = '{{ gorp_ws_url }}' + data.ws_url;

            if (!term) initTerminal();

            ws = new WebSocket(wsUrl);
            ws.binaryType = 'arraybuffer';

            ws.onopen = function() {
                statusEl.textContent = 'Status: Connected';
                connectBtn.textContent = 'Disconnect';
                connectBtn.className = 'bg-red-600 hover:bg-red-500 px-4 py-2 rounded';
                term.focus();
            };

            ws.onmessage = function(event) {
                if (event.data instanceof ArrayBuffer) {
                    const decoder = new TextDecoder();
                    term.write(decoder.decode(event.data));
                } else {
                    term.write(event.data);
                }
            };

            ws.onclose = function() {
                statusEl.textContent = 'Status: Disconnected';
                connectBtn.textContent = 'Connect';
                connectBtn.className = 'bg-green-600 hover:bg-green-500 px-4 py-2 rounded';
            };

            ws.onerror = function(error) {
                statusEl.textContent = 'Status: Error';
                console.error('WebSocket error:', error);
            };

        } catch (error) {
            statusEl.textContent = 'Status: Error - ' + error.message;
            console.error('Connection error:', error);
        }
    }

    connectBtn.addEventListener('click', connect);
});
</script>
{% endblock %}
```

**Step 3: Add TerminalTemplate to templates.rs**

Add to `/workstation/src/templates.rs`:

```rust
#[derive(Template)]
#[template(path = "terminal.html")]
pub struct TerminalTemplate {
    pub user: Option<String>,
    pub gorp_api_url: String,
    pub gorp_ws_url: String,
    pub workspace_path: String,
}
```

**Step 4: Add terminal route**

Add to `/workstation/src/routes.rs`:

```rust
use crate::templates::{IndexTemplate, TerminalTemplate};

// Add route:
.route("/terminal", get(terminal))

// Add handler:
async fn terminal(State(state): State<AppState>, session: Session) -> impl IntoResponse {
    let user = get_current_user(&session).await;

    // Convert http URL to ws URL
    let gorp_ws_url = state.config.gorp_api_url
        .replace("http://", "ws://")
        .replace("https://", "wss://");

    let template = TerminalTemplate {
        user,
        gorp_api_url: state.config.gorp_api_url.clone(),
        gorp_ws_url,
        workspace_path: state.config.workspace_path.clone(),
    };
    Html(template.render().unwrap())
}
```

**Step 5: Add terminal link to index.html**

Add to `/workstation/templates/index.html` after the channel list section:

```html
<div class="bg-gray-800 rounded-lg p-6 border border-gray-700">
    <h2 class="text-lg font-semibold mb-4">Tools</h2>
    <div class="flex gap-4">
        <a href="/terminal" class="bg-green-600 hover:bg-green-500 px-4 py-2 rounded">
            Terminal
        </a>
        <a href="/browser" class="bg-purple-600 hover:bg-purple-500 px-4 py-2 rounded">
            Browser Viewer
        </a>
    </div>
</div>
```

**Step 6: Verify build**

```bash
cargo build -p workstation
```

Expected: BUILD SUCCESS

**Step 7: Commit**

```bash
git add -A
git commit -m "feat(workstation): add terminal page with xterm.js"
```

---

## Task 5: Add Browser Dependencies

**Files:**
- Modify: `/Cargo.toml` (workspace)
- Modify: `/gorp/Cargo.toml`

**Step 1: Add chromiumoxide to workspace dependencies**

Add to root `/Cargo.toml` under `[workspace.dependencies]`:

```toml
chromiumoxide = { version = "0.7", features = ["tokio-runtime"], default-features = false }
base64 = "0.22"
```

**Step 2: Add to gorp/Cargo.toml**

Add to `/gorp/Cargo.toml` dependencies:

```toml
chromiumoxide = { workspace = true }
base64 = { workspace = true }
```

**Step 3: Verify build**

```bash
cargo build -p gorp
```

Expected: BUILD SUCCESS (may take a while - chromiumoxide is large)

**Step 4: Commit**

```bash
git add -A
git commit -m "chore(gorp): add chromiumoxide dependency"
```

---

## Task 6: Create Browser Module in Gorp

**Files:**
- Create: `/gorp/src/browser.rs`
- Modify: `/gorp/src/lib.rs`

**Step 1: Create gorp/src/browser.rs**

```rust
// ABOUTME: Browser CDP management for Chrome DevTools Protocol integration.
// ABOUTME: Handles screencast streaming and remote control of browser instances.

use anyhow::{Context, Result};
use chromiumoxide::cdp::browser_protocol::page::{
    CaptureScreenshotParams, CaptureScreenshotFormat,
};
use chromiumoxide::{Browser, BrowserConfig, Page};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use uuid::Uuid;

/// Browser session for CDP streaming
pub struct BrowserSession {
    pub id: String,
    pub page: Arc<Page>,
    shutdown_tx: Option<mpsc::Sender<()>>,
}

impl BrowserSession {
    /// Take a screenshot and return base64-encoded PNG
    pub async fn screenshot(&self) -> Result<String> {
        let params = CaptureScreenshotParams::builder()
            .format(CaptureScreenshotFormat::Png)
            .build();

        let screenshot = self.page.execute(params).await?;
        Ok(screenshot.data)
    }

    /// Navigate to a URL
    pub async fn navigate(&self, url: &str) -> Result<()> {
        self.page.goto(url).await?;
        Ok(())
    }

    /// Click at coordinates
    pub async fn click(&self, x: f64, y: f64) -> Result<()> {
        self.page.click_point(chromiumoxide::cdp::browser_protocol::page::Point::new(x, y)).await?;
        Ok(())
    }

    /// Type text
    pub async fn type_text(&self, text: &str) -> Result<()> {
        self.page.type_str(text).await?;
        Ok(())
    }

    /// Signal shutdown
    pub async fn shutdown(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(()).await;
        }
    }
}

/// Manages browser sessions
pub struct BrowserManager {
    browser: RwLock<Option<Browser>>,
    sessions: RwLock<HashMap<String, Arc<BrowserSession>>>,
}

impl BrowserManager {
    pub fn new() -> Self {
        Self {
            browser: RwLock::new(None),
            sessions: RwLock::new(HashMap::new()),
        }
    }

    /// Initialize browser if not already running
    async fn ensure_browser(&self) -> Result<()> {
        let mut browser_lock = self.browser.write().await;
        if browser_lock.is_none() {
            let (browser, mut handler) = Browser::launch(
                BrowserConfig::builder()
                    .window_size(1280, 720)
                    .build()
                    .map_err(|e| anyhow::anyhow!("Browser config error: {}", e))?,
            )
            .await
            .context("Failed to launch browser")?;

            // Spawn handler task
            tokio::spawn(async move {
                while let Some(event) = handler.next().await {
                    tracing::trace!(?event, "Browser event");
                }
            });

            *browser_lock = Some(browser);
            tracing::info!("Browser launched");
        }
        Ok(())
    }

    /// Create a new browser session
    pub async fn create_session(&self) -> Result<Arc<BrowserSession>> {
        self.ensure_browser().await?;

        let browser_lock = self.browser.read().await;
        let browser = browser_lock.as_ref().context("Browser not initialized")?;

        let page = browser.new_page("about:blank").await?;
        let session_id = Uuid::new_v4().to_string();

        let (shutdown_tx, _shutdown_rx) = mpsc::channel::<()>(1);

        let session = Arc::new(BrowserSession {
            id: session_id.clone(),
            page: Arc::new(page),
            shutdown_tx: Some(shutdown_tx),
        });

        self.sessions.write().await.insert(session_id, session.clone());
        Ok(session)
    }

    /// Get an existing session
    pub async fn get(&self, session_id: &str) -> Option<Arc<BrowserSession>> {
        self.sessions.read().await.get(session_id).cloned()
    }

    /// Remove a session
    pub async fn remove(&self, session_id: &str) -> Option<Arc<BrowserSession>> {
        self.sessions.write().await.remove(session_id)
    }
}

impl Default for BrowserManager {
    fn default() -> Self {
        Self::new()
    }
}
```

**Step 2: Export browser module in lib.rs**

Add to `/gorp/src/lib.rs`:

```rust
pub mod browser;
```

**Step 3: Verify build**

```bash
cargo build -p gorp
```

Expected: BUILD SUCCESS

**Step 4: Commit**

```bash
git add -A
git commit -m "feat(gorp): add browser CDP manager"
```

---

## Task 7: Add WebSocket Browser Endpoint to Gorp

**Files:**
- Modify: `/gorp/src/admin/routes.rs`
- Modify: `/gorp/src/admin/mod.rs`

**Step 1: Add BrowserManager to AdminState**

Modify `/gorp/src/admin/mod.rs`:

```rust
use crate::browser::BrowserManager;

pub struct AdminState {
    pub config: Config,
    pub session_store: SessionStore,
    pub scheduler: Arc<Scheduler>,
    pub terminal_manager: Arc<TerminalManager>,
    pub browser_manager: Arc<BrowserManager>,  // Add this
}
```

**Step 2: Add browser routes**

Add to router in `/gorp/src/admin/routes.rs`:

```rust
.route("/api/browser", post(api_create_browser))
.route("/api/browser/:session_id/screenshot", get(api_browser_screenshot))
.route("/api/browser/:session_id/action", post(api_browser_action))
.route("/ws/browser/:session_id", get(ws_browser))
```

**Step 3: Add browser handlers**

Add to `/gorp/src/admin/routes.rs`:

```rust
#[derive(serde::Serialize)]
struct CreateBrowserResponse {
    session_id: String,
    ws_url: String,
}

async fn api_create_browser(
    State(state): State<AdminState>,
) -> impl IntoResponse {
    match state.browser_manager.create_session().await {
        Ok(session) => {
            let response = CreateBrowserResponse {
                session_id: session.id.clone(),
                ws_url: format!("/admin/ws/browser/{}", session.id),
            };
            axum::Json(response).into_response()
        }
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to create browser session: {}", e),
        )
            .into_response(),
    }
}

async fn api_browser_screenshot(
    State(state): State<AdminState>,
    Path(session_id): Path<String>,
) -> impl IntoResponse {
    match state.browser_manager.get(&session_id).await {
        Some(session) => match session.screenshot().await {
            Ok(data) => axum::Json(serde_json::json!({ "data": data })).into_response(),
            Err(e) => (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                e.to_string(),
            )
                .into_response(),
        },
        None => (axum::http::StatusCode::NOT_FOUND, "Session not found").into_response(),
    }
}

#[derive(serde::Deserialize)]
struct BrowserAction {
    action: String,
    url: Option<String>,
    x: Option<f64>,
    y: Option<f64>,
    text: Option<String>,
}

async fn api_browser_action(
    State(state): State<AdminState>,
    Path(session_id): Path<String>,
    axum::Json(action): axum::Json<BrowserAction>,
) -> impl IntoResponse {
    let session = match state.browser_manager.get(&session_id).await {
        Some(s) => s,
        None => return (axum::http::StatusCode::NOT_FOUND, "Session not found").into_response(),
    };

    let result = match action.action.as_str() {
        "navigate" => {
            if let Some(url) = action.url {
                session.navigate(&url).await
            } else {
                Err(anyhow::anyhow!("Missing url for navigate"))
            }
        }
        "click" => {
            if let (Some(x), Some(y)) = (action.x, action.y) {
                session.click(x, y).await
            } else {
                Err(anyhow::anyhow!("Missing x,y for click"))
            }
        }
        "type" => {
            if let Some(text) = action.text {
                session.type_text(&text).await
            } else {
                Err(anyhow::anyhow!("Missing text for type"))
            }
        }
        _ => Err(anyhow::anyhow!("Unknown action: {}", action.action)),
    };

    match result {
        Ok(_) => axum::Json(serde_json::json!({ "success": true })).into_response(),
        Err(e) => (axum::http::StatusCode::BAD_REQUEST, e.to_string()).into_response(),
    }
}

async fn ws_browser(
    State(state): State<AdminState>,
    Path(session_id): Path<String>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_browser_ws(socket, state, session_id))
}

async fn handle_browser_ws(socket: WebSocket, state: AdminState, session_id: String) {
    let (mut ws_sender, mut ws_receiver) = socket.split();

    let session = match state.browser_manager.get(&session_id).await {
        Some(s) => s,
        None => {
            let _ = ws_sender
                .send(Message::Text("Session not found".to_string()))
                .await;
            return;
        }
    };

    tracing::info!(session_id = %session_id, "Browser WebSocket connected");

    // Periodic screenshot streaming
    let session_clone = session.clone();
    let screenshot_task = tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_millis(100));
        loop {
            interval.tick().await;
            match session_clone.screenshot().await {
                Ok(data) => {
                    let msg = serde_json::json!({
                        "type": "frame",
                        "data": data,
                    });
                    if ws_sender
                        .send(Message::Text(serde_json::to_string(&msg).unwrap()))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
                Err(e) => {
                    tracing::error!(error = %e, "Screenshot error");
                }
            }
        }
    });

    // Handle incoming messages (actions)
    while let Some(msg) = ws_receiver.next().await {
        match msg {
            Ok(Message::Text(text)) => {
                if let Ok(action) = serde_json::from_str::<BrowserAction>(&text) {
                    let result = match action.action.as_str() {
                        "navigate" => {
                            if let Some(url) = action.url {
                                session.navigate(&url).await
                            } else {
                                continue;
                            }
                        }
                        "click" => {
                            if let (Some(x), Some(y)) = (action.x, action.y) {
                                session.click(x, y).await
                            } else {
                                continue;
                            }
                        }
                        "type" => {
                            if let Some(text) = action.text {
                                session.type_text(&text).await
                            } else {
                                continue;
                            }
                        }
                        _ => continue,
                    };
                    if let Err(e) = result {
                        tracing::error!(error = %e, "Browser action error");
                    }
                }
            }
            Ok(Message::Close(_)) => break,
            Err(e) => {
                tracing::error!(error = %e, "WebSocket error");
                break;
            }
            _ => {}
        }
    }

    screenshot_task.abort();
    state.browser_manager.remove(&session_id).await;
    tracing::info!(session_id = %session_id, "Browser WebSocket disconnected");
}
```

**Step 5: Update main.rs to create BrowserManager**

In `/gorp/src/main.rs`:

```rust
use crate::browser::BrowserManager;

// In the webhook/admin server setup:
let browser_manager = Arc::new(BrowserManager::new());

let admin_state = AdminState {
    config: config.clone(),
    session_store: session_store.clone(),
    scheduler: scheduler.clone(),
    terminal_manager,
    browser_manager,
};
```

**Step 6: Verify build**

```bash
cargo build -p gorp
```

Expected: BUILD SUCCESS

**Step 7: Commit**

```bash
git add -A
git commit -m "feat(gorp): add WebSocket browser endpoint"
```

---

## Task 8: Add Browser Viewer Page to Workstation

**Files:**
- Create: `/workstation/templates/browser.html`
- Modify: `/workstation/src/routes.rs`
- Modify: `/workstation/src/templates.rs`

**Step 1: Create browser.html template**

Create `/workstation/templates/browser.html`:

```html
{% extends "base.html" %}

{% block title %}Browser Viewer - Workstation{% endblock %}

{% block nav %}
{% match user %}
{% when Some with (u) %}
<span class="text-gray-400">{{ u }}</span>
<a href="/auth/logout" class="text-red-400 hover:text-red-300">Logout</a>
{% when None %}
<a href="/auth/login" class="bg-blue-600 hover:bg-blue-500 px-4 py-2 rounded">Login</a>
{% endmatch %}
{% endblock %}

{% block content %}
<div class="space-y-4">
    <div class="flex items-center justify-between">
        <h1 class="text-2xl font-bold">Browser Viewer</h1>
        <div class="flex gap-2">
            <input type="text" id="url-input" placeholder="Enter URL..."
                   class="bg-gray-800 border border-gray-600 rounded px-3 py-2 w-64">
            <button id="go-btn" class="bg-blue-600 hover:bg-blue-500 px-4 py-2 rounded">Go</button>
            <button id="connect-btn" class="bg-green-600 hover:bg-green-500 px-4 py-2 rounded">
                Connect
            </button>
        </div>
    </div>

    <div id="browser-container" class="bg-black rounded-lg border border-gray-700 p-2 relative"
         style="width: 1280px; height: 720px; max-width: 100%; overflow: hidden;">
        <img id="browser-frame" style="width: 100%; height: 100%; object-fit: contain;">
        <div id="click-overlay" style="position: absolute; top: 0; left: 0; right: 0; bottom: 0; cursor: crosshair;"></div>
    </div>

    <div id="status" class="text-gray-400 text-sm">
        Status: Disconnected
    </div>
</div>

<script>
document.addEventListener('DOMContentLoaded', function() {
    const browserFrame = document.getElementById('browser-frame');
    const clickOverlay = document.getElementById('click-overlay');
    const connectBtn = document.getElementById('connect-btn');
    const goBtn = document.getElementById('go-btn');
    const urlInput = document.getElementById('url-input');
    const statusEl = document.getElementById('status');

    let ws = null;
    let sessionId = null;

    async function connect() {
        if (ws && ws.readyState === WebSocket.OPEN) {
            ws.close();
            return;
        }

        statusEl.textContent = 'Status: Connecting...';

        try {
            const response = await fetch('{{ gorp_api_url }}/admin/api/browser', {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
            });

            if (!response.ok) {
                throw new Error('Failed to create browser session');
            }

            const data = await response.json();
            sessionId = data.session_id;
            const wsUrl = '{{ gorp_ws_url }}' + data.ws_url;

            ws = new WebSocket(wsUrl);

            ws.onopen = function() {
                statusEl.textContent = 'Status: Connected';
                connectBtn.textContent = 'Disconnect';
                connectBtn.className = 'bg-red-600 hover:bg-red-500 px-4 py-2 rounded';
            };

            ws.onmessage = function(event) {
                try {
                    const msg = JSON.parse(event.data);
                    if (msg.type === 'frame' && msg.data) {
                        browserFrame.src = 'data:image/png;base64,' + msg.data;
                    }
                } catch (e) {
                    console.error('Parse error:', e);
                }
            };

            ws.onclose = function() {
                statusEl.textContent = 'Status: Disconnected';
                connectBtn.textContent = 'Connect';
                connectBtn.className = 'bg-green-600 hover:bg-green-500 px-4 py-2 rounded';
                sessionId = null;
            };

            ws.onerror = function(error) {
                statusEl.textContent = 'Status: Error';
                console.error('WebSocket error:', error);
            };

        } catch (error) {
            statusEl.textContent = 'Status: Error - ' + error.message;
            console.error('Connection error:', error);
        }
    }

    function navigate(url) {
        if (ws && ws.readyState === WebSocket.OPEN) {
            ws.send(JSON.stringify({ action: 'navigate', url: url }));
        }
    }

    clickOverlay.addEventListener('click', function(e) {
        if (ws && ws.readyState === WebSocket.OPEN) {
            const rect = clickOverlay.getBoundingClientRect();
            const x = (e.clientX - rect.left) * (1280 / rect.width);
            const y = (e.clientY - rect.top) * (720 / rect.height);
            ws.send(JSON.stringify({ action: 'click', x: x, y: y }));
        }
    });

    connectBtn.addEventListener('click', connect);
    goBtn.addEventListener('click', function() {
        const url = urlInput.value.trim();
        if (url) {
            navigate(url.startsWith('http') ? url : 'https://' + url);
        }
    });
    urlInput.addEventListener('keypress', function(e) {
        if (e.key === 'Enter') {
            goBtn.click();
        }
    });
});
</script>
{% endblock %}
```

**Step 2: Add BrowserTemplate to templates.rs**

Add to `/workstation/src/templates.rs`:

```rust
#[derive(Template)]
#[template(path = "browser.html")]
pub struct BrowserTemplate {
    pub user: Option<String>,
    pub gorp_api_url: String,
    pub gorp_ws_url: String,
}
```

**Step 3: Add browser route**

Add to `/workstation/src/routes.rs`:

```rust
use crate::templates::{IndexTemplate, TerminalTemplate, BrowserTemplate};

// Add route:
.route("/browser", get(browser))

// Add handler:
async fn browser(State(state): State<AppState>, session: Session) -> impl IntoResponse {
    let user = get_current_user(&session).await;

    let gorp_ws_url = state.config.gorp_api_url
        .replace("http://", "ws://")
        .replace("https://", "wss://");

    let template = BrowserTemplate {
        user,
        gorp_api_url: state.config.gorp_api_url.clone(),
        gorp_ws_url,
    };
    Html(template.render().unwrap())
}
```

**Step 4: Verify build**

```bash
cargo build -p workstation
```

Expected: BUILD SUCCESS

**Step 5: Commit**

```bash
git add -A
git commit -m "feat(workstation): add browser viewer page"
```

---

## Phase 2 Complete

At this point you have:
- Terminal access via PTY + WebSocket + xterm.js
- Browser viewer via Chrome CDP + WebSocket + canvas
- Both accessible from workstation dashboard

### Next Steps (Phase 3)
- Cloudflare Tunnel integration for public access
- Real Matrix SSO authentication
- Room membership authorization
- Persistent session storage

### Testing Notes

**Terminal:**
1. Start gorp: `cargo run -p gorp`
2. Start workstation: `cargo run -p workstation`
3. Open http://localhost:8088/terminal
4. Click Connect
5. Should get bash shell in workspace directory

**Browser:**
1. Chrome must be installed on the system
2. Open http://localhost:8088/browser
3. Click Connect
4. Enter URL and click Go
5. Should see browser rendering as screenshots
