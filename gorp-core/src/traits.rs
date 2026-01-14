// ABOUTME: Core traits for tiered platform abstraction
// ABOUTME: Tier 1 (MessagingPlatform), Tier 2 (ChatPlatform), Tier 3 (LocalInterface)

use anyhow::Result;
use async_trait::async_trait;
use std::fmt::Debug;
use std::pin::Pin;
use tokio_stream::Stream;

// =============================================================================
// Message Content Types
// =============================================================================

/// Content that can be sent to a chat channel
#[derive(Debug, Clone)]
pub enum MessageContent {
    /// Plain text message
    Plain(String),
    /// Message with both plain text and HTML formatting
    Html { plain: String, html: String },
    /// Message with an attachment
    Attachment {
        filename: String,
        data: Vec<u8>,
        mime_type: String,
        caption: Option<String>,
    },
}

impl MessageContent {
    pub fn plain(text: impl Into<String>) -> Self {
        Self::Plain(text.into())
    }

    pub fn html(plain: impl Into<String>, html: impl Into<String>) -> Self {
        Self::Html {
            plain: plain.into(),
            html: html.into(),
        }
    }
}

/// Information about an attachment in an incoming message
#[derive(Debug, Clone)]
pub struct AttachmentInfo {
    /// Platform-specific source identifier for downloading
    pub source_id: String,
    /// Original filename
    pub filename: String,
    /// MIME type
    pub mime_type: String,
    /// File size in bytes, if known
    pub size: Option<u64>,
}

// =============================================================================
// User Identity
// =============================================================================

/// Identity of a chat user
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ChatUser {
    /// Unique identifier (e.g., @user:matrix.org, U12345678)
    pub id: String,
    /// Display name
    pub display_name: Option<String>,
}

impl ChatUser {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            display_name: None,
        }
    }

    pub fn with_name(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            display_name: Some(name.into()),
        }
    }
}

// =============================================================================
// Incoming Message
// =============================================================================

/// Incoming message from a chat platform
#[derive(Debug, Clone)]
pub struct IncomingMessage {
    /// The channel/room this message was sent in
    pub channel_id: String,
    /// The user who sent the message
    pub sender: ChatUser,
    /// Message body (text content)
    pub body: String,
    /// Whether this is a direct message (1:1 conversation)
    pub is_direct: bool,
    /// Whether this message is formatted (HTML, markdown, etc.)
    pub formatted: bool,
    /// Attachment info if present
    pub attachment: Option<AttachmentInfo>,
    /// Platform-specific event ID
    pub event_id: String,
    /// Timestamp in seconds since Unix epoch
    pub timestamp: i64,
}

impl IncomingMessage {
    /// Backwards-compatible accessor for room_id
    pub fn room_id(&self) -> &str {
        &self.channel_id
    }
}

// =============================================================================
// Tier 1: Messaging Platform (Control Plane Only)
// =============================================================================

/// Boxed stream type for platform events
pub type EventStream = Pin<Box<dyn Stream<Item = IncomingMessage> + Send>>;

/// Tier 1: Minimum platform interface for control plane access.
///
/// Platforms implementing only this trait can receive messages and send responses,
/// but cannot create channels or have workspace-linked conversations. Messages
/// from Tier 1 platforms are routed to DISPATCH control plane only.
///
/// Examples: WhatsApp, SMS, Telegram (basic mode)
#[async_trait]
pub trait MessagingPlatform: Send + Sync {
    /// Receive incoming messages as a stream
    async fn event_stream(&self) -> Result<EventStream>;

    /// Send a message to a channel by ID
    async fn send(&self, channel_id: &str, content: MessageContent) -> Result<()>;

    /// Bot's user ID on this platform
    fn bot_user_id(&self) -> &str;

    /// Platform identifier (e.g., "matrix", "slack", "whatsapp")
    fn platform_id(&self) -> &'static str;

    /// Check if a user ID is the bot itself
    fn is_self(&self, user_id: &str) -> bool {
        user_id == self.bot_user_id()
    }
}

// =============================================================================
// Tier 2: Full Chat Platform
// =============================================================================

/// Tier 2: Full chat platform with channel management.
///
/// Platforms implementing this trait can have channels linked to workspaces,
/// enabling full Claude conversations with context. DMs still route to DISPATCH.
///
/// Examples: Matrix, Slack, Discord
#[async_trait]
pub trait ChatPlatform: MessagingPlatform {
    /// The channel type for this platform
    type Channel: ChatChannel;

    /// Get a channel by its ID with full capabilities
    async fn get_channel(&self, id: &str) -> Option<Self::Channel>;

    /// List all joined channels
    async fn joined_channels(&self) -> Vec<Self::Channel>;

    /// Optional: channel creation capability (create rooms/channels)
    fn channel_creator(&self) -> Option<&dyn ChannelCreator> {
        None
    }

    /// Optional: channel management (join/leave/invite)
    fn channel_manager(&self) -> Option<&dyn ChannelManager> {
        None
    }

    /// Optional: encryption support
    fn encryption(&self) -> Option<&dyn EncryptedPlatform> {
        None
    }
}

/// A chat channel (room, channel, conversation) on a platform
#[async_trait]
pub trait ChatChannel: Send + Sync + Debug + Clone {
    /// Unique identifier for this channel
    fn id(&self) -> &str;

    /// Human-readable name of the channel, if available
    fn name(&self) -> Option<String>;

    /// Whether this is a direct message (1:1) conversation
    async fn is_direct(&self) -> bool;

    /// Send a message to this channel
    async fn send(&self, content: MessageContent) -> Result<()>;

    /// Optional: typing indicator support
    fn typing_indicator(&self) -> Option<&dyn TypingIndicator> {
        None
    }

    /// Optional: attachment download support
    fn attachment_handler(&self) -> Option<&dyn AttachmentHandler> {
        None
    }

    /// Get member count (defaults to unknown)
    async fn member_count(&self) -> Result<usize> {
        Ok(0)
    }
}

// =============================================================================
// Tier 3: Local Interface
// =============================================================================

/// Workspace info for local interfaces
#[derive(Debug, Clone)]
pub struct WorkspaceInfo {
    /// Workspace name/identifier
    pub name: String,
    /// Directory path
    pub path: String,
    /// Whether this workspace is currently active
    pub active: bool,
}

/// Tier 3: Local interface for direct workspace access.
///
/// Local interfaces (TUI, GUI) don't have the channel abstraction layer.
/// They select workspaces directly and interact with Claude.
#[async_trait]
pub trait LocalInterface: Send + Sync {
    /// List available workspaces
    async fn list_workspaces(&self) -> Result<Vec<WorkspaceInfo>>;

    /// Select/activate a workspace by name
    async fn select_workspace(&self, name: &str) -> Result<()>;

    /// Get the currently active workspace
    fn active_workspace(&self) -> Option<&str>;

    /// Access DISPATCH control plane
    async fn dispatch(&self, command: &str) -> Result<String>;
}

// =============================================================================
// Optional Capabilities
// =============================================================================

/// Channel creation capability (not all platforms support this)
#[async_trait]
pub trait ChannelCreator: Send + Sync {
    /// Create a new channel/room
    async fn create_channel(&self, name: &str) -> Result<String>;

    /// Create a direct message channel with a user
    async fn create_dm(&self, user_id: &str) -> Result<String>;
}

/// Channel management capability
#[async_trait]
pub trait ChannelManager: Send + Sync {
    /// Join a channel by ID
    async fn join(&self, channel_id: &str) -> Result<()>;

    /// Leave a channel
    async fn leave(&self, channel_id: &str) -> Result<()>;

    /// Invite a user to a channel
    async fn invite(&self, channel_id: &str, user_id: &str) -> Result<()>;

    /// Get members of a channel
    async fn members(&self, channel_id: &str) -> Result<Vec<ChatUser>>;
}

/// Typing indicator capability
#[async_trait]
pub trait TypingIndicator: Send + Sync {
    /// Set typing indicator on/off
    async fn set_typing(&self, typing: bool) -> Result<()>;
}

/// Attachment handling capability
#[async_trait]
pub trait AttachmentHandler: Send + Sync {
    /// Download an attachment by its source identifier
    /// Returns (filename, data, mime_type)
    async fn download(&self, source_id: &str) -> Result<(String, Vec<u8>, String)>;
}

/// Encryption capability (platform-specific)
#[async_trait]
pub trait EncryptedPlatform: Send + Sync {
    /// Set up encryption for the platform
    async fn setup_encryption(&self) -> Result<()>;

    /// Verify a device (platform-specific meaning)
    async fn verify_device(&self, device_id: &str) -> Result<()>;

    /// Check if encryption is enabled
    fn is_encrypted(&self) -> bool;
}

// =============================================================================
// Backwards Compatibility - Deprecated Traits
// =============================================================================

/// Deprecated: Use ChatChannel instead
#[async_trait]
pub trait ChatRoom: Send + Sync + Debug + Clone {
    fn id(&self) -> &str;
    fn name(&self) -> Option<String>;
    async fn is_direct_message(&self) -> bool;
    async fn send(&self, content: MessageContent) -> Result<()>;
    async fn set_typing(&self, typing: bool) -> Result<()>;
    async fn download_attachment(&self, source_id: &str) -> Result<(String, Vec<u8>, String)>;
}

/// Deprecated: Use ChatPlatform instead
#[async_trait]
pub trait ChatInterface: Send + Sync {
    type Room: ChatRoom;
    async fn get_room(&self, room_id: &str) -> Option<Self::Room>;
    fn bot_user_id(&self) -> &str;
    fn is_self(&self, user_id: &str) -> bool {
        user_id == self.bot_user_id()
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_content_plain() {
        let content = MessageContent::plain("Hello");
        assert!(matches!(content, MessageContent::Plain(s) if s == "Hello"));
    }

    #[test]
    fn test_message_content_html() {
        let content = MessageContent::html("Hello", "<b>Hello</b>");
        assert!(
            matches!(content, MessageContent::Html { plain, html } if plain == "Hello" && html == "<b>Hello</b>")
        );
    }

    #[test]
    fn test_chat_user_new() {
        let user = ChatUser::new("@test:example.com");
        assert_eq!(user.id, "@test:example.com");
        assert!(user.display_name.is_none());
    }

    #[test]
    fn test_chat_user_with_name() {
        let user = ChatUser::with_name("@test:example.com", "Test User");
        assert_eq!(user.id, "@test:example.com");
        assert_eq!(user.display_name, Some("Test User".to_string()));
    }

    #[test]
    fn test_incoming_message_room_id_compat() {
        let msg = IncomingMessage {
            channel_id: "!room:example.com".to_string(),
            sender: ChatUser::new("@user:example.com"),
            body: "test".to_string(),
            is_direct: false,
            formatted: false,
            attachment: None,
            event_id: "evt1".to_string(),
            timestamp: 0,
        };
        assert_eq!(msg.room_id(), "!room:example.com");
    }
}
