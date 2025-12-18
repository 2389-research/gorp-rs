# Claude Jail Design

A Python service replacing gorp-rs's CLI subprocess hack with the Claude Agent SDK.

## Problem

gorp-rs currently communicates with Claude by spawning the CLI as a subprocess:

- Uses `--dangerously-skip-permissions` to bypass permission prompts
- Parses JSONL streaming output line-by-line from stdout
- Logs all communication to disk unencrypted
- Fragile coupling to CLI output format

This works, but it's a hack.

## Solution

**Claude Jail** is a Python WebSocket service that:

- Uses the Claude Agent SDK for proper library integration
- Manages N concurrent channel sessions with idle cleanup
- Loads `.mcp.json` per channel workspace (same format as Claude Code)
- Streams responses back to gorp-rs over WebSocket

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                         gorp-rs                              │
│  ┌─────────────┐    ┌─────────────┐    ┌─────────────┐      │
│  │  Channel A  │    │  Channel B  │    │  Channel N  │      │
│  │  Workspace  │    │  Workspace  │    │  Workspace  │      │
│  └──────┬──────┘    └──────┬──────┘    └──────┬──────┘      │
│         │                  │                  │              │
│         └──────────────────┼──────────────────┘              │
│                            │                                 │
│                    WebSocket Client                          │
└────────────────────────────┼─────────────────────────────────┘
                             │
                             ▼
┌────────────────────────────────────────────────────────────┐
│                      Claude Jail                            │
│                   (Python + Agent SDK)                      │
│                      ws://127.0.0.1:31337                   │
│                                                             │
│  ┌──────────────────────────────────────────────────────┐  │
│  │              WebSocket Server (asyncio)               │  │
│  └──────────────────────────┬───────────────────────────┘  │
│                             │                               │
│  ┌──────────────────────────▼───────────────────────────┐  │
│  │              Session Manager                          │  │
│  │   - Creates/resumes ClaudeSDKClient per channel       │  │
│  │   - Idle timeout cleanup (5 min default)              │  │
│  │   - Loads .mcp.json per workspace                     │  │
│  └──────────────────────────┬───────────────────────────┘  │
│                             │                               │
│  ┌──────────────────────────▼───────────────────────────┐  │
│  │              Agent SDK Clients                        │  │
│  │   Channel A ──► ClaudeSDKClient ──► MCP Servers      │  │
│  │   Channel B ──► ClaudeSDKClient ──► MCP Servers      │  │
│  └──────────────────────────────────────────────────────┘  │
└────────────────────────────────────────────────────────────┘
```

## WebSocket Protocol

### Gorp → Claude Jail

```json
// Start or continue a conversation
{
  "type": "query",
  "channel_id": "!abc123:matrix.org",
  "workspace": "/path/to/channel/workspace",
  "prompt": "User's message here",
  "session_id": "optional-resume-id"
}

// Explicitly close a session
{
  "type": "close_session",
  "channel_id": "!abc123:matrix.org"
}
```

### Claude Jail → Gorp

```json
// Streaming text chunks
{
  "type": "text",
  "channel_id": "!abc123:matrix.org",
  "content": "Here's what I found..."
}

// Tool use notification
{
  "type": "tool_use",
  "channel_id": "!abc123:matrix.org",
  "tool": "mcp__matrix__send_attachment",
  "input": {"file": "chart.png", "room": "!abc123:matrix.org"}
}

// Conversation complete
{
  "type": "done",
  "channel_id": "!abc123:matrix.org",
  "session_id": "uuid-for-resumption"
}

// Error occurred
{
  "type": "error",
  "channel_id": "!abc123:matrix.org",
  "message": "Session expired or MCP server failed"
}
```

## Session Management

Sessions are created on-demand and cleaned up after idle timeout:

- Keyed by `channel_id` (Matrix room ID)
- Each session loads its own `.mcp.json` from workspace
- Default idle timeout: 5 minutes
- Sessions can be resumed via `session_id`

## Project Structure

```
claude-jail/
├── pyproject.toml          # uv managed, depends on claude-agent-sdk
├── src/
│   └── claude_jail/
│       ├── __init__.py
│       ├── server.py       # WebSocket server entrypoint
│       ├── session.py      # SessionManager + ChannelSession
│       ├── protocol.py     # Message types (Pydantic models)
│       └── mcp_loader.py   # Load .mcp.json from workspace
├── tests/
│   ├── test_session.py
│   ├── test_protocol.py
│   └── test_integration.py
└── README.md
```

## Configuration

Environment variables:

| Variable | Default | Description |
|----------|---------|-------------|
| `CLAUDE_JAIL_HOST` | `127.0.0.1` | Bind address |
| `CLAUDE_JAIL_PORT` | `31337` | WebSocket port |
| `CLAUDE_JAIL_IDLE_TIMEOUT` | `300` | Session idle timeout (seconds) |
| `CLAUDE_JAIL_LOG_LEVEL` | `INFO` | Logging level |

## Gorp-rs Changes

Minimal changes required:

1. New `src/claude_jail.rs` with `ClaudeJailClient` WebSocket client
2. Replace `invoke_claude_streaming()` calls with `jail_client.query()`
3. Config change: `claude.jail_url` instead of `claude.binary_path`
4. MCP server stays exactly the same

## What We Gain

- No more `--dangerously-skip-permissions`
- No more JSONL parsing from subprocess stdout
- Proper session management with resumption
- First-class async Python instead of CLI scraping
- Full Agent SDK features (hooks, subagents, etc.)

## What Stays the Same

- MCP server in gorp-rs (Matrix tools)
- Workspace per channel
- `.mcp.json` configuration format
