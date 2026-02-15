// ABOUTME: Platform abstraction module for gorp
// ABOUTME: Re-exports platform implementations (Matrix, Telegram, Slack)

pub mod matrix;
pub mod registry;
pub mod slack;
pub mod telegram;

// Re-export registry types
pub use registry::{PlatformHealth, PlatformRegistry};

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

pub use slack::{SlackChannel, SlackPlatform};
pub use telegram::{TelegramChannel, TelegramPlatform};
