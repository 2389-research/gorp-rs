# gorp-ffi Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Create UniFFI bindings for gorp-agent and gorp-core to enable Swift/Kotlin native app integration.

**Architecture:** Wrapper types prefixed with `Ffi` expose gorp functionality through UniFFI. Embedded Tokio runtime handles async internally. Callback interface streams events to Swift/Kotlin.

**Tech Stack:** Rust, UniFFI 0.28, Tokio, gorp-agent, gorp-core

---

## Task 1: Project Setup

**Files:**
- Create: `gorp-ffi/Cargo.toml`
- Create: `gorp-ffi/src/lib.rs`
- Modify: `Cargo.toml` (workspace root)

**Step 1: Create gorp-ffi directory**

```bash
mkdir -p gorp-ffi/src
```

**Step 2: Create Cargo.toml**

Create `gorp-ffi/Cargo.toml`:

```toml
[package]
name = "gorp-ffi"
version = "0.1.0"
edition = "2021"
description = "UniFFI bindings for gorp-agent and gorp-core"
license = "MIT"

[lib]
crate-type = ["cdylib", "staticlib", "lib"]
name = "gorp_ffi"

[dependencies]
gorp-agent = { path = "../gorp-agent", features = ["acp", "mux"] }
gorp-core = { path = "../gorp-core" }
uniffi = "0.28"
tokio = { version = "1", features = ["rt-multi-thread", "sync"] }
once_cell = "1.19"
serde_json = "1"
anyhow = "1"
thiserror = "1"

[build-dependencies]
uniffi = { version = "0.28", features = ["build"] }

[features]
default = ["mux", "acp"]
mux = ["gorp-agent/mux"]
acp = ["gorp-agent/acp"]
```

**Step 3: Create initial lib.rs**

Create `gorp-ffi/src/lib.rs`:

```rust
// ABOUTME: UniFFI bindings for gorp-agent and gorp-core.
// ABOUTME: Enables Swift/Kotlin integration for native apps.

mod error;
mod runtime;

pub use error::FfiError;

uniffi::setup_scaffolding!();
```

**Step 4: Add to workspace**

Modify root `Cargo.toml` to add workspace member. After `gorp-core = { path = "gorp-core" }`, the workspace should include gorp-ffi.

Add to `[workspace]` section if it exists, or add gorp-ffi as a dev-dependency:

```toml
[workspace]
members = ["gorp-agent", "gorp-core", "gorp-ffi"]
```

**Step 5: Verify project compiles**

Run: `cargo check -p gorp-ffi`
Expected: Compilation errors about missing modules (error, runtime) - that's fine for now.

**Step 6: Commit**

```bash
git add gorp-ffi/ Cargo.toml
git commit -m "feat(gorp-ffi): initialize crate structure"
```

---

## Task 2: Error Types

**Files:**
- Create: `gorp-ffi/src/error.rs`
- Modify: `gorp-ffi/src/lib.rs`

**Step 1: Create error.rs**

Create `gorp-ffi/src/error.rs`:

```rust
// ABOUTME: FFI-safe error types for gorp-ffi.
// ABOUTME: Maps internal errors to UniFFI-compatible enums.

use thiserror::Error;

/// FFI-safe error type
#[derive(Debug, Error, uniffi::Error)]
pub enum FfiError {
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),

    #[error("Backend error: {0}")]
    BackendError(String),

    #[error("Session error: {0}")]
    SessionError(String),

    #[error("IO error: {0}")]
    IoError(String),

    #[error("Database error: {0}")]
    DatabaseError(String),
}

impl From<anyhow::Error> for FfiError {
    fn from(e: anyhow::Error) -> Self {
        FfiError::BackendError(e.to_string())
    }
}

impl From<serde_json::Error> for FfiError {
    fn from(e: serde_json::Error) -> Self {
        FfiError::InvalidConfig(e.to_string())
    }
}
```

**Step 2: Update lib.rs**

The lib.rs already imports error module. Verify it compiles.

**Step 3: Verify compilation**

Run: `cargo check -p gorp-ffi`
Expected: Still errors about runtime module - that's expected.

**Step 4: Commit**

```bash
git add gorp-ffi/src/error.rs
git commit -m "feat(gorp-ffi): add FFI error types"
```

---

## Task 3: Runtime Module

**Files:**
- Create: `gorp-ffi/src/runtime.rs`

**Step 1: Create runtime.rs**

Create `gorp-ffi/src/runtime.rs`:

```rust
// ABOUTME: Embedded Tokio runtime for FFI async operations.
// ABOUTME: Lazy-initialized, lives for app lifetime.

use once_cell::sync::Lazy;
use std::sync::Arc;
use tokio::runtime::Runtime;

static RUNTIME: Lazy<Arc<Runtime>> = Lazy::new(|| {
    Arc::new(
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .thread_name("gorp-ffi")
            .build()
            .expect("Failed to create Tokio runtime"),
    )
});

/// Get reference to the shared runtime
pub fn runtime() -> &'static Runtime {
    &RUNTIME
}

/// Block on an async operation from sync FFI context
pub fn block_on<F: std::future::Future>(f: F) -> F::Output {
    RUNTIME.block_on(f)
}

/// Spawn an async task for background execution
pub fn spawn<F>(f: F) -> tokio::task::JoinHandle<F::Output>
where
    F: std::future::Future + Send + 'static,
    F::Output: Send + 'static,
{
    RUNTIME.spawn(f)
}
```

**Step 2: Verify compilation**

Run: `cargo check -p gorp-ffi`
Expected: PASS (no errors)

**Step 3: Commit**

```bash
git add gorp-ffi/src/runtime.rs
git commit -m "feat(gorp-ffi): add embedded Tokio runtime"
```

---

## Task 4: Event Types and Callback Interface

**Files:**
- Create: `gorp-ffi/src/events.rs`
- Modify: `gorp-ffi/src/lib.rs`

**Step 1: Create events.rs**

Create `gorp-ffi/src/events.rs`:

```rust
// ABOUTME: FFI-safe event types and callback interface.
// ABOUTME: Streams agent events to Swift/Kotlin via callbacks.

use gorp_agent::AgentEvent;

/// FFI-safe error codes matching gorp-agent::ErrorCode
#[derive(Debug, Clone, Copy, uniffi::Enum)]
pub enum FfiErrorCode {
    Timeout,
    RateLimited,
    AuthFailed,
    SessionOrphaned,
    ToolFailed,
    PermissionDenied,
    BackendError,
    Unknown,
}

impl From<gorp_agent::ErrorCode> for FfiErrorCode {
    fn from(code: gorp_agent::ErrorCode) -> Self {
        match code {
            gorp_agent::ErrorCode::Timeout => FfiErrorCode::Timeout,
            gorp_agent::ErrorCode::RateLimited => FfiErrorCode::RateLimited,
            gorp_agent::ErrorCode::AuthFailed => FfiErrorCode::AuthFailed,
            gorp_agent::ErrorCode::SessionOrphaned => FfiErrorCode::SessionOrphaned,
            gorp_agent::ErrorCode::ToolFailed => FfiErrorCode::ToolFailed,
            gorp_agent::ErrorCode::PermissionDenied => FfiErrorCode::PermissionDenied,
            gorp_agent::ErrorCode::BackendError => FfiErrorCode::BackendError,
            gorp_agent::ErrorCode::Unknown => FfiErrorCode::Unknown,
        }
    }
}

/// FFI-safe usage statistics
#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: Option<u64>,
    pub cache_write_tokens: Option<u64>,
    pub cost_usd: Option<f64>,
}

impl From<gorp_agent::Usage> for FfiUsage {
    fn from(u: gorp_agent::Usage) -> Self {
        Self {
            input_tokens: u.input_tokens,
            output_tokens: u.output_tokens,
            cache_read_tokens: u.cache_read_tokens,
            cache_write_tokens: u.cache_write_tokens,
            cost_usd: u.cost_usd,
        }
    }
}

/// Callback interface implemented by Swift/Kotlin
#[uniffi::export(callback_interface)]
pub trait AgentEventCallback: Send + Sync {
    fn on_text(&self, text: String);
    fn on_tool_start(&self, id: String, name: String, input_json: String);
    fn on_tool_progress(&self, id: String, update_json: String);
    fn on_tool_end(
        &self,
        id: String,
        name: String,
        output_json: String,
        success: bool,
        duration_ms: u64,
    );
    fn on_result(&self, text: String, usage: Option<FfiUsage>, metadata_json: String);
    fn on_error(&self, code: FfiErrorCode, message: String, recoverable: bool);
    fn on_session_invalid(&self, reason: String);
    fn on_session_changed(&self, new_session_id: String);
    fn on_custom(&self, kind: String, payload_json: String);
}

/// Dispatch a gorp-agent event to the callback
pub fn dispatch_event(callback: &dyn AgentEventCallback, event: AgentEvent) {
    match event {
        AgentEvent::Text(text) => callback.on_text(text),
        AgentEvent::ToolStart { id, name, input } => {
            callback.on_tool_start(id, name, input.to_string());
        }
        AgentEvent::ToolProgress { id, update } => {
            callback.on_tool_progress(id, update.to_string());
        }
        AgentEvent::ToolEnd {
            id,
            name,
            output,
            success,
            duration_ms,
        } => {
            callback.on_tool_end(id, name, output.to_string(), success, duration_ms);
        }
        AgentEvent::Result {
            text,
            usage,
            metadata,
        } => {
            callback.on_result(text, usage.map(Into::into), metadata.to_string());
        }
        AgentEvent::Error {
            code,
            message,
            recoverable,
        } => {
            callback.on_error(code.into(), message, recoverable);
        }
        AgentEvent::SessionInvalid { reason } => {
            callback.on_session_invalid(reason);
        }
        AgentEvent::SessionChanged { new_session_id } => {
            callback.on_session_changed(new_session_id);
        }
        AgentEvent::Custom { kind, payload } => {
            callback.on_custom(kind, payload.to_string());
        }
    }
}
```

**Step 2: Update lib.rs**

Add to `gorp-ffi/src/lib.rs`:

```rust
mod events;

pub use events::{AgentEventCallback, FfiErrorCode, FfiUsage};
```

**Step 3: Verify compilation**

Run: `cargo check -p gorp-ffi`
Expected: PASS

**Step 4: Commit**

```bash
git add gorp-ffi/src/events.rs gorp-ffi/src/lib.rs
git commit -m "feat(gorp-ffi): add event types and callback interface"
```

---

## Task 5: Agent Handle Wrapper

**Files:**
- Create: `gorp-ffi/src/agent.rs`
- Modify: `gorp-ffi/src/lib.rs`

**Step 1: Create agent.rs**

Create `gorp-ffi/src/agent.rs`:

```rust
// ABOUTME: FFI wrappers for AgentHandle and AgentRegistry.
// ABOUTME: Main interface for creating agents and sending prompts.

use crate::error::FfiError;
use crate::events::{dispatch_event, AgentEventCallback};
use crate::runtime::{block_on, spawn};
use gorp_agent::{AgentHandle, AgentRegistry};
use std::sync::Arc;

/// FFI-safe wrapper around AgentHandle
#[derive(uniffi::Object)]
pub struct FfiAgentHandle {
    inner: AgentHandle,
}

#[uniffi::export]
impl FfiAgentHandle {
    /// Get the backend name
    pub fn name(&self) -> String {
        self.inner.name().to_string()
    }

    /// Create a new session, returns session ID
    pub fn new_session(&self) -> Result<String, FfiError> {
        block_on(self.inner.new_session()).map_err(Into::into)
    }

    /// Load an existing session by ID
    pub fn load_session(&self, session_id: String) -> Result<(), FfiError> {
        block_on(self.inner.load_session(&session_id)).map_err(Into::into)
    }

    /// Send a prompt with streaming callback
    ///
    /// Returns immediately. Events are delivered via callback on background thread.
    pub fn prompt(
        &self,
        session_id: String,
        text: String,
        callback: Arc<dyn AgentEventCallback>,
    ) -> Result<(), FfiError> {
        let handle = self.inner.clone();

        spawn(async move {
            match handle.prompt(&session_id, &text).await {
                Ok(mut receiver) => {
                    while let Some(event) = receiver.recv().await {
                        dispatch_event(callback.as_ref(), event);
                    }
                }
                Err(e) => {
                    callback.on_error(
                        crate::events::FfiErrorCode::Unknown,
                        e.to_string(),
                        false,
                    );
                }
            }
        });

        Ok(())
    }

    /// Cancel an in-progress prompt
    pub fn cancel(&self, session_id: String) -> Result<(), FfiError> {
        block_on(self.inner.cancel(&session_id)).map_err(Into::into)
    }

    /// Abandon a session that was created but never used
    pub fn abandon_session(&self, session_id: String) {
        self.inner.abandon_session(&session_id);
    }
}

/// FFI-safe wrapper around AgentRegistry
#[derive(uniffi::Object)]
pub struct FfiAgentRegistry {
    inner: AgentRegistry,
}

#[uniffi::export]
impl FfiAgentRegistry {
    /// Create a new registry with all available backends
    #[uniffi::constructor]
    pub fn new() -> Self {
        Self {
            inner: AgentRegistry::default(),
        }
    }

    /// List available backend names
    pub fn available_backends(&self) -> Vec<String> {
        self.inner
            .available()
            .into_iter()
            .map(String::from)
            .collect()
    }

    /// Create a backend by name with JSON configuration
    pub fn create(&self, name: String, config_json: String) -> Result<Arc<FfiAgentHandle>, FfiError> {
        let config: serde_json::Value = serde_json::from_str(&config_json)?;
        let handle = self.inner.create(&name, &config)?;
        Ok(Arc::new(FfiAgentHandle { inner: handle }))
    }
}
```

**Step 2: Update lib.rs**

Add to `gorp-ffi/src/lib.rs`:

```rust
mod agent;

pub use agent::{FfiAgentHandle, FfiAgentRegistry};
```

**Step 3: Verify compilation**

Run: `cargo check -p gorp-ffi`
Expected: PASS

**Step 4: Commit**

```bash
git add gorp-ffi/src/agent.rs gorp-ffi/src/lib.rs
git commit -m "feat(gorp-ffi): add agent handle and registry wrappers"
```

---

## Task 6: Session Store Wrapper

**Files:**
- Create: `gorp-ffi/src/session.rs`
- Modify: `gorp-ffi/src/lib.rs`

**Step 1: Create session.rs**

Create `gorp-ffi/src/session.rs`:

```rust
// ABOUTME: FFI wrapper for SessionStore.
// ABOUTME: Provides SQLite-backed session/channel persistence.

use crate::error::FfiError;
use gorp_core::session::{Channel, SessionStore};
use std::sync::Arc;

/// FFI-safe channel record
#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiChannel {
    pub channel_name: String,
    pub room_id: String,
    pub session_id: String,
    pub directory: String,
    pub started: bool,
    pub created_at: String,
    pub backend_type: Option<String>,
}

impl From<Channel> for FfiChannel {
    fn from(c: Channel) -> Self {
        Self {
            channel_name: c.channel_name,
            room_id: c.room_id,
            session_id: c.session_id,
            directory: c.directory,
            started: c.started,
            created_at: c.created_at,
            backend_type: c.backend_type,
        }
    }
}

/// FFI wrapper for SessionStore
#[derive(uniffi::Object)]
pub struct FfiSessionStore {
    inner: SessionStore,
}

#[uniffi::export]
impl FfiSessionStore {
    /// Create/open a session store at the given workspace path
    #[uniffi::constructor]
    pub fn new(workspace_path: String) -> Result<Arc<Self>, FfiError> {
        let store = SessionStore::new(&workspace_path)
            .map_err(|e| FfiError::DatabaseError(e.to_string()))?;
        Ok(Arc::new(Self { inner: store }))
    }

    /// Create a new channel
    pub fn create_channel(
        &self,
        channel_name: String,
        room_id: String,
    ) -> Result<FfiChannel, FfiError> {
        let channel = self
            .inner
            .create_channel(&channel_name, &room_id)
            .map_err(|e| FfiError::DatabaseError(e.to_string()))?;
        Ok(channel.into())
    }

    /// Get channel by name
    pub fn get_by_name(&self, channel_name: String) -> Result<Option<FfiChannel>, FfiError> {
        let channel = self
            .inner
            .get_by_name(&channel_name)
            .map_err(|e| FfiError::DatabaseError(e.to_string()))?;
        Ok(channel.map(Into::into))
    }

    /// Get channel by room ID
    pub fn get_by_room(&self, room_id: String) -> Result<Option<FfiChannel>, FfiError> {
        let channel = self
            .inner
            .get_by_room(&room_id)
            .map_err(|e| FfiError::DatabaseError(e.to_string()))?;
        Ok(channel.map(Into::into))
    }

    /// Get channel by session ID
    pub fn get_by_session_id(&self, session_id: String) -> Result<Option<FfiChannel>, FfiError> {
        let channel = self
            .inner
            .get_by_session_id(&session_id)
            .map_err(|e| FfiError::DatabaseError(e.to_string()))?;
        Ok(channel.map(Into::into))
    }

    /// List all channels
    pub fn list_all(&self) -> Result<Vec<FfiChannel>, FfiError> {
        let channels = self
            .inner
            .list_all()
            .map_err(|e| FfiError::DatabaseError(e.to_string()))?;
        Ok(channels.into_iter().map(Into::into).collect())
    }

    /// Mark a channel as started
    pub fn mark_started(&self, room_id: String) -> Result<(), FfiError> {
        self.inner
            .mark_started(&room_id)
            .map_err(|e| FfiError::DatabaseError(e.to_string()))
    }

    /// Delete a channel by name
    pub fn delete_channel(&self, channel_name: String) -> Result<(), FfiError> {
        self.inner
            .delete_channel(&channel_name)
            .map_err(|e| FfiError::DatabaseError(e.to_string()))
    }

    /// Delete a channel by room ID
    pub fn delete_by_room(&self, room_id: String) -> Result<Option<String>, FfiError> {
        self.inner
            .delete_by_room(&room_id)
            .map_err(|e| FfiError::DatabaseError(e.to_string()))
    }

    /// Reset a channel's session
    pub fn reset_session(
        &self,
        channel_name: String,
        new_session_id: String,
    ) -> Result<(), FfiError> {
        self.inner
            .reset_session(&channel_name, &new_session_id)
            .map_err(|e| FfiError::DatabaseError(e.to_string()))
    }

    /// Update backend type for a channel
    pub fn update_backend_type(
        &self,
        channel_name: String,
        backend_type: Option<String>,
    ) -> Result<(), FfiError> {
        self.inner
            .update_backend_type(&channel_name, backend_type.as_deref())
            .map_err(|e| FfiError::DatabaseError(e.to_string()))
    }

    /// Get a setting value
    pub fn get_setting(&self, key: String) -> Result<Option<String>, FfiError> {
        self.inner
            .get_setting(&key)
            .map_err(|e| FfiError::DatabaseError(e.to_string()))
    }

    /// Set a setting value
    pub fn set_setting(&self, key: String, value: String) -> Result<(), FfiError> {
        self.inner
            .set_setting(&key, &value)
            .map_err(|e| FfiError::DatabaseError(e.to_string()))
    }
}

impl FfiSessionStore {
    /// Get the inner SessionStore (for SchedulerStore creation)
    pub(crate) fn inner(&self) -> &SessionStore {
        &self.inner
    }
}
```

**Step 2: Update lib.rs**

Add to `gorp-ffi/src/lib.rs`:

```rust
mod session;

pub use session::{FfiChannel, FfiSessionStore};
```

**Step 3: Verify compilation**

Run: `cargo check -p gorp-ffi`
Expected: PASS

**Step 4: Commit**

```bash
git add gorp-ffi/src/session.rs gorp-ffi/src/lib.rs
git commit -m "feat(gorp-ffi): add session store wrapper"
```

---

## Task 7: Scheduler Store Wrapper

**Files:**
- Create: `gorp-ffi/src/scheduler.rs`
- Modify: `gorp-ffi/src/lib.rs`

**Step 1: Create scheduler.rs**

Create `gorp-ffi/src/scheduler.rs`:

```rust
// ABOUTME: FFI wrapper for SchedulerStore.
// ABOUTME: Provides cron-based prompt scheduling.

use crate::error::FfiError;
use crate::session::FfiSessionStore;
use gorp_core::scheduler::{ScheduleStatus, ScheduledPrompt, SchedulerStore};
use std::sync::Arc;

/// FFI-safe schedule status
#[derive(Debug, Clone, Copy, uniffi::Enum)]
pub enum FfiScheduleStatus {
    Active,
    Paused,
    Completed,
    Failed,
    Executing,
    Cancelled,
}

impl From<ScheduleStatus> for FfiScheduleStatus {
    fn from(s: ScheduleStatus) -> Self {
        match s {
            ScheduleStatus::Active => FfiScheduleStatus::Active,
            ScheduleStatus::Paused => FfiScheduleStatus::Paused,
            ScheduleStatus::Completed => FfiScheduleStatus::Completed,
            ScheduleStatus::Failed => FfiScheduleStatus::Failed,
            ScheduleStatus::Executing => FfiScheduleStatus::Executing,
            ScheduleStatus::Cancelled => FfiScheduleStatus::Cancelled,
        }
    }
}

/// FFI-safe scheduled prompt record
#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiScheduledPrompt {
    pub id: String,
    pub channel_name: String,
    pub room_id: String,
    pub prompt: String,
    pub created_by: String,
    pub created_at: String,
    pub execute_at: Option<String>,
    pub cron_expression: Option<String>,
    pub last_executed_at: Option<String>,
    pub next_execution_at: String,
    pub status: FfiScheduleStatus,
    pub error_message: Option<String>,
    pub execution_count: i32,
}

impl From<ScheduledPrompt> for FfiScheduledPrompt {
    fn from(s: ScheduledPrompt) -> Self {
        Self {
            id: s.id,
            channel_name: s.channel_name,
            room_id: s.room_id,
            prompt: s.prompt,
            created_by: s.created_by,
            created_at: s.created_at,
            execute_at: s.execute_at,
            cron_expression: s.cron_expression,
            last_executed_at: s.last_executed_at,
            next_execution_at: s.next_execution_at,
            status: s.status.into(),
            error_message: s.error_message,
            execution_count: s.execution_count,
        }
    }
}

/// FFI wrapper for SchedulerStore
#[derive(uniffi::Object)]
pub struct FfiSchedulerStore {
    inner: SchedulerStore,
}

#[uniffi::export]
impl FfiSchedulerStore {
    /// Create a scheduler store that shares the session store's database
    #[uniffi::constructor]
    pub fn new(session_store: &FfiSessionStore) -> Result<Arc<Self>, FfiError> {
        let db = session_store.inner().db_connection();
        let store = SchedulerStore::new(db);
        store
            .initialize_schema()
            .map_err(|e| FfiError::DatabaseError(e.to_string()))?;
        Ok(Arc::new(Self { inner: store }))
    }

    /// List all scheduled prompts
    pub fn list_all(&self) -> Result<Vec<FfiScheduledPrompt>, FfiError> {
        let schedules = self
            .inner
            .list_all()
            .map_err(|e| FfiError::DatabaseError(e.to_string()))?;
        Ok(schedules.into_iter().map(Into::into).collect())
    }

    /// List schedules for a specific room
    pub fn list_by_room(&self, room_id: String) -> Result<Vec<FfiScheduledPrompt>, FfiError> {
        let schedules = self
            .inner
            .list_by_room(&room_id)
            .map_err(|e| FfiError::DatabaseError(e.to_string()))?;
        Ok(schedules.into_iter().map(Into::into).collect())
    }

    /// List schedules for a specific channel
    pub fn list_by_channel(&self, channel_name: String) -> Result<Vec<FfiScheduledPrompt>, FfiError> {
        let schedules = self
            .inner
            .list_by_channel(&channel_name)
            .map_err(|e| FfiError::DatabaseError(e.to_string()))?;
        Ok(schedules.into_iter().map(Into::into).collect())
    }

    /// Get a schedule by ID
    pub fn get_by_id(&self, id: String) -> Result<Option<FfiScheduledPrompt>, FfiError> {
        let schedule = self
            .inner
            .get_by_id(&id)
            .map_err(|e| FfiError::DatabaseError(e.to_string()))?;
        Ok(schedule.map(Into::into))
    }

    /// Delete a schedule by ID
    pub fn delete_schedule(&self, id: String) -> Result<bool, FfiError> {
        self.inner
            .delete_schedule(&id)
            .map_err(|e| FfiError::DatabaseError(e.to_string()))
    }

    /// Pause a schedule
    pub fn pause_schedule(&self, id: String) -> Result<bool, FfiError> {
        self.inner
            .pause_schedule(&id)
            .map_err(|e| FfiError::DatabaseError(e.to_string()))
    }

    /// Resume a paused schedule
    pub fn resume_schedule(&self, id: String) -> Result<bool, FfiError> {
        self.inner
            .resume_schedule(&id)
            .map_err(|e| FfiError::DatabaseError(e.to_string()))
    }

    /// Cancel a schedule
    pub fn cancel_schedule(&self, id: String) -> Result<bool, FfiError> {
        self.inner
            .cancel_schedule(&id)
            .map_err(|e| FfiError::DatabaseError(e.to_string()))
    }
}
```

**Step 2: Update lib.rs**

Add to `gorp-ffi/src/lib.rs`:

```rust
mod scheduler;

pub use scheduler::{FfiScheduleStatus, FfiScheduledPrompt, FfiSchedulerStore};
```

**Step 3: Verify compilation**

Run: `cargo check -p gorp-ffi`
Expected: PASS

**Step 4: Commit**

```bash
git add gorp-ffi/src/scheduler.rs gorp-ffi/src/lib.rs
git commit -m "feat(gorp-ffi): add scheduler store wrapper"
```

---

## Task 8: Build Configuration

**Files:**
- Create: `gorp-ffi/build.rs`
- Create: `gorp-ffi/uniffi.toml`

**Step 1: Create build.rs**

Create `gorp-ffi/build.rs`:

```rust
fn main() {
    uniffi::generate_scaffolding("src/gorp_ffi.udl").unwrap();
}
```

**Step 2: Create UDL file**

Create `gorp-ffi/src/gorp_ffi.udl`:

```udl
namespace gorp_ffi {};
```

Note: With UniFFI proc macros, we only need a minimal UDL. The `#[uniffi::export]` macros handle most of the interface definition.

**Step 3: Create uniffi.toml**

Create `gorp-ffi/uniffi.toml`:

```toml
[bindings.swift]
module_name = "GorpFFI"
generate_immutable_records = true

[bindings.kotlin]
package_name = "com.gorp.ffi"
```

**Step 4: Verify build**

Run: `cargo build -p gorp-ffi`
Expected: PASS (library builds successfully)

**Step 5: Commit**

```bash
git add gorp-ffi/build.rs gorp-ffi/src/gorp_ffi.udl gorp-ffi/uniffi.toml
git commit -m "feat(gorp-ffi): add UniFFI build configuration"
```

---

## Task 9: Integration Test

**Files:**
- Create: `gorp-ffi/tests/integration_test.rs`

**Step 1: Create integration test**

Create `gorp-ffi/tests/integration_test.rs`:

```rust
// ABOUTME: Integration tests for gorp-ffi.
// ABOUTME: Tests the FFI layer without generating actual bindings.

use gorp_ffi::{FfiAgentRegistry, FfiSessionStore};
use std::sync::Arc;
use tempfile::TempDir;

#[test]
fn test_registry_lists_backends() {
    let registry = FfiAgentRegistry::new();
    let backends = registry.available_backends();

    assert!(backends.contains(&"mock".to_string()));
    assert!(backends.contains(&"direct".to_string()));
    // acp and mux depend on feature flags
}

#[test]
fn test_create_mock_backend() {
    let registry = FfiAgentRegistry::new();
    let config = r#"{"responses": ["Hello!"]}"#;

    let handle = registry.create("mock".to_string(), config.to_string());
    assert!(handle.is_ok());

    let handle = handle.unwrap();
    assert_eq!(handle.name(), "mock");
}

#[test]
fn test_session_store_crud() {
    let temp_dir = TempDir::new().unwrap();
    let workspace_path = temp_dir.path().to_string_lossy().to_string();

    let store = FfiSessionStore::new(workspace_path).unwrap();

    // Create channel
    let channel = store
        .create_channel("test-channel".to_string(), "!room:example.com".to_string())
        .unwrap();
    assert_eq!(channel.channel_name, "test-channel");
    assert!(!channel.started);

    // Get by name
    let found = store.get_by_name("test-channel".to_string()).unwrap();
    assert!(found.is_some());
    assert_eq!(found.unwrap().room_id, "!room:example.com");

    // List all
    let all = store.list_all().unwrap();
    assert_eq!(all.len(), 1);

    // Mark started
    store.mark_started("!room:example.com".to_string()).unwrap();
    let updated = store.get_by_name("test-channel".to_string()).unwrap();
    assert!(updated.unwrap().started);

    // Delete
    store.delete_channel("test-channel".to_string()).unwrap();
    let deleted = store.get_by_name("test-channel".to_string()).unwrap();
    assert!(deleted.is_none());
}

#[test]
fn test_mock_backend_new_session() {
    let registry = FfiAgentRegistry::new();
    let config = r#"{"responses": ["Hello!"]}"#;
    let handle = registry.create("mock".to_string(), config.to_string()).unwrap();

    let session_id = handle.new_session().unwrap();
    assert!(!session_id.is_empty());
}
```

**Step 2: Run tests**

Run: `cargo test -p gorp-ffi`
Expected: All tests PASS

**Step 3: Commit**

```bash
git add gorp-ffi/tests/
git commit -m "test(gorp-ffi): add integration tests"
```

---

## Task 10: Final lib.rs Cleanup

**Files:**
- Modify: `gorp-ffi/src/lib.rs`

**Step 1: Final lib.rs**

Update `gorp-ffi/src/lib.rs` to be complete:

```rust
// ABOUTME: UniFFI bindings for gorp-agent and gorp-core.
// ABOUTME: Enables Swift/Kotlin integration for native apps.

mod agent;
mod error;
mod events;
mod runtime;
mod scheduler;
mod session;

pub use agent::{FfiAgentHandle, FfiAgentRegistry};
pub use error::FfiError;
pub use events::{AgentEventCallback, FfiErrorCode, FfiUsage};
pub use scheduler::{FfiScheduleStatus, FfiScheduledPrompt, FfiSchedulerStore};
pub use session::{FfiChannel, FfiSessionStore};

uniffi::setup_scaffolding!();
```

**Step 2: Verify full build**

Run: `cargo build -p gorp-ffi && cargo test -p gorp-ffi`
Expected: Build and all tests PASS

**Step 3: Final commit**

```bash
git add gorp-ffi/src/lib.rs
git commit -m "feat(gorp-ffi): finalize public API exports"
```

---

## Task 11: Generate Swift Bindings (Verification)

**Step 1: Install uniffi-bindgen if needed**

Run: `cargo install uniffi_bindgen --version 0.28`

**Step 2: Generate Swift bindings**

Run: `cargo run -p uniffi-bindgen generate --library target/debug/libgorp_ffi.dylib --language swift --out-dir gorp-ffi/bindings/swift`

Or use cargo-uniffi if available:
Run: `cargo uniffi-bindgen generate gorp-ffi/src/gorp_ffi.udl --language swift --out-dir gorp-ffi/bindings/swift`

**Step 3: Verify bindings generated**

Check that `gorp-ffi/bindings/swift/gorp_ffi.swift` exists and contains the expected types.

**Step 4: Commit bindings**

```bash
git add gorp-ffi/bindings/
git commit -m "build(gorp-ffi): generate Swift bindings"
```

---

## Task 12: Update BBS Thread

**Step 1: Post response to BuddyAgent thread**

Use BBS MCP to post a response:

```
Topic: gorp-rs
Thread: c0454eb6-8ab5-423d-a4d4-52043a505110
Message: ## gorp-ffi v0.1.0 Released

We've implemented UniFFI bindings for gorp-rs! The new `gorp-ffi` crate provides:

### Exposed API
- `FfiAgentRegistry` - create backends by name (mock, direct, acp, mux)
- `FfiAgentHandle` - session management, prompts with streaming callback
- `AgentEventCallback` - implement in Swift/Kotlin to receive events
- `FfiSessionStore` - SQLite channel/session persistence
- `FfiSchedulerStore` - cron-based prompt scheduling

### Usage
```swift
let registry = FfiAgentRegistry()
let agent = try registry.create(name: "mux", configJson: "{}")
let sessionId = try agent.newSession()
agent.prompt(sessionId: sessionId, text: "Hello!", callback: myHandler)
```

### Build Output
- `libgorp_ffi.a` / `.dylib`
- `gorp_ffi.swift` (generated)

Let us know how the migration from mux-ffi goes!

-- gorp-dev@mcp
```

---

Plan complete and saved to `docs/plans/2026-01-01-gorp-ffi-implementation.md`. Two execution options:

**1. Subagent-Driven (this session)** - I dispatch fresh subagent per task, review between tasks, fast iteration

**2. Parallel Session (separate)** - Open new session with executing-plans, batch execution with checkpoints

Which approach?
