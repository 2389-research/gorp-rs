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
