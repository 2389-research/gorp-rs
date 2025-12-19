# Warm Session Manager Design

## Problem

Claude Code takes ~2 minutes to initialize via ACP. Each webhook/message currently spawns a fresh process, causing unacceptable latency.

## Solution

Keep Claude Code processes alive between requests per channel.

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│                        gorp                              │
├─────────────┬─────────────┬─────────────────────────────┤
│  webhook    │  message    │  scheduler                  │
│  handler    │  handler    │  (triggers 5min pre-warm)   │
└──────┬──────┴──────┬──────┴──────┬──────────────────────┘
       │             │             │
       └─────────────┼─────────────┘
                     ▼
         ┌───────────────────────┐
         │  WarmSessionManager   │
         │  - channel → session  │
         │  - last_used times    │
         │  - 1hr cleanup task   │
         └───────────┬───────────┘
                     ▼
         ┌───────────────────────┐
         │  AcpClient (per chan) │
         │  - kept alive         │
         │  - reused for prompts │
         └───────────────────────┘
```

## Warming Strategy

1. **Lazy with keep-alive**: First message starts Claude Code, subsequent messages reuse it
2. **Predictive for schedules**: Start warming 5 minutes before scheduled prompts
3. **1 hour timeout**: Kill idle processes after 1 hour of inactivity

## API

```rust
pub struct WarmSessionManager {
    sessions: HashMap<String, WarmSession>,
    config: WarmConfig,
}

struct WarmSession {
    client: AcpClient,
    session_id: String,
    last_used: Instant,
    channel_name: String,
}

struct WarmConfig {
    keep_alive_duration: Duration,  // 1 hour
    pre_warm_lead_time: Duration,   // 5 minutes
}

impl WarmSessionManager {
    async fn get_session(&mut self, channel: &Channel) -> Result<&mut WarmSession>;
    async fn pre_warm(&mut self, channel: &Channel) -> Result<()>;
    async fn cleanup_stale(&mut self);
    async fn prompt(&mut self, channel_name: &str, text: &str) -> Result<Receiver<AcpEvent>>;
}
```

## Integration Points

1. **webhook.rs** - Replace `invoke_acp()` with `warm_manager.prompt()`
2. **message_handler.rs** - Same replacement
3. **scheduler.rs** - Add `warm_manager.pre_warm()` call 5 min before scheduled time
4. **main.rs** - Create WarmSessionManager, spawn cleanup task, pass to handlers

## Configuration

```toml
[acp]
agent_binary = "claude-code-acp"
keep_alive_secs = 3600      # 1 hour
pre_warm_secs = 300         # 5 minutes
```

## Constraints

- 1-5 channels expected (no pooling needed)
- Per-channel isolation maintained
- Event streaming unchanged
