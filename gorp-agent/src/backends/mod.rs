// ABOUTME: Backend implementations (ACP, direct CLI, mux, mock).
// ABOUTME: Each backend implements AgentBackend trait.

pub mod mock;

#[cfg(feature = "acp")]
pub mod acp;

#[cfg(feature = "mux")]
pub mod mux;

pub mod direct_cli;
pub mod direct_codex;
