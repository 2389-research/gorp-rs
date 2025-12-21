// ABOUTME: Pluggable agent backend abstraction for gorp.
// ABOUTME: Provides trait-based backends (ACP, direct CLI, mock) with Send+Sync handles.

pub mod config;
pub mod event;
pub mod traits;
pub mod handle;
pub mod registry;

pub mod backends;
pub mod testing;

// Re-exports
pub use config::{Config, BackendConfig};
pub use event::{AgentEvent, ErrorCode, Usage};
pub use traits::AgentBackend;
pub use handle::{AgentHandle, EventReceiver, SessionState};
pub use registry::{AgentRegistry, BackendFactory};
