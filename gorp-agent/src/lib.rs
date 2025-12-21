// ABOUTME: Pluggable agent backend abstraction for gorp.
// ABOUTME: Provides trait-based backends (ACP, direct CLI, mock) with Send+Sync handles.

pub mod config;
pub mod event;
pub mod handle;
pub mod registry;
pub mod traits;

pub mod backends;
pub mod testing;

// Re-exports
pub use config::{BackendConfig, Config};
pub use event::{AgentEvent, ErrorCode, Usage};
pub use handle::{AgentHandle, EventReceiver, SessionState};
pub use registry::{AgentRegistry, BackendFactory};
pub use traits::AgentBackend;
