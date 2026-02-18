# ACP Migration Status

## Overview

gorp has been migrated from direct Claude Code CLI invocation to using the Agent Client Protocol (ACP). This enables support for multiple AI backends including Claude (via `claude-code-acp`) and OpenAI Codex (via `codex-acp`).

## Current State

### What Works

1. **ACP Protocol Integration** - Full ACP client implementation in `gorp-agent/src/backends/acp.rs`
2. **Warm Session Management** - Sessions are kept alive per-channel to avoid cold start latency (`gorp-core/src/warm_session.rs`)
3. **Matrix Integration** - Messages flow through the message bus to the orchestrator, which routes to the appropriate agent backend
4. **Configurable Backend** - Switch between Claude and Codex via `config.toml`

### Configuration

```toml
[backend]
type = "acp"
binary = "claude-code-acp"
timeout_secs = 300
```

## Performance Comparison

Benchmarked on 2024-12-18 using simple "reply with exactly: N" prompts:

| Metric | Codex (OpenAI) | Claude | Difference |
|--------|----------------|--------|------------|
| Cold start | 1.4s | 26s | 18x faster |
| Warm prompt avg | 1.0s | 17.7s | 17x faster |
| 5 prompts total | 5.4s | 97s | 18x faster |

### Key Finding

The Claude slowness is **not** MCP initialization or session handling - it's pure API response time. Even trivial prompts take 15-22 seconds with Claude Code.

## Architecture

```
Matrix Message
     │
     ▼
┌─────────────────┐
│  message_handler │  (src/message_handler/)
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│ warm_session    │  Manages per-channel sessions
│ manager         │  (src/warm_session.rs)
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│  AcpBackend /   │  ACP protocol implementation
│ PersistentAcp   │  (gorp-agent/src/backends/acp.rs)
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│  claude-code-acp │  OR  codex-acp
│  (subprocess)    │
└────────┬────────┘
         │
         ▼
    Claude API  OR  OpenAI API
```

## LocalSet Pattern

The ACP client uses `!Send` futures, requiring a `LocalSet` for proper async execution. The pattern used:

```rust
let local = tokio::task::LocalSet::new();
local.run_until(async move {
    // Handler task receives messages via channel
    let handler_task = tokio::task::spawn_local(async move {
        while let Some(msg) = rx.recv().await {
            // Process in LocalSet context
            // spawn_local works here for ACP I/O
        }
    });

    tokio::task::yield_now().await;  // Let spawned task start

    tokio::select! {
        r = client.sync(settings) => r,
        _ = handler_task => Err(...)
    }
}).await;
```

## Known Issues

### Claude-specific
- ~17-20 second response time even for trivial prompts
- MCP SDK server reinitializes on each prompt (adds ~8s to cold start)

### Codex-specific
- Session reuse may hang on complex prompts (needs investigation)
- Some prompts that require tool calls may not complete

### General
- LoadSession is supported by the ACP backend. Falls back to NewSession when the remote session no longer exists.

## Files Changed in Migration

- `gorp-agent/src/backends/acp.rs` - ACP protocol client (`AcpBackend`, `PersistentAcpClient`)
- `gorp-core/src/warm_session.rs` - Warm session management
- `src/message_handler/` - Message handler module, updated for ACP event handling
- `src/main.rs` - LocalSet integration, channel-based message routing
- `src/mcp.rs` - MCP handler
- `config.toml` - `[backend]` section

## Next Steps

1. Investigate Codex session reuse hanging on complex prompts
2. Consider adding model selection support (Haiku for faster responses?)
3. Add metrics for prompt latency tracking
4. Clean up debug logging (currently verbose for troubleshooting)

## References

- [claude-code-acp](https://github.com/anthropics/claude-code) - Claude's ACP adapter
- [codex-acp](https://github.com/zed-industries/codex-acp) - OpenAI Codex ACP adapter
- [agent-client-protocol](https://crates.io/crates/agent-client-protocol) - Rust ACP crate
