# Simple Claude Code TUI Demo

This example shows how to build a tiny terminal UI that shells out to the `claude`
CLI in `--print` mode. It keeps a session alive across turns, renders an
in-place chat log, and demonstrates how to pass through extra flags such as
`--sdk-url` when you want the CLI to act as a thin WebSocket client.

## Prerequisites

1. Claude Code CLI must already be authenticated on this machine.
2. Node 18+ (or any Node release that supports `crypto.randomUUID`).
3. Optional: set `SDK_URL` to point at a remote Claude Code SDK server.

## Usage

```bash
# install deps
cd examples/simple-tui
npm install

# run the demo
npm start

# specify a remote SDK server (forwarded via --sdk-url)
SDK_URL=ws://localhost:8080 npm start

# choose a fixed session id so you can reconnect later
SESSION_ID=your-session-guid npm start
```

Controls:

- Type a prompt and press Enter to send it.
- `/reset` drops the existing conversation and starts a new session.
- `/quit` or Ctrl+C exits without touching the underlying session.

### Python alternative

Prefer Python? Run:

```bash
cd examples/simple-tui
python3 tui.py

# With remote SDK server
SDK_URL=ws://localhost:8080 python3 tui.py
```

Both variants share the same environment knobs (`SESSION_ID`, `SDK_URL`, `CLAUDE_BIN`)
so you can switch between them freely.

Under the hood this script spawns the CLI with:

```
claude --print --input-format text --output-format json \
       --session-id/--resume ... [--sdk-url ...]
```

and pipes user messages through stdin so you do not have to worry about shell
escaping. Because it uses the CLI exactly how a Matrix bridge or other bot
integration would, you can use it as a reference implementation for wiring
Claude Code into any other text-based frontend.***
