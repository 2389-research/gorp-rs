// ABOUTME: Platform abstraction module for gorp
// ABOUTME: Re-exports platform implementations (Matrix, future: Slack, Discord, etc.)

pub mod matrix;

// Re-export platform implementations for convenient access
pub use matrix::{
    // Client functions
    create_client, create_dm_room, create_room, invite_user, login,
    // Platform types (new abstraction layer)
    MatrixChannel, MatrixPlatform,
};

// When new platforms are added:
// pub mod slack;
// pub mod discord;
// pub mod whatsapp;
