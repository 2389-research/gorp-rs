# Workstation Web App - Current Status

## Overview

The workstation is a web-based interface for interacting with gorp channels. It provides:
- Matrix OIDC authentication (login with your Matrix account)
- Channel browsing and file management
- Web-based terminal (PTY via WebSocket)
- Browser viewer (Chrome CDP screenshots via WebSocket)

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                    Cloudflare Tunnel                        │
│              (public HTTPS URL for OIDC)                    │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                      Workstation                            │
│                    (localhost:8088)                         │
│                                                             │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────┐ │
│  │ OIDC Auth   │  │ File Browser│  │ Gorp Proxy          │ │
│  │ (Matrix)    │  │ /files/*    │  │ /gorp/api/*         │ │
│  │ /auth/*     │  │ /edit/*     │  │ /gorp/ws/*          │ │
│  └─────────────┘  └─────────────┘  └─────────────────────┘ │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼ (localhost only)
┌─────────────────────────────────────────────────────────────┐
│                         Gorp                                │
│                    (localhost:13000)                        │
│                                                             │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────┐ │
│  │ Admin Panel │  │ Terminal    │  │ Browser CDP         │ │
│  │ /admin/*    │  │ /admin/ws/  │  │ /admin/ws/browser/* │ │
│  │             │  │ terminal/*  │  │                     │ │
│  └─────────────┘  └─────────────┘  └─────────────────────┘ │
└─────────────────────────────────────────────────────────────┘
```

## Implementation Status

### Phase 1: Basic Structure ✅
- Axum web server with session management
- Askama templates with Tailwind CSS
- Basic routing and health checks

### Phase 2: Terminal & Browser ✅
- Terminal page with xterm.js
- Browser viewer with click overlay
- WebSocket proxying through workstation to gorp
- PTY management in gorp (portable-pty)
- Chrome CDP integration in gorp (chromiumoxide)

### Phase 3: Matrix OIDC Authentication ✅ (with caveats)
- Dynamic OIDC client registration with account.matrix.org
- PKCE authorization code flow
- Token exchange and userinfo fetching
- SQLite session storage (in-memory fallback)
- Displays Matrix user ID after login

## Current Issues

### 1. Gorp API Compatibility
The workstation expects `/admin/api/channels` endpoint in gorp, but older gorp versions don't have this route. The endpoint was added to support the channel list on the homepage.

**Workaround:** Run gorp from the same worktree as workstation, or add the `api_list_channels` route to the other gorp.

### 2. SQLite Session Storage
File-based SQLite sessions fail with "unable to open database file" when using `tokio_rusqlite`. Currently using in-memory storage as a workaround (sessions lost on restart).

**Location:** `workstation/src/main.rs` line ~47

### 3. Remote Access Requires Tunnel
The workstation proxies requests to gorp at `localhost:13000`. When accessing remotely via Cloudflare Tunnel, this works because the proxy runs server-side. However, the OIDC redirect_uri must match the tunnel URL.

**Config:** `.env` file contains `OIDC_REDIRECT_URI` which must match current tunnel URL.

## Key Files

### Workstation
- `workstation/src/main.rs` - Entry point, session store setup
- `workstation/src/routes.rs` - All HTTP routes including gorp proxy
- `workstation/src/auth.rs` - OIDC login/callback/logout handlers
- `workstation/src/oidc.rs` - OIDC discovery and client registration
- `workstation/src/config.rs` - Environment variable configuration
- `workstation/src/gorp_client.rs` - HTTP client for gorp API
- `workstation/templates/*.html` - Askama HTML templates

### Gorp (relevant endpoints)
- `gorp/src/admin/routes.rs` - Admin panel routes including API
- `gorp/src/terminal.rs` - PTY session management
- `gorp/src/browser.rs` - Chrome CDP session management
- `gorp/src/webhook.rs` - Webhook server setup (includes admin router)

## Configuration

### Environment Variables (.env)
```bash
OIDC_ISSUER=https://account.matrix.org/
OIDC_REDIRECT_URI=https://<tunnel-url>/auth/callback
SESSION_DB_PATH=workstation_sessions.db
GORP_API_URL=http://localhost:13000
WORKSPACE_PATH=./workspace
WORKSTATION_PORT=8088
```

### Running Locally
```bash
# Start gorp first (needs config.toml with Matrix credentials)
cargo run -p gorp start

# Start workstation
cargo run -p workstation
```

### Running with Cloudflare Tunnel
```bash
# Start tunnel (get URL like https://xyz.trycloudflare.com)
cloudflared tunnel --url http://localhost:8088

# Update .env with tunnel URL
OIDC_REDIRECT_URI=https://xyz.trycloudflare.com/auth/callback

# Delete cached OIDC client (registered with old URL)
rm workstation_oidc_client.json

# Restart workstation
cargo run -p workstation
```

## Next Steps (Phase 4)

1. **Fix SQLite session storage** - Investigate tokio_rusqlite file open issue
2. **Room membership authorization** - Filter channels by user's Matrix room membership
3. **Token refresh** - Handle expired access tokens
4. **Session cleanup** - Expire old sessions automatically
5. **Error handling** - Better error pages and user feedback

## Related Documentation

- [Workstation Design](plans/2025-12-17-workstation-webapp-design.md)
- [Phase 1 Implementation](plans/2025-12-17-workstation-webapp-implementation.md)
- [Phase 2 Implementation](plans/2025-12-17-workstation-phase2-implementation.md)
