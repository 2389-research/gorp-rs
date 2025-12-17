# Workstation Webapp Design

## Overview

A user-facing webapp for Matrix users to configure their gorp workspaces. Runs as a separate Rust/Axum service alongside gorp, exposed via Cloudflare Tunnel.

## Features

- **Workspace config**: CLAUDE.md, MCP servers, tool permissions
- **File manager**: Browse, edit, upload files in workspace directories
- **Terminal**: Shell access to user's container
- **Browser viewer**: Watch/control Chrome instance Claude uses via superpowers-chrome

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                     Local Machine                            │
│                                                              │
│  ┌──────────────┐      REST/WS      ┌──────────────┐        │
│  │   Webapp     │◄────────────────►│    Gorp      │        │
│  │  :8088       │                   │   :13000     │        │
│  │              │                   │              │        │
│  │ - Auth       │    filesystem     │ - Matrix     │        │
│  │ - File mgmt  │◄────────────────►│ - Claude     │        │
│  │ - Terminal   │   (workspaces/)   │ - Sessions   │        │
│  │ - Browser    │                   │ - Scheduler  │        │
│  └──────────────┘                   └──────────────┘        │
│         ▲                                  │                 │
│         │                                  ▼                 │
│         │                           ┌──────────────┐        │
│         │                           │  Containers  │        │
│         │                           │  (per user)  │        │
│         │                           └──────────────┘        │
└─────────│───────────────────────────────────────────────────┘
          │
    Cloudflare Tunnel
          │
          ▼
       Users (Matrix SSO)
```

## Container Model

One container per user:

```
User A's Container (gorp-harper-matrix-org)
├── gorp
├── claude
└── /workspace/
    ├── channel-1/
    ├── channel-2/
    └── channel-3/
```

Container naming convention: `gorp-{sanitized_matrix_id}`

## Authentication

Matrix SSO via OpenID Connect:

1. User visits webapp → clicks "Login with Matrix"
2. Webapp redirects to Matrix homeserver's auth endpoint
3. User authenticates with Matrix credentials
4. Homeserver redirects back with auth code
5. Webapp exchanges code for user's Matrix ID
6. Webapp creates session cookie

Authorization: Users see channels for Matrix rooms they're members of.

## API Endpoints

### Gorp API (new endpoints)

```
GET  /api/auth/matrix          → initiate Matrix SSO
GET  /api/auth/callback        → Matrix SSO callback
POST /api/auth/logout          → clear session

GET  /api/channels             → list user's channels
GET  /api/channels/{name}      → channel details

POST /api/terminal             → spawn shell in user's container
WS   /ws/terminal/{session}    → PTY stream

POST /api/browser/connect      → start Chrome screencast
WS   /ws/browser/{session}     → screencast + input
POST /api/browser/action       → click/type/navigate
```

### Webapp Routes

```
GET  /                         → dashboard (channel list)
GET  /channels/{name}          → channel detail page
GET  /channels/{name}/settings → config editors
GET  /terminal                 → terminal page
GET  /browser                  → browser viewer page

GET  /files/{channel}/*path    → read/list
PUT  /files/{channel}/*path    → write
POST /files/{channel}/*path    → create
DELETE /files/{channel}/*path  → delete
```

## File Management

Direct filesystem access (same machine as gorp):

- Tree view of `workspace/{channel}/`
- Create, rename, delete files and folders
- Edit text files inline
- Upload/download files
- Path traversal protection

Key files:
- `CLAUDE.md` - channel instructions
- `.mcp.json` - MCP server configuration
- `.claude/settings.json` - tool permissions

## Terminal

One terminal per user's container (not per channel):

1. User authenticates → webapp knows Matrix ID
2. Webapp derives container name from Matrix ID
3. Gorp spawns PTY in container
4. xterm.js connects via WebSocket
5. Shell starts in `/workspace`

## Browser Viewer

Stream Chrome via CDP (Chrome DevTools Protocol):

1. Webapp connects to Chrome's CDP through gorp
2. Uses `Page.screencastFrame` for live video
3. User clicks/types → webapp sends CDP commands

Gorp proxies CDP for auth and isolation.

## Tech Stack

- **Webapp**: Rust, Axum, htmx, Tailwind CSS
- **Terminal**: xterm.js + WebSocket
- **Browser**: CDP screencast + canvas
- **Auth**: Matrix OIDC
- **Tunnel**: Cloudflare Tunnel (programmatic setup)

## Data Flow

| Feature | Path |
|---------|------|
| Auth | Webapp ↔ Matrix homeserver (OIDC) |
| Channel list | Webapp → Gorp API → Matrix API |
| Files | Webapp → filesystem (direct) |
| Terminal | Webapp → Gorp WS → container PTY |
| Browser | Webapp → Gorp WS → Chrome CDP |

## Security

- Matrix SSO for all access
- Users only see channels for rooms they're members of
- Path traversal protection on file operations
- Terminal/browser scoped to user's container only
- Chrome not directly exposed (proxied through gorp)
