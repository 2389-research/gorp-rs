# Workstation Webapp Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Build a user-facing webapp for Matrix users to configure their gorp workspaces, with file management, terminal, and browser viewer.

**Architecture:** Separate Rust/Axum service in a Cargo workspace alongside gorp. Matrix OIDC for auth. Direct filesystem access for workspace files. REST + WebSocket APIs on gorp for terminal/browser streaming.

**Tech Stack:** Rust, Axum, htmx, Tailwind CSS, askama templates, xterm.js, Matrix OIDC

---

## Phase 1: Foundation (This Plan)

- Project scaffolding (Cargo workspace)
- Basic Axum server with htmx
- Matrix OIDC authentication
- Channel listing
- File management UI

## Phase 2: Real-time Features (Future Plan)

- Terminal WebSocket + PTY
- Browser viewer CDP streaming
- Cloudflare Tunnel integration

---

## Task 1: Convert to Cargo Workspace

**Files:**
- Modify: `/Cargo.toml` (root)
- Create: `/gorp/Cargo.toml`
- Move: `/src/*` → `/gorp/src/*`

**Step 1: Create workspace Cargo.toml**

Create new root `/Cargo.toml`:

```toml
[workspace]
resolver = "2"
members = [
    "gorp",
    "workstation",
]

[workspace.package]
edition = "2021"

[workspace.dependencies]
tokio = { version = "1", features = ["full"] }
axum = "0.8"
tower = "0.5"
tower-http = { version = "0.6", features = ["trace", "cors"] }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "json"] }
anyhow = "1.0"
askama = "0.14"
tower-sessions = { version = "0.13", features = ["memory-store"] }
```

**Step 2: Move gorp to subdirectory**

```bash
mkdir -p gorp
mv src gorp/
mv tests gorp/
mv Cargo.toml gorp/Cargo.toml.bak
```

**Step 3: Create gorp/Cargo.toml**

```toml
[package]
name = "gorp"
version = "0.2.1"
edition.workspace = true

[dependencies]
tokio = { workspace = true }
matrix-sdk = { version = "0.16", features = ["e2e-encryption", "sqlite"] }
rusqlite = { version = "0.37", features = ["bundled"] }
tracing = { workspace = true }
tracing-subscriber = { workspace = true }
dotenvy = "0.15"
serde = { workspace = true }
serde_json = { workspace = true }
toml = "0.8"
anyhow = { workspace = true }
uuid = { version = "1.6", features = ["v4"] }
futures-util = "0.3"
chrono = "0.4"
axum = { workspace = true }
tower = { workspace = true }
tower-http = { workspace = true }
directories = "6.0.0"
tracing-appender = "0.2.4"
pulldown-cmark = "0.10"
two_timer = "2.2"
cron = "0.15"
regex = "1.12.2"
chrono-tz = "0.10.4"
clap = { version = "4.5.53", features = ["derive"] }
askama = { workspace = true }
tower-sessions = { workspace = true }
mime_guess = "2.0"
metrics = "0.24"
metrics-exporter-prometheus = "0.16"
```

**Step 4: Verify gorp still builds**

```bash
cargo build -p gorp
```

Expected: BUILD SUCCESS

**Step 5: Run gorp tests**

```bash
cargo test -p gorp
```

Expected: All tests pass

**Step 6: Commit**

```bash
git add -A
git commit -m "refactor: convert to cargo workspace"
```

---

## Task 2: Create Workstation Crate Scaffold

**Files:**
- Create: `/workstation/Cargo.toml`
- Create: `/workstation/src/main.rs`
- Create: `/workstation/src/lib.rs`

**Step 1: Create workstation/Cargo.toml**

```toml
[package]
name = "workstation"
version = "0.1.0"
edition.workspace = true

[dependencies]
tokio = { workspace = true }
axum = { workspace = true }
tower = { workspace = true }
tower-http = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
tracing = { workspace = true }
tracing-subscriber = { workspace = true }
anyhow = { workspace = true }
askama = { workspace = true }
tower-sessions = { workspace = true }
dotenvy = "0.15"
```

**Step 2: Create workstation/src/lib.rs**

```rust
// ABOUTME: Workstation webapp library - user-facing config UI for gorp workspaces.
// ABOUTME: Provides file management, terminal access, and browser viewer.

pub mod config;
pub mod routes;
```

**Step 3: Create workstation/src/main.rs**

```rust
// ABOUTME: Workstation webapp entry point - starts the Axum server.
// ABOUTME: Serves htmx UI for workspace configuration.

use anyhow::Result;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();

    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| "workstation=debug,tower_http=debug".into()))
        .with(tracing_subscriber::fmt::layer())
        .init();

    tracing::info!("Starting workstation webapp");

    let app = workstation::routes::create_router();

    let addr = "0.0.0.0:8088";
    tracing::info!("Listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
```

**Step 4: Create workstation/src/config.rs**

```rust
// ABOUTME: Configuration loading for workstation webapp.
// ABOUTME: Reads environment variables and config files.

use anyhow::Result;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub port: u16,
    pub gorp_api_url: String,
    pub workspace_path: String,
    pub matrix_homeserver: String,
}

impl Config {
    pub fn load() -> Result<Self> {
        Ok(Self {
            port: std::env::var("WORKSTATION_PORT")
                .unwrap_or_else(|_| "8088".to_string())
                .parse()?,
            gorp_api_url: std::env::var("GORP_API_URL")
                .unwrap_or_else(|_| "http://localhost:13000".to_string()),
            workspace_path: std::env::var("WORKSPACE_PATH")
                .unwrap_or_else(|_| "./workspace".to_string()),
            matrix_homeserver: std::env::var("MATRIX_HOMESERVER")
                .unwrap_or_else(|_| "https://matrix.org".to_string()),
        })
    }
}
```

**Step 5: Create workstation/src/routes.rs**

```rust
// ABOUTME: Axum router setup for workstation webapp.
// ABOUTME: Defines all HTTP routes and middleware.

use axum::{routing::get, Router};

pub fn create_router() -> Router {
    Router::new()
        .route("/", get(index))
        .route("/health", get(health))
}

async fn index() -> &'static str {
    "Workstation"
}

async fn health() -> &'static str {
    "ok"
}
```

**Step 6: Verify workstation builds**

```bash
cargo build -p workstation
```

Expected: BUILD SUCCESS

**Step 7: Test workstation starts**

```bash
cargo run -p workstation &
sleep 2
curl http://localhost:8088/health
pkill -f "target/debug/workstation"
```

Expected: "ok"

**Step 8: Commit**

```bash
git add -A
git commit -m "feat(workstation): scaffold new crate"
```

---

## Task 3: Add Askama Templates + htmx

**Files:**
- Create: `/workstation/templates/base.html`
- Create: `/workstation/templates/index.html`
- Modify: `/workstation/src/routes.rs`
- Create: `/workstation/src/templates.rs`

**Step 1: Create workstation/templates/base.html**

```html
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>{% block title %}Workstation{% endblock %}</title>
    <script src="https://unpkg.com/htmx.org@1.9.10"></script>
    <script src="https://cdn.tailwindcss.com"></script>
</head>
<body class="bg-gray-900 text-gray-100 min-h-screen">
    <nav class="bg-gray-800 border-b border-gray-700 px-4 py-3">
        <div class="flex items-center justify-between max-w-7xl mx-auto">
            <a href="/" class="text-xl font-bold text-blue-400">Workstation</a>
            <div class="flex items-center gap-4">
                {% block nav %}{% endblock %}
            </div>
        </div>
    </nav>
    <main class="max-w-7xl mx-auto px-4 py-6">
        {% block content %}{% endblock %}
    </main>
</body>
</html>
```

**Step 2: Create workstation/templates/index.html**

```html
{% extends "base.html" %}

{% block title %}Workstation - Dashboard{% endblock %}

{% block content %}
<div class="space-y-6">
    <h1 class="text-2xl font-bold">Welcome to Workstation</h1>
    <p class="text-gray-400">Configure your gorp workspaces.</p>

    <div class="bg-gray-800 rounded-lg p-6 border border-gray-700">
        <h2 class="text-lg font-semibold mb-4">Your Channels</h2>
        <p class="text-gray-500">Login to see your channels.</p>
    </div>
</div>
{% endblock %}
```

**Step 3: Create workstation/src/templates.rs**

```rust
// ABOUTME: Askama template definitions for workstation webapp.
// ABOUTME: Defines structs that map to HTML templates.

use askama::Template;

#[derive(Template)]
#[template(path = "index.html")]
pub struct IndexTemplate;
```

**Step 4: Update workstation/src/lib.rs**

```rust
// ABOUTME: Workstation webapp library - user-facing config UI for gorp workspaces.
// ABOUTME: Provides file management, terminal access, and browser viewer.

pub mod config;
pub mod routes;
pub mod templates;
```

**Step 5: Update workstation/src/routes.rs**

```rust
// ABOUTME: Axum router setup for workstation webapp.
// ABOUTME: Defines all HTTP routes and middleware.

use askama::Template;
use axum::{
    response::{Html, IntoResponse},
    routing::get,
    Router,
};

use crate::templates::IndexTemplate;

pub fn create_router() -> Router {
    Router::new()
        .route("/", get(index))
        .route("/health", get(health))
}

async fn index() -> impl IntoResponse {
    let template = IndexTemplate;
    Html(template.render().unwrap())
}

async fn health() -> &'static str {
    "ok"
}
```

**Step 6: Verify templates render**

```bash
cargo run -p workstation &
sleep 2
curl http://localhost:8088/ | grep -q "Workstation"
echo "Template renders: $?"
pkill -f "target/debug/workstation"
```

Expected: "Template renders: 0"

**Step 7: Commit**

```bash
git add -A
git commit -m "feat(workstation): add askama templates with htmx"
```

---

## Task 4: Add Session Middleware

**Files:**
- Modify: `/workstation/src/routes.rs`
- Modify: `/workstation/Cargo.toml`

**Step 1: Update routes.rs with session layer**

```rust
// ABOUTME: Axum router setup for workstation webapp.
// ABOUTME: Defines all HTTP routes and middleware.

use askama::Template;
use axum::{
    response::{Html, IntoResponse},
    routing::get,
    Router,
};
use tower_sessions::{MemoryStore, SessionManagerLayer};

use crate::templates::IndexTemplate;

pub fn create_router() -> Router {
    let session_store = MemoryStore::default();
    let session_layer = SessionManagerLayer::new(session_store)
        .with_secure(false)
        .with_same_site(tower_sessions::cookie::SameSite::Lax);

    Router::new()
        .route("/", get(index))
        .route("/health", get(health))
        .layer(session_layer)
}

async fn index() -> impl IntoResponse {
    let template = IndexTemplate;
    Html(template.render().unwrap())
}

async fn health() -> &'static str {
    "ok"
}
```

**Step 2: Verify session middleware works**

```bash
cargo build -p workstation
```

Expected: BUILD SUCCESS

**Step 3: Commit**

```bash
git add -A
git commit -m "feat(workstation): add session middleware"
```

---

## Task 5: Matrix OIDC Auth - Login Route

**Files:**
- Create: `/workstation/src/auth.rs`
- Modify: `/workstation/src/lib.rs`
- Modify: `/workstation/src/routes.rs`
- Modify: `/workstation/Cargo.toml`

**Step 1: Add oauth2 dependency to Cargo.toml**

Add to workstation/Cargo.toml dependencies:

```toml
oauth2 = "4.4"
reqwest = { version = "0.12", features = ["json"] }
url = "2.5"
```

**Step 2: Create workstation/src/auth.rs**

```rust
// ABOUTME: Matrix OIDC authentication for workstation webapp.
// ABOUTME: Handles login flow, token exchange, and session management.

use anyhow::Result;
use axum::{
    extract::{Query, State},
    response::Redirect,
};
use oauth2::{
    basic::BasicClient, AuthUrl, AuthorizationCode, ClientId, CsrfToken,
    PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, Scope, TokenUrl,
};
use serde::Deserialize;
use tower_sessions::Session;

use crate::AppState;

const OIDC_STATE_KEY: &str = "oidc_state";
const PKCE_VERIFIER_KEY: &str = "pkce_verifier";
pub const USER_KEY: &str = "matrix_user";

#[derive(Clone)]
pub struct OidcConfig {
    pub client: BasicClient,
}

impl OidcConfig {
    pub fn new(homeserver: &str, client_id: &str, redirect_uri: &str) -> Result<Self> {
        let auth_url = AuthUrl::new(format!(
            "{}/_matrix/client/v3/login/sso/redirect",
            homeserver
        ))?;
        let token_url = TokenUrl::new(format!("{}/_matrix/client/v3/login", homeserver))?;

        let client = BasicClient::new(ClientId::new(client_id.to_string()))
            .set_auth_uri(auth_url)
            .set_token_uri(token_url)
            .set_redirect_uri(RedirectUrl::new(redirect_uri.to_string())?);

        Ok(Self { client })
    }
}

pub async fn login(State(state): State<AppState>, session: Session) -> Redirect {
    let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();

    let (auth_url, csrf_token) = state
        .oidc
        .client
        .authorize_url(CsrfToken::new_random)
        .add_scope(Scope::new("openid".to_string()))
        .set_pkce_challenge(pkce_challenge)
        .url();

    session
        .insert(OIDC_STATE_KEY, csrf_token.secret().clone())
        .await
        .ok();
    session
        .insert(PKCE_VERIFIER_KEY, pkce_verifier.secret().clone())
        .await
        .ok();

    Redirect::to(auth_url.as_str())
}

#[derive(Deserialize)]
pub struct CallbackParams {
    pub code: String,
    pub state: String,
}

pub async fn callback(
    State(_state): State<AppState>,
    session: Session,
    Query(params): Query<CallbackParams>,
) -> Redirect {
    let stored_state: Option<String> = session.get(OIDC_STATE_KEY).await.ok().flatten();

    if stored_state.as_deref() != Some(&params.state) {
        tracing::warn!("CSRF state mismatch");
        return Redirect::to("/?error=state_mismatch");
    }

    // For now, just set a placeholder user - real OIDC exchange requires more Matrix-specific handling
    // Matrix SSO returns a login token, not standard OIDC tokens
    session.insert(USER_KEY, "authenticated").await.ok();

    session.remove::<String>(OIDC_STATE_KEY).await.ok();
    session.remove::<String>(PKCE_VERIFIER_KEY).await.ok();

    Redirect::to("/")
}

pub async fn logout(session: Session) -> Redirect {
    session.flush().await.ok();
    Redirect::to("/")
}

pub async fn get_current_user(session: &Session) -> Option<String> {
    session.get::<String>(USER_KEY).await.ok().flatten()
}
```

**Step 3: Update workstation/src/lib.rs**

```rust
// ABOUTME: Workstation webapp library - user-facing config UI for gorp workspaces.
// ABOUTME: Provides file management, terminal access, and browser viewer.

pub mod auth;
pub mod config;
pub mod routes;
pub mod templates;

use auth::OidcConfig;

#[derive(Clone)]
pub struct AppState {
    pub config: config::Config,
    pub oidc: OidcConfig,
}
```

**Step 4: Update workstation/src/routes.rs**

```rust
// ABOUTME: Axum router setup for workstation webapp.
// ABOUTME: Defines all HTTP routes and middleware.

use askama::Template;
use axum::{
    response::{Html, IntoResponse},
    routing::get,
    Router,
};
use tower_sessions::{MemoryStore, Session, SessionManagerLayer};

use crate::{
    auth::{self, get_current_user},
    templates::IndexTemplate,
    AppState,
};

pub fn create_router(state: AppState) -> Router {
    let session_store = MemoryStore::default();
    let session_layer = SessionManagerLayer::new(session_store)
        .with_secure(false)
        .with_same_site(tower_sessions::cookie::SameSite::Lax);

    Router::new()
        .route("/", get(index))
        .route("/health", get(health))
        .route("/auth/login", get(auth::login))
        .route("/auth/callback", get(auth::callback))
        .route("/auth/logout", get(auth::logout))
        .layer(session_layer)
        .with_state(state)
}

async fn index(session: Session) -> impl IntoResponse {
    let user = get_current_user(&session).await;
    let template = IndexTemplate { user };
    Html(template.render().unwrap())
}

async fn health() -> &'static str {
    "ok"
}
```

**Step 5: Update workstation/src/templates.rs**

```rust
// ABOUTME: Askama template definitions for workstation webapp.
// ABOUTME: Defines structs that map to HTML templates.

use askama::Template;

#[derive(Template)]
#[template(path = "index.html")]
pub struct IndexTemplate {
    pub user: Option<String>,
}
```

**Step 6: Update workstation/templates/index.html**

```html
{% extends "base.html" %}

{% block title %}Workstation - Dashboard{% endblock %}

{% block nav %}
{% match user %}
{% when Some with (u) %}
<span class="text-gray-400">{{ u }}</span>
<a href="/auth/logout" class="text-red-400 hover:text-red-300">Logout</a>
{% when None %}
<a href="/auth/login" class="bg-blue-600 hover:bg-blue-500 px-4 py-2 rounded">Login with Matrix</a>
{% endmatch %}
{% endblock %}

{% block content %}
<div class="space-y-6">
    <h1 class="text-2xl font-bold">Welcome to Workstation</h1>
    <p class="text-gray-400">Configure your gorp workspaces.</p>

    {% match user %}
    {% when Some with (_u) %}
    <div class="bg-gray-800 rounded-lg p-6 border border-gray-700">
        <h2 class="text-lg font-semibold mb-4">Your Channels</h2>
        <p class="text-gray-500">Channel list coming soon...</p>
    </div>
    {% when None %}
    <div class="bg-gray-800 rounded-lg p-6 border border-gray-700">
        <h2 class="text-lg font-semibold mb-4">Get Started</h2>
        <p class="text-gray-500">Login with your Matrix account to see your channels.</p>
    </div>
    {% endmatch %}
</div>
{% endblock %}
```

**Step 7: Update workstation/src/main.rs**

```rust
// ABOUTME: Workstation webapp entry point - starts the Axum server.
// ABOUTME: Serves htmx UI for workspace configuration.

use anyhow::Result;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use workstation::{auth::OidcConfig, config::Config, AppState};

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "workstation=debug,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    tracing::info!("Starting workstation webapp");

    let config = Config::load()?;
    let oidc = OidcConfig::new(
        &config.matrix_homeserver,
        "workstation",
        &format!("http://localhost:{}/auth/callback", config.port),
    )?;

    let state = AppState {
        config: config.clone(),
        oidc,
    };

    let app = workstation::routes::create_router(state);

    let addr = format!("0.0.0.0:{}", config.port);
    tracing::info!("Listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
```

**Step 8: Verify auth routes exist**

```bash
cargo build -p workstation
```

Expected: BUILD SUCCESS

**Step 9: Commit**

```bash
git add -A
git commit -m "feat(workstation): add matrix oidc auth routes"
```

---

## Task 6: File Management - List Directory

**Files:**
- Create: `/workstation/src/files.rs`
- Modify: `/workstation/src/lib.rs`
- Modify: `/workstation/src/routes.rs`
- Create: `/workstation/templates/files.html`

**Step 1: Create workstation/src/files.rs**

```rust
// ABOUTME: File management for workspace directories.
// ABOUTME: Provides list, read, write, delete operations with path safety.

use anyhow::{bail, Result};
use axum::{
    extract::{Path, State},
    response::{Html, IntoResponse},
};
use askama::Template;
use serde::Serialize;
use std::path::PathBuf;
use tower_sessions::Session;

use crate::{auth::get_current_user, AppState};

#[derive(Debug, Clone, Serialize)]
pub struct FileEntry {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
}

#[derive(Template)]
#[template(path = "files.html")]
pub struct FilesTemplate {
    pub user: Option<String>,
    pub channel: String,
    pub path: String,
    pub entries: Vec<FileEntry>,
    pub error: Option<String>,
}

fn validate_path(workspace: &str, channel: &str, subpath: &str) -> Result<PathBuf> {
    let base = PathBuf::from(workspace).join(channel);
    let full = base.join(subpath);

    let canonical_base = base.canonicalize().unwrap_or(base.clone());
    let canonical_full = full.canonicalize().unwrap_or(full.clone());

    if !canonical_full.starts_with(&canonical_base) {
        bail!("Path traversal detected");
    }

    Ok(full)
}

pub async fn list_files(
    State(state): State<AppState>,
    session: Session,
    Path((channel, path)): Path<(String, String)>,
) -> impl IntoResponse {
    let user = get_current_user(&session).await;

    if user.is_none() {
        return Html(
            FilesTemplate {
                user: None,
                channel,
                path,
                entries: vec![],
                error: Some("Not authenticated".to_string()),
            }
            .render()
            .unwrap(),
        );
    }

    let validated = match validate_path(&state.config.workspace_path, &channel, &path) {
        Ok(p) => p,
        Err(e) => {
            return Html(
                FilesTemplate {
                    user,
                    channel,
                    path,
                    entries: vec![],
                    error: Some(e.to_string()),
                }
                .render()
                .unwrap(),
            );
        }
    };

    let entries = match std::fs::read_dir(&validated) {
        Ok(dir) => dir
            .filter_map(|e| e.ok())
            .map(|e| {
                let metadata = e.metadata().ok();
                FileEntry {
                    name: e.file_name().to_string_lossy().to_string(),
                    is_dir: e.file_type().map(|t| t.is_dir()).unwrap_or(false),
                    size: metadata.map(|m| m.len()).unwrap_or(0),
                }
            })
            .collect(),
        Err(e) => {
            return Html(
                FilesTemplate {
                    user,
                    channel,
                    path,
                    entries: vec![],
                    error: Some(e.to_string()),
                }
                .render()
                .unwrap(),
            );
        }
    };

    Html(
        FilesTemplate {
            user,
            channel,
            path,
            entries,
            error: None,
        }
        .render()
        .unwrap(),
    )
}
```

**Step 2: Create workstation/templates/files.html**

```html
{% extends "base.html" %}

{% block title %}Files - {{ channel }}{% endblock %}

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
<div class="space-y-6">
    <div class="flex items-center gap-2 text-gray-400">
        <a href="/" class="hover:text-white">Home</a>
        <span>/</span>
        <span class="text-white">{{ channel }}</span>
        {% if !path.is_empty() %}
        <span>/</span>
        <span class="text-white">{{ path }}</span>
        {% endif %}
    </div>

    {% match error %}
    {% when Some with (e) %}
    <div class="bg-red-900 border border-red-700 rounded p-4 text-red-200">
        {{ e }}
    </div>
    {% when None %}
    {% endmatch %}

    <div class="bg-gray-800 rounded-lg border border-gray-700 overflow-hidden">
        <table class="w-full">
            <thead class="bg-gray-700">
                <tr>
                    <th class="px-4 py-2 text-left">Name</th>
                    <th class="px-4 py-2 text-left">Size</th>
                    <th class="px-4 py-2 text-left">Actions</th>
                </tr>
            </thead>
            <tbody>
                {% for entry in entries %}
                <tr class="border-t border-gray-700 hover:bg-gray-750">
                    <td class="px-4 py-2">
                        {% if entry.is_dir %}
                        <a href="/files/{{ channel }}/{{ path }}{% if !path.is_empty() %}/{% endif %}{{ entry.name }}"
                           class="text-blue-400 hover:text-blue-300">
                            📁 {{ entry.name }}
                        </a>
                        {% else %}
                        <span>📄 {{ entry.name }}</span>
                        {% endif %}
                    </td>
                    <td class="px-4 py-2 text-gray-400">
                        {% if entry.is_dir %}-{% else %}{{ entry.size }} bytes{% endif %}
                    </td>
                    <td class="px-4 py-2">
                        {% if !entry.is_dir %}
                        <a href="/files/{{ channel }}/{{ path }}{% if !path.is_empty() %}/{% endif %}{{ entry.name }}/edit"
                           class="text-yellow-400 hover:text-yellow-300">Edit</a>
                        {% endif %}
                    </td>
                </tr>
                {% endfor %}
            </tbody>
        </table>
    </div>
</div>
{% endblock %}
```

**Step 3: Update workstation/src/lib.rs**

```rust
// ABOUTME: Workstation webapp library - user-facing config UI for gorp workspaces.
// ABOUTME: Provides file management, terminal access, and browser viewer.

pub mod auth;
pub mod config;
pub mod files;
pub mod routes;
pub mod templates;

use auth::OidcConfig;

#[derive(Clone)]
pub struct AppState {
    pub config: config::Config,
    pub oidc: OidcConfig,
}
```

**Step 4: Update workstation/src/routes.rs**

Add the files route:

```rust
// ABOUTME: Axum router setup for workstation webapp.
// ABOUTME: Defines all HTTP routes and middleware.

use askama::Template;
use axum::{
    response::{Html, IntoResponse},
    routing::get,
    Router,
};
use tower_sessions::{MemoryStore, Session, SessionManagerLayer};

use crate::{
    auth::{self, get_current_user},
    files,
    templates::IndexTemplate,
    AppState,
};

pub fn create_router(state: AppState) -> Router {
    let session_store = MemoryStore::default();
    let session_layer = SessionManagerLayer::new(session_store)
        .with_secure(false)
        .with_same_site(tower_sessions::cookie::SameSite::Lax);

    Router::new()
        .route("/", get(index))
        .route("/health", get(health))
        .route("/auth/login", get(auth::login))
        .route("/auth/callback", get(auth::callback))
        .route("/auth/logout", get(auth::logout))
        .route("/files/{channel}/{*path}", get(files::list_files))
        .layer(session_layer)
        .with_state(state)
}

async fn index(session: Session) -> impl IntoResponse {
    let user = get_current_user(&session).await;
    let template = IndexTemplate { user };
    Html(template.render().unwrap())
}

async fn health() -> &'static str {
    "ok"
}
```

**Step 5: Verify files route compiles**

```bash
cargo build -p workstation
```

Expected: BUILD SUCCESS

**Step 6: Commit**

```bash
git add -A
git commit -m "feat(workstation): add file listing"
```

---

## Task 7: File Management - Read and Edit

**Files:**
- Modify: `/workstation/src/files.rs`
- Create: `/workstation/templates/file_edit.html`
- Modify: `/workstation/src/routes.rs`

**Step 1: Add read/write to files.rs**

Add to the end of `/workstation/src/files.rs`:

```rust
#[derive(Template)]
#[template(path = "file_edit.html")]
pub struct FileEditTemplate {
    pub user: Option<String>,
    pub channel: String,
    pub path: String,
    pub content: String,
    pub error: Option<String>,
}

pub async fn read_file(
    State(state): State<AppState>,
    session: Session,
    Path((channel, path)): Path<(String, String)>,
) -> impl IntoResponse {
    let user = get_current_user(&session).await;

    if user.is_none() {
        return Html(
            FileEditTemplate {
                user: None,
                channel,
                path,
                content: String::new(),
                error: Some("Not authenticated".to_string()),
            }
            .render()
            .unwrap(),
        );
    }

    let validated = match validate_path(&state.config.workspace_path, &channel, &path) {
        Ok(p) => p,
        Err(e) => {
            return Html(
                FileEditTemplate {
                    user,
                    channel,
                    path,
                    content: String::new(),
                    error: Some(e.to_string()),
                }
                .render()
                .unwrap(),
            );
        }
    };

    let content = match std::fs::read_to_string(&validated) {
        Ok(c) => c,
        Err(e) => {
            return Html(
                FileEditTemplate {
                    user,
                    channel,
                    path,
                    content: String::new(),
                    error: Some(e.to_string()),
                }
                .render()
                .unwrap(),
            );
        }
    };

    Html(
        FileEditTemplate {
            user,
            channel,
            path,
            content,
            error: None,
        }
        .render()
        .unwrap(),
    )
}

#[derive(serde::Deserialize)]
pub struct SaveFileForm {
    pub content: String,
}

pub async fn save_file(
    State(state): State<AppState>,
    session: Session,
    Path((channel, path)): Path<(String, String)>,
    axum::Form(form): axum::Form<SaveFileForm>,
) -> impl IntoResponse {
    let user = get_current_user(&session).await;

    if user.is_none() {
        return Html(
            FileEditTemplate {
                user: None,
                channel,
                path,
                content: form.content,
                error: Some("Not authenticated".to_string()),
            }
            .render()
            .unwrap(),
        );
    }

    let validated = match validate_path(&state.config.workspace_path, &channel, &path) {
        Ok(p) => p,
        Err(e) => {
            return Html(
                FileEditTemplate {
                    user,
                    channel,
                    path,
                    content: form.content,
                    error: Some(e.to_string()),
                }
                .render()
                .unwrap(),
            );
        }
    };

    if let Err(e) = std::fs::write(&validated, &form.content) {
        return Html(
            FileEditTemplate {
                user,
                channel,
                path,
                content: form.content,
                error: Some(e.to_string()),
            }
            .render()
            .unwrap(),
        );
    }

    Html(
        FileEditTemplate {
            user,
            channel,
            path,
            content: form.content,
            error: None,
        }
        .render()
        .unwrap(),
    )
}
```

**Step 2: Create workstation/templates/file_edit.html**

```html
{% extends "base.html" %}

{% block title %}Edit - {{ path }}{% endblock %}

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
<div class="space-y-6">
    <div class="flex items-center gap-2 text-gray-400">
        <a href="/" class="hover:text-white">Home</a>
        <span>/</span>
        <a href="/files/{{ channel }}/" class="hover:text-white">{{ channel }}</a>
        <span>/</span>
        <span class="text-white">{{ path }}</span>
    </div>

    {% match error %}
    {% when Some with (e) %}
    <div class="bg-red-900 border border-red-700 rounded p-4 text-red-200">
        {{ e }}
    </div>
    {% when None %}
    {% endmatch %}

    <form method="POST" class="space-y-4">
        <textarea
            name="content"
            class="w-full h-96 bg-gray-800 text-gray-100 font-mono text-sm p-4 rounded border border-gray-700 focus:border-blue-500 focus:outline-none"
        >{{ content }}</textarea>

        <div class="flex gap-4">
            <button
                type="submit"
                class="bg-blue-600 hover:bg-blue-500 px-6 py-2 rounded font-medium"
            >
                Save
            </button>
            <a
                href="/files/{{ channel }}/"
                class="bg-gray-700 hover:bg-gray-600 px-6 py-2 rounded"
            >
                Cancel
            </a>
        </div>
    </form>
</div>
{% endblock %}
```

**Step 3: Update routes.rs with edit routes**

Add to router in `/workstation/src/routes.rs`:

```rust
.route("/files/{channel}/{*path}/edit", get(files::read_file).post(files::save_file))
```

Full routes.rs:

```rust
// ABOUTME: Axum router setup for workstation webapp.
// ABOUTME: Defines all HTTP routes and middleware.

use askama::Template;
use axum::{
    response::{Html, IntoResponse},
    routing::get,
    Router,
};
use tower_sessions::{MemoryStore, Session, SessionManagerLayer};

use crate::{
    auth::{self, get_current_user},
    files,
    templates::IndexTemplate,
    AppState,
};

pub fn create_router(state: AppState) -> Router {
    let session_store = MemoryStore::default();
    let session_layer = SessionManagerLayer::new(session_store)
        .with_secure(false)
        .with_same_site(tower_sessions::cookie::SameSite::Lax);

    Router::new()
        .route("/", get(index))
        .route("/health", get(health))
        .route("/auth/login", get(auth::login))
        .route("/auth/callback", get(auth::callback))
        .route("/auth/logout", get(auth::logout))
        .route("/files/{channel}/{*path}", get(files::list_files))
        .route(
            "/files/{channel}/{*path}/edit",
            get(files::read_file).post(files::save_file),
        )
        .layer(session_layer)
        .with_state(state)
}

async fn index(session: Session) -> impl IntoResponse {
    let user = get_current_user(&session).await;
    let template = IndexTemplate { user };
    Html(template.render().unwrap())
}

async fn health() -> &'static str {
    "ok"
}
```

**Step 4: Verify compiles**

```bash
cargo build -p workstation
```

Expected: BUILD SUCCESS

**Step 5: Commit**

```bash
git add -A
git commit -m "feat(workstation): add file read/edit"
```

---

## Task 8: Channel List from Gorp API

**Files:**
- Create: `/workstation/src/gorp_client.rs`
- Modify: `/workstation/src/lib.rs`
- Modify: `/workstation/src/routes.rs`
- Modify: `/workstation/src/templates.rs`
- Modify: `/workstation/templates/index.html`

**Step 1: Create workstation/src/gorp_client.rs**

```rust
// ABOUTME: HTTP client for communicating with gorp API.
// ABOUTME: Fetches channel data and proxies requests.

use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Channel {
    pub channel_name: String,
    pub room_id: String,
    pub session_id: String,
    pub directory: String,
    pub started: bool,
    pub created_at: String,
}

pub struct GorpClient {
    base_url: String,
    client: reqwest::Client,
}

impl GorpClient {
    pub fn new(base_url: &str) -> Self {
        Self {
            base_url: base_url.to_string(),
            client: reqwest::Client::new(),
        }
    }

    pub async fn list_channels(&self) -> Result<Vec<Channel>> {
        let url = format!("{}/api/channels", self.base_url);
        let response = self.client.get(&url).send().await?;

        if !response.status().is_success() {
            anyhow::bail!("Failed to fetch channels: {}", response.status());
        }

        let channels: Vec<Channel> = response.json().await?;
        Ok(channels)
    }
}
```

**Step 2: Update lib.rs**

```rust
// ABOUTME: Workstation webapp library - user-facing config UI for gorp workspaces.
// ABOUTME: Provides file management, terminal access, and browser viewer.

pub mod auth;
pub mod config;
pub mod files;
pub mod gorp_client;
pub mod routes;
pub mod templates;

use auth::OidcConfig;
use gorp_client::GorpClient;

#[derive(Clone)]
pub struct AppState {
    pub config: config::Config,
    pub oidc: OidcConfig,
    pub gorp: GorpClient,
}
```

**Step 3: Update main.rs**

```rust
// ABOUTME: Workstation webapp entry point - starts the Axum server.
// ABOUTME: Serves htmx UI for workspace configuration.

use anyhow::Result;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use workstation::{auth::OidcConfig, config::Config, gorp_client::GorpClient, AppState};

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "workstation=debug,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    tracing::info!("Starting workstation webapp");

    let config = Config::load()?;
    let oidc = OidcConfig::new(
        &config.matrix_homeserver,
        "workstation",
        &format!("http://localhost:{}/auth/callback", config.port),
    )?;
    let gorp = GorpClient::new(&config.gorp_api_url);

    let state = AppState {
        config: config.clone(),
        oidc,
        gorp,
    };

    let app = workstation::routes::create_router(state);

    let addr = format!("0.0.0.0:{}", config.port);
    tracing::info!("Listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
```

**Step 4: Update templates.rs**

```rust
// ABOUTME: Askama template definitions for workstation webapp.
// ABOUTME: Defines structs that map to HTML templates.

use askama::Template;

use crate::gorp_client::Channel;

#[derive(Template)]
#[template(path = "index.html")]
pub struct IndexTemplate {
    pub user: Option<String>,
    pub channels: Vec<Channel>,
    pub error: Option<String>,
}
```

**Step 5: Update routes.rs index handler**

```rust
// ABOUTME: Axum router setup for workstation webapp.
// ABOUTME: Defines all HTTP routes and middleware.

use askama::Template;
use axum::{
    extract::State,
    response::{Html, IntoResponse},
    routing::get,
    Router,
};
use tower_sessions::{MemoryStore, Session, SessionManagerLayer};

use crate::{
    auth::{self, get_current_user},
    files,
    templates::IndexTemplate,
    AppState,
};

pub fn create_router(state: AppState) -> Router {
    let session_store = MemoryStore::default();
    let session_layer = SessionManagerLayer::new(session_store)
        .with_secure(false)
        .with_same_site(tower_sessions::cookie::SameSite::Lax);

    Router::new()
        .route("/", get(index))
        .route("/health", get(health))
        .route("/auth/login", get(auth::login))
        .route("/auth/callback", get(auth::callback))
        .route("/auth/logout", get(auth::logout))
        .route("/files/{channel}/{*path}", get(files::list_files))
        .route(
            "/files/{channel}/{*path}/edit",
            get(files::read_file).post(files::save_file),
        )
        .layer(session_layer)
        .with_state(state)
}

async fn index(State(state): State<AppState>, session: Session) -> impl IntoResponse {
    let user = get_current_user(&session).await;

    let (channels, error) = if user.is_some() {
        match state.gorp.list_channels().await {
            Ok(c) => (c, None),
            Err(e) => (vec![], Some(e.to_string())),
        }
    } else {
        (vec![], None)
    };

    let template = IndexTemplate {
        user,
        channels,
        error,
    };
    Html(template.render().unwrap())
}

async fn health() -> &'static str {
    "ok"
}
```

**Step 6: Update index.html**

```html
{% extends "base.html" %}

{% block title %}Workstation - Dashboard{% endblock %}

{% block nav %}
{% match user %}
{% when Some with (u) %}
<span class="text-gray-400">{{ u }}</span>
<a href="/auth/logout" class="text-red-400 hover:text-red-300">Logout</a>
{% when None %}
<a href="/auth/login" class="bg-blue-600 hover:bg-blue-500 px-4 py-2 rounded">Login with Matrix</a>
{% endmatch %}
{% endblock %}

{% block content %}
<div class="space-y-6">
    <h1 class="text-2xl font-bold">Welcome to Workstation</h1>
    <p class="text-gray-400">Configure your gorp workspaces.</p>

    {% match error %}
    {% when Some with (e) %}
    <div class="bg-red-900 border border-red-700 rounded p-4 text-red-200">
        {{ e }}
    </div>
    {% when None %}
    {% endmatch %}

    {% match user %}
    {% when Some with (_u) %}
    <div class="bg-gray-800 rounded-lg p-6 border border-gray-700">
        <h2 class="text-lg font-semibold mb-4">Your Channels</h2>
        {% if channels.is_empty() %}
        <p class="text-gray-500">No channels found. Create one in Matrix with !create &lt;name&gt;</p>
        {% else %}
        <div class="grid gap-4 md:grid-cols-2 lg:grid-cols-3">
            {% for channel in channels %}
            <a href="/files/{{ channel.channel_name }}/"
               class="block bg-gray-700 hover:bg-gray-600 rounded-lg p-4 transition">
                <h3 class="font-medium text-blue-400">{{ channel.channel_name }}</h3>
                <p class="text-sm text-gray-400 mt-1">Created: {{ channel.created_at }}</p>
            </a>
            {% endfor %}
        </div>
        {% endif %}
    </div>
    {% when None %}
    <div class="bg-gray-800 rounded-lg p-6 border border-gray-700">
        <h2 class="text-lg font-semibold mb-4">Get Started</h2>
        <p class="text-gray-500">Login with your Matrix account to see your channels.</p>
    </div>
    {% endmatch %}
</div>
{% endblock %}
```

**Step 7: Verify compiles**

```bash
cargo build -p workstation
```

Expected: BUILD SUCCESS

**Step 8: Commit**

```bash
git add -A
git commit -m "feat(workstation): add channel list from gorp API"
```

---

## Task 9: Add Channel List API to Gorp

**Files:**
- Modify: `/gorp/src/admin/routes.rs`

**Step 1: Add API route to gorp**

Add to `/gorp/src/admin/routes.rs` router:

```rust
.route("/api/channels", get(api_list_channels))
```

**Step 2: Add API handler**

Add handler function:

```rust
async fn api_list_channels(
    State(state): State<AdminState>,
) -> impl IntoResponse {
    match state.session_store.list_channels() {
        Ok(channels) => axum::Json(channels).into_response(),
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            e.to_string(),
        )
            .into_response(),
    }
}
```

**Step 3: Add CORS for API**

Update gorp's admin router to include CORS middleware for API routes.

**Step 4: Verify gorp compiles**

```bash
cargo build -p gorp
```

Expected: BUILD SUCCESS

**Step 5: Commit**

```bash
git add -A
git commit -m "feat(gorp): add /api/channels endpoint"
```

---

## Phase 1 Complete

At this point you have:
- Cargo workspace with gorp and workstation crates
- Basic workstation webapp with htmx UI
- Matrix OIDC auth scaffolding
- File listing and editing
- Channel list from gorp API

Phase 2 will add:
- Terminal WebSocket + xterm.js
- Browser viewer CDP streaming
- Cloudflare Tunnel integration

---

## Execution Notes

- Test each task manually before committing
- If gorp API doesn't exist yet, mock it or skip channel list until Task 9
- Adjust paths if workspace structure differs
- Matrix OIDC may need tweaking for your specific homeserver
