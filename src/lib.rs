// ABOUTME: Root library module exposing all public modules
// ABOUTME: Provides access to config, session, matrix client, and webhook modules
pub mod admin;
pub mod config;
pub mod matrix_client;
pub mod mcp;
pub mod message_handler;
pub mod metrics;
pub mod paths;
pub mod scheduler;
pub mod session;
pub mod utils;
pub mod warm_session;
pub mod webhook;

// Re-export gorp-agent types for convenience
pub use gorp_agent::{AgentEvent, AgentHandle, AgentRegistry};
