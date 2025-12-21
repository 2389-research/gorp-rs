# ACP Session Resume Limitation: Sessions Don't Persist Across Container Restarts

## Problem Statement

When a gorp container restarts, ACP session resume fails with "Failed to load ACP session". Conversations are lost and a new session is created, even though the session ID is correctly stored in sessions.db.

## Observed Behavior

```
[Before restart]
session_id: 68058b02-7710-458e-b13c-e617c8c196f4
channel: feeds
Successfully completed prompt with 186 events

[After restart]
Attempting to resume existing session, session_id: 68058b02-7710-458e-b13c-e617c8c196f4
Failed to resume session, creating new one, error: Failed to load ACP session
Created new ACP session, session_id: [NEW-UUID]
```

## Root Cause Analysis

### How ACP Sessions Work

1. **Session Creation**: When `NewSession` is called, `claude-code-acp` creates an in-memory session
2. **Session State**: All conversation context is held in the running `claude-code-acp` process memory
3. **Session ID**: Just a UUID handle to reference the in-memory session
4. **LoadSession**: Attempts to find an existing session by ID in the running process

### Why Resume Fails After Restart

```
Container Lifecycle:
┌─────────────────────────────────────────────────────────────┐
│ Container Start #1                                           │
│                                                              │
│  gorp ──NewSession──> claude-code-acp                       │
│        <──UUID 1234──                                        │
│                                                              │
│  [Session 1234 exists in claude-code-acp memory]            │
│  [gorp stores 1234 in sessions.db]                          │
│                                                              │
│  gorp ──Prompt──> claude-code-acp (using 1234)              │
│        <──Response──                                         │
│                                                              │
└─────────────────────────────────────────────────────────────┘
                           │
                    Container Restart
                           │
                           ▼
┌─────────────────────────────────────────────────────────────┐
│ Container Start #2                                           │
│                                                              │
│  [gorp loads session ID 1234 from sessions.db]              │
│  [NEW claude-code-acp process starts - empty memory]        │
│                                                              │
│  gorp ──LoadSession(1234)──> claude-code-acp                │
│        <──Error: Unknown session──                           │
│                                                              │
│  gorp ──NewSession──> claude-code-acp                       │
│        <──UUID 5678──                                        │
│                                                              │
│  [Previous conversation context is LOST]                     │
│                                                              │
└─────────────────────────────────────────────────────────────┘
```

### Key Insight

ACP sessions are **ephemeral by design**. The protocol doesn't include session persistence - it assumes the agent process runs continuously.

## Current Mitigation

gorp handles this gracefully with a fallback mechanism:

```rust
// From warm_session.rs (simplified)
match acp_client.load_session(&session_id).await {
    Ok(_) => { /* Resume succeeded */ }
    Err(_) => {
        // Fallback: create new session
        let new_id = acp_client.new_session().await?;
        // Update stored session ID
    }
}
```

This ensures the bot remains functional, but conversation continuity is lost.

## Potential Solutions

### Option 1: Accept as Limitation (Current Behavior)

**Status**: Implemented

Users start fresh conversations after container restarts. This is acceptable for many use cases.

**Pros:**
- Already working
- Simple and reliable
- No additional complexity

**Cons:**
- Conversation context lost on restart
- Users may need to re-explain context

### Option 2: Leverage Claude Code's Conversation History

Claude Code stores conversation history in `~/.claude/projects/`. After restart, gorp could:

1. Note that LoadSession failed
2. Create new ACP session
3. Include context in system prompt: "Continuing from previous conversation about X"
4. Or prepend summary of previous messages

**Implementation Sketch:**
```rust
// After LoadSession fails
let new_session = acp_client.new_session().await?;

// Fetch previous conversation from channel history
let context = channel.get_recent_messages(10).await?;
let context_summary = format!(
    "Previous conversation context:\n{}",
    summarize_messages(&context)
);

// Include in first prompt's system context
```

**Pros:**
- Maintains some conversation continuity
- Uses existing Claude Code history
- No ACP protocol changes needed

**Cons:**
- Adds tokens to each resumed conversation
- Summary may lose nuance
- More complex implementation

### Option 3: Persist Session State (ACP Protocol Change)

Would require changes to the ACP protocol to support session serialization/deserialization.

**Not recommended** - would require upstream changes to claude-code-acp.

### Option 4: Keep claude-code-acp Warm

Use a sidecar process or socket to keep claude-code-acp running even when gorp restarts.

**Pros:**
- True session persistence
- No conversation loss

**Cons:**
- More complex container architecture
- claude-code-acp may have its own restart requirements
- Resource usage when idle

## Recommendation

**Keep Option 1** (current behavior) as the default, with **Option 2** as a future enhancement.

The graceful fallback to new sessions is working correctly. For most Matrix bot use cases, starting fresh after a restart is acceptable - users can simply reference previous messages if needed.

Option 2 would be a nice-to-have enhancement for channels that need stronger conversation continuity, but adds complexity.

## Test Scenario

The `acp-session-resume` scenario in `scenarios.jsonl` documents this expected behavior:

```json
{
  "name": "acp-session-resume",
  "given": "existing session ID in DB",
  "when": "gorp-acp restarts and prompt sent",
  "then": "conversation context preserved",
  "validates": "LoadSession with persisted ID"
}
```

**Update needed**: This scenario's "then" clause is aspirational, not current behavior. Should be updated to:
```json
{
  "then": "fallback to new session if LoadSession fails, context may be lost",
  "validates": "graceful degradation when ACP session not found"
}
```

## Files Involved

- `src/warm_session.rs` - Session resume logic and fallback
- `src/acp_client.rs` - ACP protocol client (LoadSession, NewSession)
- `src/session.rs` - SessionStore for persisting session IDs
- `workspace/sessions.db` - SQLite storage for channel→session mappings

## Tested On

- gorp-8 instance on prod server
- Container restart via `docker rm -f && gorp-multi.sh start`
- Claude Code 2.0.74, claude-code-acp
- December 2024
