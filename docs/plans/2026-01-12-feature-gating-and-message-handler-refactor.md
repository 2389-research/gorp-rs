# Feature Gating & Message Handler Refactor

## Overview

Simplify gorp-rs by:
1. Feature-gating GUI and admin dashboard (optional compilation)
2. Refactoring the monolithic message_handler into focused modules

## Phase 1: Feature Gating

### Problem
- `ServerState` lives in `src/gui/mod.rs` but is used by headless mode
- GUI dependencies (iced, tray-icon, global-hotkey) always compile
- Admin dashboard compiles even when not needed

### Solution

**Extract ServerState to `src/server.rs`:**
```rust
// src/server.rs
pub struct ServerState {
    pub config: Arc<Config>,
    pub matrix_client: Client,
    pub session_store: Arc<SessionStore>,
    pub scheduler_store: SchedulerStore,
    pub warm_manager: SharedWarmSessionManager,
    pub sync_token: String,
}

impl ServerState {
    pub async fn initialize(config: Config) -> Result<Self> { ... }
    pub fn get_rooms(&self) -> Vec<RoomInfo> { ... }
}
```

**Cargo.toml features:**
```toml
[features]
default = ["gui", "admin"]
gui = ["dep:iced", "dep:tray-icon", "dep:global-hotkey"]
admin = ["dep:askama", "dep:tower-sessions"]
```

**Conditional compilation:**
- `src/gui/` - `#[cfg(feature = "gui")]`
- `src/admin/` - `#[cfg(feature = "admin")]`
- Admin routes in webhook.rs - conditionally mounted

### Files Changed
- `Cargo.toml` - add features, make deps optional
- `src/server.rs` - NEW, extracted from gui/mod.rs
- `src/lib.rs` - conditional gui/admin exports
- `src/gui/mod.rs` - remove ServerState, import from server
- `src/main.rs` - update imports
- `src/webhook.rs` - conditional admin mounting

## Phase 2: Message Handler Refactor

### Problem
- `src/message_handler/mod.rs` is 2,160 lines
- `handle_message` function is ~740 lines
- `handle_command` function is ~1,200 lines
- Hard to navigate and maintain

### Solution

**New module structure:**
```
src/message_handler/
├── mod.rs              # Thin router (~150 lines)
├── commands.rs         # Testable commands (existing)
├── matrix_commands.rs  # Matrix-dependent commands (NEW)
├── chat.rs             # Chat message processing (NEW)
├── attachments.rs      # Attachment handling (expand)
├── helpers.rs          # Utilities (merge context.rs)
├── traits.rs           # MessageSender trait (existing)
└── schedule_import.rs  # Schedule parsing (existing)
```

**Extraction targets:**

1. `matrix_commands.rs` (~800 lines):
   - setup, create, join, delete commands
   - schedule command and subcommands
   - All commands requiring Matrix client

2. `chat.rs` (~600 lines):
   - Regular chat message processing
   - Claude session management
   - Response streaming to Matrix
   - Typing indicators

3. `attachments.rs` (expand):
   - Move `download_attachment` from mod.rs
   - Keep existing attachment helpers

4. `helpers.rs` (merge):
   - Absorb `context.rs` (write_context_file)
   - Existing helper functions

**New mod.rs structure:**
```rust
pub async fn handle_message(...) -> Result<()> {
    // Early filtering (~30 lines)
    // Command routing (~10 lines)
    // DISPATCH routing (~10 lines)
    // Onboarding check (~20 lines)
    // Chat delegation (~10 lines)
}
```

### Files Changed
- `src/message_handler/mod.rs` - slim to ~150 lines
- `src/message_handler/matrix_commands.rs` - NEW
- `src/message_handler/chat.rs` - NEW
- `src/message_handler/attachments.rs` - expand
- `src/message_handler/helpers.rs` - merge context.rs
- `src/message_handler/context.rs` - DELETE

## Implementation Order

### Phase 1 Tasks
1. Create `src/server.rs` with ServerState
2. Update `src/gui/mod.rs` to import ServerState
3. Update `src/main.rs` imports
4. Add features to `Cargo.toml`
5. Gate `gui` module
6. Gate `admin` module
7. Conditional admin routes in webhook.rs
8. Verify: `cargo build --no-default-features`

### Phase 2 Tasks
1. Create `src/message_handler/chat.rs`
2. Create `src/message_handler/matrix_commands.rs`
3. Merge context.rs into helpers.rs
4. Move download_attachment to attachments.rs
5. Refactor mod.rs to thin router
6. Verify: `cargo test`

## Verification

```bash
# Full build (default)
cargo build

# Headless build (no GUI, no admin)
cargo build --no-default-features

# Headless with admin dashboard
cargo build --no-default-features --features admin

# All tests pass
cargo test
```
