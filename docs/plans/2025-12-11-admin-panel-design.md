# Admin Panel Design Specification

## Overview

A full-featured admin dashboard for gorp providing configuration management, channel oversight, monitoring, and markdown document browsing through an HTMX-powered web interface.

## Architecture

```
┌─────────────────────────────────────────────────┐
│                   Axum Server                    │
│                  (port 13000)                    │
├─────────────────┬───────────────────────────────┤
│  /webhook/*     │  /admin/*                     │
│  (existing)     │  (admin panel)                │
│                 │                               │
│                 │  ┌─────────────────────────┐  │
│                 │  │  Matrix Auth Middleware │  │
│                 │  └─────────────────────────┘  │
│                 │  ┌─────────────────────────┐  │
│                 │  │  HTMX + Askama Templates│  │
│                 │  └─────────────────────────┘  │
└─────────────────┴───────────────────────────────┘
```

## Key Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Authentication | Matrix (allowed_users) | Reuses existing whitelist |
| UI Framework | HTMX + Askama templates | Dynamic UX without build complexity |
| CSS | Tailwind CDN | Zero build step, full utility classes |
| Port | Same as webhooks (13000) | Single port to manage |
| Templates | Embedded in binary | Single-file deployment |

## Phased Roadmap

### Phase 1: Config Management (MVP)
- Base template with nav, Tailwind styling
- Session middleware (cookie-based)
- Auth gate (webhook API key or localhost)
- Config view page (secrets redacted)
- Config edit form with validation
- Toast notifications for feedback

### Phase 2: Channel Management
- List all channels with status indicators
- Create/delete channels from UI
- View session info, webhook URLs
- Toggle debug mode per channel
- Channel detail pages

### Phase 3: Monitoring & Logs
- Connection health dashboard
- Schedule status (active, next run times)
- Live log viewer (tail recent entries)
- Error alerts and notifications

### Phase 4: Full Dashboard
- Analytics (messages/day, active channels)
- Message history viewer
- Real-time status via SSE
- Schedule management UI (create/edit/delete)

### Phase 5: Markdown Browser
- Browse workspace directories
- Render markdown files with styling
- Navigate `.matrix/` logs
- Search across workspaces

## URL Structure

```
/admin                     → Dashboard home
/admin/login               → Auth flow
/admin/logout              → Clear session

/admin/config              → View/edit configuration
/admin/config/save         → POST: save config changes

/admin/channels            → List all channels
/admin/channels/:name      → Channel detail view
/admin/channels/:name/debug → POST: toggle debug mode

/admin/schedules           → View all schedules
/admin/logs                → Log viewer
/admin/health              → Connection status

/admin/browse              → Workspace file browser
/admin/browse/*path        → View file/directory
/admin/render/*path        → Render markdown file
```

## Technical Stack

### Dependencies

```toml
askama = "0.12"           # Compile-time templates
askama_axum = "0.4"       # Axum integration
tower-sessions = "0.13"   # Session management
```

### Template Structure

```
templates/
├── base.html             # Layout with nav, HTMX/Tailwind
├── admin/
│   ├── login.html
│   ├── dashboard.html
│   ├── config.html
│   ├── channels/
│   │   ├── list.html
│   │   └── detail.html
│   ├── schedules.html
│   ├── logs.html
│   └── browse/
│       ├── directory.html
│       └── file.html
└── partials/
    ├── toast.html
    ├── status_badge.html
    └── channel_row.html
```

### Code Organization

```
src/
├── admin/
│   ├── mod.rs           # Router setup, middleware
│   ├── auth.rs          # Auth, sessions
│   ├── config.rs        # Config handlers
│   ├── channels.rs      # Channel management
│   ├── browse.rs        # File browser
│   └── templates.rs     # Askama structs
└── webhook.rs           # Mount admin router here
```

## Phase 1 Details

### Authentication (Simplified)

For Phase 1, use a simple approach:
- If `webhook.api_key` is configured, require it as password
- If not configured, allow localhost access without auth
- Full Matrix OAuth deferred to Phase 1.5

### Config Fields

**Editable:**
- Matrix: home_server, user_id, device_name, room_prefix, allowed_users
- Webhook: port, host, api_key
- Workspace: path
- Scheduler: timezone

**Display only (security):**
- password (shows "configured" or "not set")
- access_token (shows "configured" or "not set")
- recovery_key (shows "configured" or "not set")

### Restart Handling

Some config changes require restart:
- home_server, user_id, password, access_token, recovery_key

UI displays warning: "Restart required for changes to take effect."

### HTMX Patterns

```html
<!-- Form submission with feedback -->
<form hx-post="/admin/config/save"
      hx-target="#toast"
      hx-swap="innerHTML">
  ...
</form>

<!-- Live status polling -->
<div hx-get="/admin/health"
     hx-trigger="every 5s"
     hx-swap="innerHTML">
</div>
```

## Static Assets

```html
<!-- Tailwind via CDN -->
<script src="https://cdn.tailwindcss.com"></script>

<!-- HTMX via CDN -->
<script src="https://unpkg.com/htmx.org@1.9"></script>
```

Future optimization: pre-compile Tailwind CSS for smaller payload.

## Security Considerations

1. **Secrets never sent to browser** - Password/token fields show status only
2. **CSRF protection** - Use tower-sessions with secure cookies
3. **Path traversal** - Validate all file paths in browse endpoints
4. **Auth required** - All /admin/* routes except /admin/login protected

## Success Criteria

### Phase 1 Complete When:
- [ ] Can view current config in browser
- [ ] Can edit non-sensitive config values
- [ ] Changes persist to config.toml
- [ ] Toast shows success/error feedback
- [ ] Works on localhost without auth (if no api_key)
- [ ] Works with api_key auth when configured
