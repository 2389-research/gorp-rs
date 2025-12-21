# Message Display Bug: Long Messages Not Appearing Until Next User Action

## Problem Statement

Users report that long messages from the bot sometimes don't appear until they send another message. The message is successfully sent by the bot, but the user's Matrix client doesn't display it until a subsequent action triggers a sync.

## Root Cause Analysis

### How Messages Flow

1. **ACP Streaming**: Claude streams response chunks via ACP protocol
2. **Local Buffering**: All chunks are accumulated in a `final_response` string (`message_handler.rs:441-443`)
3. **Chunking**: Once complete, the response is split into 8000-char chunks (`utils.rs:18-70`)
4. **Sending**: Chunks are sent via `room.send()` with 100ms delays between them

### The Timing Problem

The `room.send().await` call completes when the Matrix **server** acknowledges receipt. However, this does NOT mean the recipient's client has synced to receive the message.

Matrix clients sync on intervals (could be 30+ seconds when idle). When a user sends a message, their client does an immediate sync to check for responses. But if the bot's response takes longer than expected:

1. User sends message
2. User's client syncs immediately, sees nothing yet
3. User's client returns to idle sync interval
4. Bot finishes processing, sends response
5. Server has the message, but user's client won't sync for another 30 seconds
6. User sends another message → triggers sync → NOW they see the response

### Recent Fix (ae4d21d) - Partial Solution

The commit "fix: send first message chunk before stopping typing indicator" addressed a related issue where the typing indicator would stop before any message content was visible. Now the first chunk is sent BEFORE stopping the typing indicator.

This helps when responses are quick, but doesn't solve the fundamental sync timing issue for long-running responses.

## Proposed Solutions

### Option 1: Thinking Message + Edit + Delete Pattern (Recommended)

```
User: "explain quantum computing"
Bot:  "🤔 Thinking..." (sent immediately, user gets notification)
      [processing time...]
Bot:  [deletes "thinking" message]
Bot:  "Quantum computing is..." (new message, triggers second notification)
```

**Pros:**
- User gets notification when bot starts AND when bot finishes
- Clean final state (only one message visible)
- Works regardless of processing time

**Cons:**
- Brief moment with two messages
- Slightly more complex implementation

**Implementation:**
```rust
// 1. Send thinking message immediately
let thinking = room.send(RoomMessageEventContent::text_plain("🤔 Thinking...")).await?;
let thinking_event_id = thinking.event_id;

// 2. Start typing indicator, process with Claude...

// 3. Delete thinking message
room.redact(&thinking_event_id, None, None).await?;

// 4. Send actual response (triggers notification)
room.send(RoomMessageEventContent::text_html(&response, &html)).await?;
```

### Option 2: Thinking Message + Edit (No Notification on Complete)

```
User: "explain quantum computing"
Bot:  "🤔 Thinking..." (sent immediately)
      [processing time...]
Bot:  [edits "thinking" to show actual response]
```

**Pros:**
- Clean, single message throughout
- Uses Matrix SDK's `make_edit_event()` API

**Cons:**
- Edits don't trigger push notifications
- If user leaves, they won't know response is ready
- Only works if user stays watching the room

**Implementation:**
```rust
// 1. Send thinking message
let thinking = room.send(RoomMessageEventContent::text_plain("🤔 Thinking...")).await?;

// 2. Process with Claude...

// 3. Create and send edit
let new_content = EditedContent::RoomMessage(
    RoomMessageEventContentWithoutRelation::text_html(&response, &html)
);
let edit = room.make_edit_event(&thinking.event_id, new_content).await?;
room.send(edit).await?;
```

### Option 3: Thinking + Edit + "Done" Ping

Combines Option 2 with a brief follow-up message to trigger notification:

```
Bot:  "🤔 Thinking..." → [edits to full response] → "✓"
```

**Pros:**
- User gets notification when complete
- Main content is in the edited message

**Cons:**
- Extra "✓" message is noise
- Could delete it after a delay, but adds complexity

### Option 4: Mention User in Response

Include `@username` in the response to force a push notification.

**Pros:**
- Guaranteed notification

**Cons:**
- Could be annoying for every response
- Changes message format

### Option 5: Progressive Streaming Edits

Edit the "thinking" message multiple times as chunks arrive:

```
"🤔 Thinking..."
"Quantum computing is a type of..."
"Quantum computing is a type of computation that..."
[continues until complete]
```

**Pros:**
- Real-time streaming feel
- User sees progress

**Cons:**
- High message edit volume
- Still no notification on complete
- More complex implementation
- Rate limiting concerns

## Recommendation

**Option 1 (Thinking + Delete + New Message)** is recommended because:

1. Guarantees notification on both start and completion
2. Clean final state with single message
3. Works regardless of how long processing takes
4. Relatively simple to implement
5. Follows pattern used by other chat bots (Slack apps, Discord bots, etc.)

The brief moment where two messages are visible is acceptable UX, and the delete happens quickly after the real response is sent.

## Implementation Notes

### Matrix SDK APIs Needed

- `room.send()` - Already in use
- `room.redact()` - For deleting the thinking message
- Capture `SendResponse.event_id` - To reference the thinking message for deletion

### Chunking Consideration

For responses that need chunking (>8000 chars):
- Delete thinking message before first chunk
- Send all chunks as new messages (current behavior)
- Each chunk beyond the first won't trigger notification, but user is already watching

### Error Handling

If Claude processing fails:
- Edit thinking message to show error, OR
- Delete thinking message and send error as new message

## Files to Modify

- `src/message_handler.rs` - Main implementation in `handle_message()` and response sending logic
- Potentially `src/utils.rs` - If chunking logic needs adjustment

## References

- [Matrix SDK Room struct](https://matrix-org.github.io/matrix-rust-sdk/matrix_sdk/room/struct.Room.html)
- [RoomMessageEventContent](https://matrix-org.github.io/matrix-rust-sdk/matrix_sdk/ruma/events/room/message/struct.RoomMessageEventContent.html)
- Commit ae4d21d - Previous fix for typing indicator timing
