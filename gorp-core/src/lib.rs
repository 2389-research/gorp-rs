// ABOUTME: Platform-agnostic chat orchestration for AI agents
// ABOUTME: Provides traits and core logic for any chat interface

pub mod commands;
pub mod config;
pub mod metrics;
pub mod orchestrator;
pub mod paths;
pub mod scheduler;
pub mod session;
pub mod traits;
pub mod utils;
pub mod warm_session;

// Re-export gorp-agent types
pub use gorp_agent::{AgentEvent, AgentHandle, AgentRegistry};
