// ABOUTME: GUI module entry point - desktop app using iced framework
// ABOUTME: Provides run_gui() function and re-exports ServerState from crate::server

pub mod app;
pub mod components;
pub mod sync;
pub mod theme;
pub mod views;

use crate::config::Config;
use anyhow::Result;

// Re-export ServerState and RoomInfo from server module for backward compatibility
pub use crate::server::{RoomInfo, ServerState};

/// Launch the GUI application.
/// Initializes server components and runs iced application.
pub fn run_gui() -> Result<()> {
    tracing::info!("Starting gorp desktop GUI");

    // Initialize logging (simplified for GUI - just console)
    use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, Layer};

    let console_layer = fmt::layer()
        .pretty()
        .with_target(true)
        .with_filter(tracing_subscriber::EnvFilter::new(
            "warn,gorp=info,matrix_sdk_crypto=error",
        ));

    tracing_subscriber::registry()
        .with(console_layer)
        .init();

    // Load config
    dotenvy::dotenv().ok();
    let config = Config::load()?;

    // Run iced application
    app::run(config)
}
