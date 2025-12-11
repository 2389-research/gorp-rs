# Admin Panel Phase 1 Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Build a web-based config management interface for gorp at `/admin/*`

**Architecture:** Add admin routes to existing axum server. Use Askama for compile-time templates with HTMX for dynamic updates. Simple auth via API key or localhost-only access.

**Tech Stack:** Axum, Askama, HTMX (CDN), Tailwind CSS (CDN), tower-sessions

---

### Task 1: Add Dependencies

**Files:**
- Modify: `Cargo.toml`

**Step 1: Add new dependencies**

Add to `[dependencies]` section in `Cargo.toml`:

```toml
askama = "0.12"
askama_axum = "0.4"
tower-sessions = { version = "0.13", features = ["memory-store"] }
```

**Step 2: Verify dependencies resolve**

Run: `cargo check`
Expected: Compiles without errors

**Step 3: Commit**

```bash
git add Cargo.toml Cargo.lock
git commit -m "chore: add askama and tower-sessions dependencies"
```

---

### Task 2: Create Admin Module Structure

**Files:**
- Create: `src/admin/mod.rs`
- Create: `src/admin/routes.rs`
- Create: `src/admin/templates.rs`
- Modify: `src/lib.rs`

**Step 1: Create admin module directory**

```bash
mkdir -p src/admin
```

**Step 2: Create mod.rs**

Create `src/admin/mod.rs`:

```rust
// ABOUTME: Admin panel module for web-based configuration management
// ABOUTME: Provides routes at /admin/* for config viewing and editing

pub mod routes;
pub mod templates;

pub use routes::admin_router;
```

**Step 3: Create placeholder routes.rs**

Create `src/admin/routes.rs`:

```rust
// ABOUTME: Admin panel route handlers
// ABOUTME: Handles config viewing, editing, and session management

use axum::{routing::get, Router};

use crate::admin::templates::DashboardTemplate;

/// Build the admin router mounted at /admin
pub fn admin_router() -> Router {
    Router::new()
        .route("/", get(dashboard))
}

async fn dashboard() -> DashboardTemplate {
    DashboardTemplate {
        title: "gorp Admin".to_string(),
    }
}
```

**Step 4: Create placeholder templates.rs**

Create `src/admin/templates.rs`:

```rust
// ABOUTME: Askama template structs for admin panel
// ABOUTME: Templates are compiled into binary at build time

use askama::Template;

#[derive(Template)]
#[template(path = "admin/dashboard.html")]
pub struct DashboardTemplate {
    pub title: String,
}
```

**Step 5: Add admin module to lib.rs**

Add to `src/lib.rs` after other module declarations:

```rust
pub mod admin;
```

**Step 6: Commit**

```bash
git add src/admin src/lib.rs
git commit -m "feat: create admin module structure"
```

---

### Task 3: Create Base Templates

**Files:**
- Create: `templates/base.html`
- Create: `templates/admin/dashboard.html`

**Step 1: Create templates directory**

```bash
mkdir -p templates/admin
```

**Step 2: Create base.html**

Create `templates/base.html`:

```html
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>{% block title %}gorp Admin{% endblock %}</title>
    <script src="https://cdn.tailwindcss.com"></script>
    <script src="https://unpkg.com/htmx.org@1.9.10"></script>
</head>
<body class="bg-gray-100 min-h-screen">
    <nav class="bg-gray-800 text-white p-4">
        <div class="container mx-auto flex justify-between items-center">
            <a href="/admin" class="text-xl font-bold">gorp</a>
            <div class="space-x-4">
                <a href="/admin" class="hover:text-gray-300">Dashboard</a>
                <a href="/admin/config" class="hover:text-gray-300">Config</a>
                <a href="/admin/channels" class="hover:text-gray-300">Channels</a>
            </div>
        </div>
    </nav>

    <main class="container mx-auto p-6">
        {% block content %}{% endblock %}
    </main>

    <div id="toast" class="fixed bottom-4 right-4"></div>
</body>
</html>
```

**Step 3: Create dashboard.html**

Create `templates/admin/dashboard.html`:

```html
{% extends "base.html" %}

{% block title %}{{ title }}{% endblock %}

{% block content %}
<div class="bg-white rounded-lg shadow p-6">
    <h1 class="text-2xl font-bold mb-4">Dashboard</h1>
    <p class="text-gray-600">Welcome to the gorp admin panel.</p>

    <div class="grid grid-cols-1 md:grid-cols-3 gap-4 mt-6">
        <a href="/admin/config" class="block p-4 bg-blue-50 rounded-lg hover:bg-blue-100">
            <h2 class="font-semibold text-blue-800">Configuration</h2>
            <p class="text-sm text-blue-600">View and edit settings</p>
        </a>
        <a href="/admin/channels" class="block p-4 bg-green-50 rounded-lg hover:bg-green-100">
            <h2 class="font-semibold text-green-800">Channels</h2>
            <p class="text-sm text-green-600">Manage Claude channels</p>
        </a>
        <a href="/admin/health" class="block p-4 bg-purple-50 rounded-lg hover:bg-purple-100">
            <h2 class="font-semibold text-purple-800">Health</h2>
            <p class="text-sm text-purple-600">Connection status</p>
        </a>
    </div>
</div>
{% endblock %}
```

**Step 4: Commit**

```bash
git add templates/
git commit -m "feat: add base HTML templates with Tailwind and HTMX"
```

---

### Task 4: Mount Admin Router to Webhook Server

**Files:**
- Modify: `src/webhook.rs`

**Step 1: Import admin router**

Add to imports at top of `src/webhook.rs`:

```rust
use crate::admin::admin_router;
```

**Step 2: Mount admin router**

In `start_webhook_server` function, modify the Router construction. Find:

```rust
let app = Router::new()
```

And change to:

```rust
let app = Router::new()
    .nest("/admin", admin_router())
```

**Step 3: Build and verify**

Run: `cargo build`
Expected: Compiles without errors

**Step 4: Commit**

```bash
git add src/webhook.rs
git commit -m "feat: mount admin router at /admin"
```

---

### Task 5: Add Config View Template and Route

**Files:**
- Create: `templates/admin/config.html`
- Modify: `src/admin/templates.rs`
- Modify: `src/admin/routes.rs`

**Step 1: Create ConfigTemplate struct**

Add to `src/admin/templates.rs`:

```rust
#[derive(Template)]
#[template(path = "admin/config.html")]
pub struct ConfigTemplate {
    pub title: String,
    pub home_server: String,
    pub user_id: String,
    pub device_name: String,
    pub room_prefix: String,
    pub allowed_users: String,
    pub webhook_port: u16,
    pub webhook_host: String,
    pub webhook_api_key_set: bool,
    pub workspace_path: String,
    pub scheduler_timezone: String,
    pub password_set: bool,
    pub access_token_set: bool,
    pub recovery_key_set: bool,
}
```

**Step 2: Create config.html template**

Create `templates/admin/config.html`:

```html
{% extends "base.html" %}

{% block title %}{{ title }}{% endblock %}

{% block content %}
<div class="bg-white rounded-lg shadow p-6">
    <h1 class="text-2xl font-bold mb-6">Configuration</h1>

    <form hx-post="/admin/config/save" hx-target="#toast" hx-swap="innerHTML" class="space-y-6">

        <!-- Matrix Section -->
        <div class="border-b pb-4">
            <h2 class="text-lg font-semibold mb-4 text-gray-700">Matrix</h2>
            <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
                <div>
                    <label class="block text-sm font-medium text-gray-700">Home Server</label>
                    <input type="text" name="home_server" value="{{ home_server }}"
                           class="mt-1 block w-full rounded-md border-gray-300 shadow-sm focus:border-blue-500 focus:ring-blue-500 p-2 border">
                </div>
                <div>
                    <label class="block text-sm font-medium text-gray-700">User ID</label>
                    <input type="text" name="user_id" value="{{ user_id }}"
                           class="mt-1 block w-full rounded-md border-gray-300 shadow-sm focus:border-blue-500 focus:ring-blue-500 p-2 border">
                </div>
                <div>
                    <label class="block text-sm font-medium text-gray-700">Device Name</label>
                    <input type="text" name="device_name" value="{{ device_name }}"
                           class="mt-1 block w-full rounded-md border-gray-300 shadow-sm focus:border-blue-500 focus:ring-blue-500 p-2 border">
                </div>
                <div>
                    <label class="block text-sm font-medium text-gray-700">Room Prefix</label>
                    <input type="text" name="room_prefix" value="{{ room_prefix }}"
                           class="mt-1 block w-full rounded-md border-gray-300 shadow-sm focus:border-blue-500 focus:ring-blue-500 p-2 border">
                </div>
                <div class="md:col-span-2">
                    <label class="block text-sm font-medium text-gray-700">Allowed Users (comma-separated)</label>
                    <input type="text" name="allowed_users" value="{{ allowed_users }}"
                           class="mt-1 block w-full rounded-md border-gray-300 shadow-sm focus:border-blue-500 focus:ring-blue-500 p-2 border">
                </div>
            </div>
            <div class="mt-4 grid grid-cols-1 md:grid-cols-3 gap-4">
                <div class="flex items-center space-x-2">
                    <span class="text-sm text-gray-600">Password:</span>
                    {% if password_set %}
                    <span class="text-green-600 text-sm">✓ Configured</span>
                    {% else %}
                    <span class="text-yellow-600 text-sm">Not set</span>
                    {% endif %}
                </div>
                <div class="flex items-center space-x-2">
                    <span class="text-sm text-gray-600">Access Token:</span>
                    {% if access_token_set %}
                    <span class="text-green-600 text-sm">✓ Configured</span>
                    {% else %}
                    <span class="text-yellow-600 text-sm">Not set</span>
                    {% endif %}
                </div>
                <div class="flex items-center space-x-2">
                    <span class="text-sm text-gray-600">Recovery Key:</span>
                    {% if recovery_key_set %}
                    <span class="text-green-600 text-sm">✓ Configured</span>
                    {% else %}
                    <span class="text-yellow-600 text-sm">Not set</span>
                    {% endif %}
                </div>
            </div>
        </div>

        <!-- Webhook Section -->
        <div class="border-b pb-4">
            <h2 class="text-lg font-semibold mb-4 text-gray-700">Webhook</h2>
            <div class="grid grid-cols-1 md:grid-cols-3 gap-4">
                <div>
                    <label class="block text-sm font-medium text-gray-700">Port</label>
                    <input type="number" name="webhook_port" value="{{ webhook_port }}"
                           class="mt-1 block w-full rounded-md border-gray-300 shadow-sm focus:border-blue-500 focus:ring-blue-500 p-2 border">
                </div>
                <div>
                    <label class="block text-sm font-medium text-gray-700">Host</label>
                    <input type="text" name="webhook_host" value="{{ webhook_host }}"
                           class="mt-1 block w-full rounded-md border-gray-300 shadow-sm focus:border-blue-500 focus:ring-blue-500 p-2 border">
                </div>
                <div class="flex items-center">
                    <span class="text-sm text-gray-600">API Key:</span>
                    {% if webhook_api_key_set %}
                    <span class="text-green-600 text-sm ml-2">✓ Configured</span>
                    {% else %}
                    <span class="text-yellow-600 text-sm ml-2">Not set (localhost only)</span>
                    {% endif %}
                </div>
            </div>
        </div>

        <!-- Workspace Section -->
        <div class="border-b pb-4">
            <h2 class="text-lg font-semibold mb-4 text-gray-700">Workspace</h2>
            <div>
                <label class="block text-sm font-medium text-gray-700">Path</label>
                <input type="text" name="workspace_path" value="{{ workspace_path }}"
                       class="mt-1 block w-full rounded-md border-gray-300 shadow-sm focus:border-blue-500 focus:ring-blue-500 p-2 border">
            </div>
        </div>

        <!-- Scheduler Section -->
        <div class="pb-4">
            <h2 class="text-lg font-semibold mb-4 text-gray-700">Scheduler</h2>
            <div>
                <label class="block text-sm font-medium text-gray-700">Timezone</label>
                <input type="text" name="scheduler_timezone" value="{{ scheduler_timezone }}"
                       class="mt-1 block w-full rounded-md border-gray-300 shadow-sm focus:border-blue-500 focus:ring-blue-500 p-2 border"
                       placeholder="America/Chicago">
            </div>
        </div>

        <div class="flex justify-between items-center">
            <p class="text-sm text-yellow-600">⚠️ Some changes require restart to take effect</p>
            <button type="submit"
                    class="bg-blue-600 text-white px-6 py-2 rounded-md hover:bg-blue-700 focus:outline-none focus:ring-2 focus:ring-blue-500 focus:ring-offset-2">
                Save Changes
            </button>
        </div>
    </form>
</div>
{% endblock %}
```

**Step 3: Add config route**

Add to `src/admin/routes.rs`. First add imports:

```rust
use axum::{routing::get, Router, extract::State};
use std::sync::Arc;
use crate::config::Config;
use crate::admin::templates::{DashboardTemplate, ConfigTemplate};
```

Add the route in `admin_router`:

```rust
pub fn admin_router() -> Router<AdminState> {
    Router::new()
        .route("/", get(dashboard))
        .route("/config", get(config_view))
}

#[derive(Clone)]
pub struct AdminState {
    pub config: Arc<Config>,
}
```

Add the handler:

```rust
async fn config_view(State(state): State<AdminState>) -> ConfigTemplate {
    let config = &state.config;
    ConfigTemplate {
        title: "Configuration - gorp Admin".to_string(),
        home_server: config.matrix.home_server.clone(),
        user_id: config.matrix.user_id.clone(),
        device_name: config.matrix.device_name.clone(),
        room_prefix: config.matrix.room_prefix.clone(),
        allowed_users: config.matrix.allowed_users.join(", "),
        webhook_port: config.webhook.port,
        webhook_host: config.webhook.host.clone(),
        webhook_api_key_set: config.webhook.api_key.is_some(),
        workspace_path: config.workspace.path.clone(),
        scheduler_timezone: config.scheduler.timezone.clone(),
        password_set: config.matrix.password.is_some(),
        access_token_set: config.matrix.access_token.is_some(),
        recovery_key_set: config.matrix.recovery_key.is_some(),
    }
}
```

**Step 4: Build and verify**

Run: `cargo build`
Expected: Compiles without errors

**Step 5: Commit**

```bash
git add src/admin/ templates/admin/config.html
git commit -m "feat: add config view page"
```

---

### Task 6: Add Config Save Endpoint

**Files:**
- Create: `templates/partials/toast.html`
- Modify: `src/admin/routes.rs`
- Modify: `src/admin/templates.rs`

**Step 1: Create ToastTemplate**

Add to `src/admin/templates.rs`:

```rust
#[derive(Template)]
#[template(path = "partials/toast.html")]
pub struct ToastTemplate {
    pub message: String,
    pub is_error: bool,
}
```

**Step 2: Create toast.html**

Create `templates/partials/toast.html`:

```html
<div class="{% if is_error %}bg-red-500{% else %}bg-green-500{% endif %} text-white px-6 py-3 rounded-lg shadow-lg"
     x-data="{ show: true }"
     x-show="show"
     x-init="setTimeout(() => show = false, 3000)">
    {{ message }}
</div>
```

**Step 3: Add save endpoint to routes**

Add imports to `src/admin/routes.rs`:

```rust
use axum::{routing::{get, post}, Form};
use serde::Deserialize;
```

Add form struct:

```rust
#[derive(Deserialize)]
pub struct ConfigForm {
    pub home_server: String,
    pub user_id: String,
    pub device_name: String,
    pub room_prefix: String,
    pub allowed_users: String,
    pub webhook_port: u16,
    pub webhook_host: String,
    pub workspace_path: String,
    pub scheduler_timezone: String,
}
```

Add route:

```rust
pub fn admin_router() -> Router<AdminState> {
    Router::new()
        .route("/", get(dashboard))
        .route("/config", get(config_view))
        .route("/config/save", post(config_save))
}
```

Add handler:

```rust
use crate::admin::templates::{DashboardTemplate, ConfigTemplate, ToastTemplate};
use crate::paths;

async fn config_save(
    State(state): State<AdminState>,
    Form(form): Form<ConfigForm>,
) -> ToastTemplate {
    // Parse allowed_users from comma-separated string
    let allowed_users: Vec<String> = form
        .allowed_users
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    // Build new config preserving secrets from current config
    let mut new_config = (*state.config).clone();
    new_config.matrix.home_server = form.home_server;
    new_config.matrix.user_id = form.user_id;
    new_config.matrix.device_name = form.device_name;
    new_config.matrix.room_prefix = form.room_prefix;
    new_config.matrix.allowed_users = allowed_users;
    new_config.webhook.port = form.webhook_port;
    new_config.webhook.host = form.webhook_host;
    new_config.workspace.path = form.workspace_path;
    new_config.scheduler.timezone = form.scheduler_timezone;

    // Serialize to TOML
    let toml_str = match toml::to_string_pretty(&new_config) {
        Ok(s) => s,
        Err(e) => {
            return ToastTemplate {
                message: format!("Failed to serialize config: {}", e),
                is_error: true,
            };
        }
    };

    // Write to config file
    let config_path = paths::config_file();
    if let Err(e) = std::fs::write(&config_path, toml_str) {
        return ToastTemplate {
            message: format!("Failed to save config: {}", e),
            is_error: true,
        };
    }

    ToastTemplate {
        message: "Configuration saved! Restart required for some changes.".to_string(),
        is_error: false,
    }
}
```

**Step 4: Build and verify**

Run: `cargo build`
Expected: Compiles without errors

**Step 5: Commit**

```bash
git add src/admin/ templates/partials/
git commit -m "feat: add config save endpoint with toast notifications"
```

---

### Task 7: Wire Up AdminState in Webhook Server

**Files:**
- Modify: `src/webhook.rs`
- Modify: `src/admin/routes.rs`

**Step 1: Update webhook.rs to pass state**

In `src/webhook.rs`, modify the admin router mounting:

```rust
use crate::admin::{admin_router, AdminState};

// In start_webhook_server function, create admin state:
let admin_state = AdminState {
    config: Arc::clone(&config),
};

let app = Router::new()
    .nest("/admin", admin_router().with_state(admin_state))
    .route("/webhook/session/:session_id", post(webhook_handler))
    .layer(TraceLayer::new_for_http())
    .with_state(state);
```

**Step 2: Export AdminState from admin/mod.rs**

Update `src/admin/mod.rs`:

```rust
pub mod routes;
pub mod templates;

pub use routes::{admin_router, AdminState};
```

**Step 3: Build and test**

Run: `cargo build && cargo run`

Visit: `http://localhost:13000/admin`
Expected: Dashboard page loads with navigation

Visit: `http://localhost:13000/admin/config`
Expected: Config page loads with current values

**Step 4: Commit**

```bash
git add src/webhook.rs src/admin/
git commit -m "feat: wire up admin state to webhook server"
```

---

### Task 8: Add Simple Auth Middleware

**Files:**
- Create: `src/admin/auth.rs`
- Modify: `src/admin/mod.rs`
- Modify: `src/admin/routes.rs`

**Step 1: Create auth.rs**

Create `src/admin/auth.rs`:

```rust
// ABOUTME: Simple authentication for admin panel
// ABOUTME: Uses API key or allows localhost access if no key configured

use axum::{
    extract::{ConnectInfo, Request, State},
    http::StatusCode,
    middleware::Next,
    response::Response,
};
use std::net::SocketAddr;

use super::AdminState;

pub async fn auth_middleware(
    State(state): State<AdminState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let api_key = &state.config.webhook.api_key;

    // If no API key configured, only allow localhost
    if api_key.is_none() {
        let is_localhost = addr.ip().is_loopback();
        if !is_localhost {
            tracing::warn!(remote_addr = %addr, "Admin access denied: no API key and not localhost");
            return Err(StatusCode::FORBIDDEN);
        }
        return Ok(next.run(request).await);
    }

    // Check for API key in query params or header
    let uri = request.uri();
    let query = uri.query().unwrap_or("");
    let has_valid_key = query.contains(&format!("key={}", api_key.as_ref().unwrap()))
        || request
            .headers()
            .get("X-API-Key")
            .and_then(|v| v.to_str().ok())
            == api_key.as_deref();

    if has_valid_key {
        Ok(next.run(request).await)
    } else {
        tracing::warn!(remote_addr = %addr, "Admin access denied: invalid API key");
        Err(StatusCode::UNAUTHORIZED)
    }
}
```

**Step 2: Update mod.rs**

Update `src/admin/mod.rs`:

```rust
pub mod auth;
pub mod routes;
pub mod templates;

pub use routes::{admin_router, AdminState};
```

**Step 3: Apply middleware to routes**

Update `src/admin/routes.rs`:

```rust
use axum::middleware;
use super::auth::auth_middleware;

pub fn admin_router() -> Router<AdminState> {
    Router::new()
        .route("/", get(dashboard))
        .route("/config", get(config_view))
        .route("/config/save", post(config_save))
        .layer(middleware::from_fn_with_state(AdminState::default(), auth_middleware))
}
```

Wait - AdminState doesn't implement Default. Let's fix that:

Add to routes.rs:

```rust
impl Default for AdminState {
    fn default() -> Self {
        panic!("AdminState must be provided, not defaulted")
    }
}
```

Actually, better approach - pass state through middleware setup in webhook.rs.

**Step 4: Revise approach - apply middleware at mount point**

Update `src/webhook.rs` instead:

```rust
use axum::middleware;
use crate::admin::{admin_router, AdminState, auth::auth_middleware};

// In start_webhook_server:
let admin_state = AdminState {
    config: Arc::clone(&config),
};

let admin_routes = admin_router()
    .layer(middleware::from_fn_with_state(admin_state.clone(), auth_middleware))
    .with_state(admin_state);

let app = Router::new()
    .nest("/admin", admin_routes)
    // ... rest
```

**Step 5: Export auth module**

Update `src/admin/mod.rs`:

```rust
pub mod auth;
pub mod routes;
pub mod templates;

pub use auth::auth_middleware;
pub use routes::{admin_router, AdminState};
```

**Step 6: Build and test**

Run: `cargo build`
Expected: Compiles without errors

**Step 7: Commit**

```bash
git add src/admin/
git commit -m "feat: add simple auth middleware for admin panel"
```

---

### Task 9: Integration Test

**Files:**
- None (manual testing)

**Step 1: Run the server**

```bash
cargo run
```

**Step 2: Test dashboard**

Visit: `http://localhost:13000/admin`
Expected: Dashboard page with cards for Config, Channels, Health

**Step 3: Test config view**

Visit: `http://localhost:13000/admin/config`
Expected: Config form populated with current values

**Step 4: Test config save**

Modify a value (e.g., room_prefix) and click "Save Changes"
Expected: Green toast notification appears

**Step 5: Verify config file updated**

```bash
cat ~/.config/gorp/config.toml | grep room_prefix
```
Expected: Shows the modified value

**Step 6: Final commit**

```bash
git add -A
git commit -m "feat: complete admin panel phase 1 - config management"
```

---

## Summary

Phase 1 delivers:
- ✅ Dashboard at `/admin`
- ✅ Config view at `/admin/config`
- ✅ Config edit with form submission
- ✅ TOML save to config file
- ✅ Toast notifications for feedback
- ✅ Simple auth (localhost or API key)
- ✅ Tailwind + HTMX styling
