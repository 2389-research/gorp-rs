// ABOUTME: Platform abstraction module for gorp
// ABOUTME: Re-exports platform implementations (Matrix, Telegram, Slack)

pub mod factory;
pub mod matrix;
pub mod registry;
#[cfg(feature = "slack")]
pub mod slack;
#[cfg(feature = "telegram")]
pub mod telegram;

// Re-export registry types
pub use registry::{PlatformHealth, PlatformRegistry, SharedPlatformRegistry};

// Re-export platform implementations for convenient access
pub use matrix::{
    // Client functions
    create_client,
    create_dm_room,
    create_room,
    invite_user,
    login,
    // Platform types (new abstraction layer)
    MatrixChannel,
    MatrixPlatform,
};

#[cfg(feature = "slack")]
pub use slack::{SlackChannel, SlackPlatform};
#[cfg(feature = "telegram")]
pub use telegram::{TelegramChannel, TelegramPlatform};
