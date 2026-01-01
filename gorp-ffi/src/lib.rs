// ABOUTME: UniFFI bindings for gorp-agent and gorp-core.
// ABOUTME: Enables Swift/Kotlin integration for native apps.

mod agent;
mod error;
mod events;
mod runtime;

pub use agent::{FfiAgentHandle, FfiAgentRegistry};
pub use error::FfiError;
pub use events::{AgentEventCallback, FfiErrorCode, FfiUsage};

uniffi::setup_scaffolding!();
