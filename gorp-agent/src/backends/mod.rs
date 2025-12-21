// ABOUTME: Backend implementations (ACP, direct CLI, mock).
// ABOUTME: Each backend implements AgentBackend trait.

pub mod mock;

#[cfg(feature = "acp")]
pub mod acp;

pub mod direct_cli;
pub mod direct_codex;
