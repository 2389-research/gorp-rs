// ABOUTME: Platform-agnostic chat orchestration for AI agents
// ABOUTME: Provides traits and core logic for any chat interface

pub mod traits;

// Re-export gorp-agent types
pub use gorp_agent::{AgentEvent, AgentHandle, AgentRegistry};
