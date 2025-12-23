# Remote Channel Reset Design

## Problem

When Claude Code gets into a bad state and returns errors in a channel, the `!reset` command only works from *inside* that channel. If the channel is broken/erroring, you can't send commands to it.

## Solution

Add `!reset <channel_name>` command that works from DMs (orchestrator), allowing reset of any channel remotely.

Also fix: the current `!reset` command updates the database but doesn't evict the stuck AgentHandle from the warm session cache, so the broken agent persists in memory.

## Changes

### 1. Add `evict()` to WarmSessionManager

```rust
// warm_session.rs
pub fn evict(&mut self, channel_name: &str) -> bool {
    self.sessions.remove(channel_name).is_some()
}
```

### 2. Add DM command `!reset <channel_name>`

In `handle_command()`, add to DM commands section:

```rust
"reset" if is_dm => {
    let channel_name = args.trim();
    if channel_name.is_empty() {
        // Show usage
        return Ok(());
    }

    // Look up channel
    let channel = session_store.get_by_name(channel_name)?;

    // Generate new session ID
    let new_session_id = uuid::Uuid::new_v4().to_string();
    session_store.reset_session(&channel.channel_name, &new_session_id)?;

    // Evict from warm cache
    warm_manager.write().await.evict(&channel.channel_name);

    // Confirm in DM
    // Optionally notify the channel
}
```

### 3. Fix existing room `!reset` to also evict

Add eviction call after `reset_session()` in the room reset handler.

## Testing

1. Create a channel, send message to warm it up
2. From DM: `!reset <channel_name>`
3. Verify session_id changed in database
4. Verify warm cache no longer has the session
5. Send message to channel, verify fresh session created
