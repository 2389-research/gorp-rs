# Matrix ↔︎ Claude Code Bridge (Python)

This example shows how to connect a Matrix room to Claude Code by spawning the
`claude` CLI for messages prefixed with `!claude`. It uses[`matrix-nio`](https://github.com/poljar/matrix-nio)
and mirrors the same CLI flags as the terminal demos, so you can optionally
tunnel through `--sdk-url`.

## Features

- Optionally filters to a single room (listens to all rooms by default) and responds when a message starts with `!claude`.
- Keeps a long-lived Claude session per Matrix room so follow-ups share context.
- Supports `/reset` and `/end` commands inside Matrix to drop the session.
- Pipes replies back to the room as Markdown/plain text.

## Prerequisites

1. Claude Code CLI must be installed and authenticated.
2. Python 3.10+ (asyncio + `asyncio.create_subprocess_exec`).
3. A Matrix account for the bot and a room ID that the bot can access.

Install dependencies:

```bash
cd docs/examples/matrix-bridge
uv sync
```

## Configuration

Copy `.env.example` to `.env` and fill in your Matrix credentials:

```bash
cp .env.example .env
```

Edit `.env` with your values:

| Variable | Required | Description |
|----------|----------|-------------|
| `MATRIX_HOMESERVER` | Yes | Base URL, e.g. `https://matrix.example.com` |
| `MATRIX_USER` | Yes | Matrix user ID (`@bot:example.com`) |
| `MATRIX_ROOM_ID` | No | **Optional** - Room ID to restrict bot to single room (e.g. `!abc123:example.com`). If not set, bot listens in ALL rooms. |
| `MATRIX_PASSWORD` | Yes* | Account password (*or use `MATRIX_ACCESS_TOKEN`) |
| `MATRIX_ACCESS_TOKEN` | No | Optional access token instead of password |
| `CLAUDE_BIN` | No | Optional path to the `claude` binary (defaults to `claude`) |
| `SDK_URL` | No | Optional `--sdk-url` forwarded to the CLI |

## Usage

```bash
cd docs/examples/matrix-bridge
uv run python main.py
```

In the Matrix room, use:

- `!claude <prompt>` to send a prompt.
- `!claude /reset` to discard the existing Claude session.
- `!claude /end` to drop the session without starting a new one.

You can customize the prefix by editing `COMMAND_PREFIX` inside the script.

## Caveats

- This example is intentionally simple (no retries, no encryption, no state
  persistence). For production, add proper logging, health checks, and store
  session IDs in a database.
- The CLI runs on the same host as the bot. If you need remote execution, set
  `SDK_URL` to point at your Claude Code SDK server and forward any directory
  mounts as needed.
