// ABOUTME: Platform-agnostic chat orchestration for AI agents
// ABOUTME: Provides traits and core logic for any chat interface

pub mod commands;
pub mod config;
pub mod dispatch_events;
pub mod metrics;
pub mod orchestrator;
pub mod paths;
pub mod scheduler;
pub mod session;
pub mod traits;
pub mod utils;
pub mod warm_session;

pub use dispatch_events::WorkerEvent;

// Re-export core traits for convenient access
pub use traits::{
    // Tier 1: Messaging Platform
    EventStream, MessagingPlatform,
    // Tier 2: Chat Platform
    ChatChannel, ChatPlatform,
    // Tier 3: Local Interface
    LocalInterface, WorkspaceInfo,
    // Optional Capabilities
    AttachmentHandler, ChannelCreator, ChannelManager, EncryptedPlatform, TypingIndicator,
    // Data Types
    AttachmentInfo, ChatUser, IncomingMessage, MessageContent,
    // Deprecated (backwards compatibility)
    ChatInterface, ChatRoom,
};

// Re-export gorp-agent types
pub use gorp_agent::{AgentEvent, AgentHandle, AgentRegistry};
