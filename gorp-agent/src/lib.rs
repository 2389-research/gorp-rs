// ABOUTME: Pluggable agent backend abstraction for gorp.
// ABOUTME: Provides trait-based backends (ACP, direct CLI, mock) with Send+Sync handles.

pub mod event;
pub mod traits;
pub mod handle;
pub mod registry;

pub mod backends;
pub mod testing;

// Re-exports will be enabled as we implement each module
pub use event::{AgentEvent, ErrorCode, Usage};
pub use traits::AgentBackend;
pub use handle::{AgentHandle, EventReceiver};
// pub use registry::{AgentRegistry, BackendFactory};
