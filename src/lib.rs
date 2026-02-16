// ABOUTME: Root library module exposing all public modules
// ABOUTME: Provides access to config, session, matrix client, and webhook modules

// Server state module (shared between GUI and headless mode)
pub mod server;

// GUI module (desktop app) - only available with `gui` feature
#[cfg(feature = "gui")]
pub mod gui;

// TUI module (terminal interface) - only available with `tui` feature
#[cfg(feature = "tui")]
pub mod tui;

// Coven gateway provider - only available with `coven` feature
#[cfg(feature = "coven")]
pub mod coven;

// Core message bus types
pub mod bus;

// Platform abstraction layer
pub mod platform;

// Backwards compatibility: alias platform::matrix as matrix_client
pub use platform::matrix as matrix_client;

// Matrix-specific modules (stay local until migrated)
#[cfg(feature = "admin")]
pub mod admin;
pub mod dispatch_handler;
pub mod dispatch_system_prompt;
pub mod dispatch_tools;
pub mod matrix_interface;
pub mod mcp;
pub mod message_handler;
pub mod onboarding;
pub mod webhook;

// Keep local scheduler.rs - it has Matrix-specific execution code
// The core scheduling logic is in gorp_core::scheduler
pub mod scheduler;
pub mod task_executor;

// Re-export platform-agnostic modules from gorp-core
pub use gorp_core::config;
pub use gorp_core::metrics;
pub use gorp_core::paths;
pub use gorp_core::session;
pub use gorp_core::utils;
pub use gorp_core::warm_session;

// Re-export gorp-core traits and types
pub use gorp_core::commands;
pub use gorp_core::orchestrator;
pub use gorp_core::traits;

// Re-export gorp-agent types for convenience
pub use gorp_agent::{AgentEvent, AgentHandle, AgentRegistry};
