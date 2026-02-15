// ABOUTME: Core traits for tiered platform abstraction
// ABOUTME: Tier 1 (MessagingPlatform), Tier 2 (ChatPlatform), Tier 3 (LocalInterface)

use anyhow::Result;
use async_trait::async_trait;
use std::fmt::Debug;
use std::pin::Pin;
use std::time::Duration;
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
    /// Which platform this message came from (e.g., "matrix", "telegram", "slack")
    pub platform_id: String,
    /// The channel/room this message was sent in
    pub channel_id: String,
    /// Thread identifier for platforms that support threading (Slack thread_ts, WhatsApp quoted message ID)
    pub thread_id: Option<String>,
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

/// Connection state reported by each platform for health checks and monitoring
#[derive(Debug, Clone)]
pub enum PlatformConnectionState {
    /// Platform is connected and processing events
    Connected,
    /// Platform is establishing connection
    Connecting,
    /// Platform lost connection
    Disconnected { reason: String },
    /// Platform requires authentication (e.g., WhatsApp QR scan, Matrix device verify)
    AuthRequired,
    /// Platform is rate-limited by the upstream API
    RateLimited { retry_after: Duration },
}

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

    /// Gracefully shut down the platform connection
    async fn shutdown(&self) -> Result<()> {
        Ok(())
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

    /// Report current connection state for health monitoring
    fn connection_state(&self) -> PlatformConnectionState {
        PlatformConnectionState::Connected
    }

    /// Optional: threaded conversation support
    fn threading(&self) -> Option<&dyn ThreadedPlatform> {
        None
    }

    /// Optional: slash command support
    fn slash_commands(&self) -> Option<&dyn SlashCommandProvider> {
        None
    }

    /// Optional: rich formatting support
    fn rich_formatter(&self) -> Option<&dyn RichFormatter> {
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
// Extension Traits (optional platform capabilities)
// =============================================================================

/// Platforms that support threaded conversations (e.g., Slack)
#[async_trait]
pub trait ThreadedPlatform: Send + Sync {
    /// Send a message as a reply within a specific thread
    async fn send_threaded(
        &self,
        channel_id: &str,
        thread_ts: &str,
        content: MessageContent,
    ) -> Result<()>;
}

/// Definition of a slash command supported by a platform
#[derive(Debug, Clone)]
pub struct SlashCommandDef {
    /// Command name (e.g., "/gorp")
    pub name: String,
    /// Human-readable description
    pub description: String,
}

/// Invocation of a slash command from a user
#[derive(Debug, Clone)]
pub struct SlashCommandInvocation {
    /// The command that was invoked (e.g., "/gorp")
    pub command: String,
    /// Text following the command
    pub text: String,
    /// Channel where the command was invoked
    pub channel_id: String,
    /// User who invoked the command
    pub user_id: String,
    /// URL for deferred response delivery
    pub response_url: String,
}

/// Platforms that support slash commands (e.g., Slack)
#[async_trait]
pub trait SlashCommandProvider: Send + Sync {
    /// List registered slash commands
    fn registered_commands(&self) -> Vec<SlashCommandDef>;
    /// Handle a slash command invocation
    async fn handle_command(&self, cmd: SlashCommandInvocation) -> Result<MessageContent>;
}

/// Platforms that support rich formatted output (e.g., Slack Block Kit)
/// format_as_blocks is infallible -- always returns valid formatted output, falling back to raw text on parse failure
pub trait RichFormatter: Send + Sync {
    /// Convert content to platform-specific rich format (e.g., Block Kit JSON)
    fn format_as_blocks(&self, content: &str) -> serde_json::Value;
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
            platform_id: "matrix".to_string(),
            channel_id: "!room:example.com".to_string(),
            thread_id: None,
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

    #[test]
    fn test_incoming_message_platform_id() {
        let msg = IncomingMessage {
            platform_id: "telegram".to_string(),
            channel_id: "12345".to_string(),
            thread_id: None,
            sender: ChatUser::new("67890"),
            body: "hello".to_string(),
            is_direct: true,
            formatted: false,
            attachment: None,
            event_id: "msg_1".to_string(),
            timestamp: 1700000000,
        };
        assert_eq!(msg.platform_id, "telegram");
        assert!(msg.thread_id.is_none());
    }

    #[test]
    fn test_incoming_message_thread_id() {
        let msg = IncomingMessage {
            platform_id: "slack".to_string(),
            channel_id: "C123".to_string(),
            thread_id: Some("1700000000.000100".to_string()),
            sender: ChatUser::new("U456"),
            body: "threaded reply".to_string(),
            is_direct: false,
            formatted: false,
            attachment: None,
            event_id: "msg_2".to_string(),
            timestamp: 1700000001,
        };
        assert_eq!(msg.thread_id.as_deref(), Some("1700000000.000100"));
    }

    #[test]
    fn test_platform_connection_state_variants() {
        let connected = PlatformConnectionState::Connected;
        assert!(matches!(connected, PlatformConnectionState::Connected));

        let connecting = PlatformConnectionState::Connecting;
        assert!(matches!(connecting, PlatformConnectionState::Connecting));

        let disconnected = PlatformConnectionState::Disconnected {
            reason: "timeout".to_string(),
        };
        assert!(
            matches!(disconnected, PlatformConnectionState::Disconnected { reason } if reason == "timeout")
        );

        let auth = PlatformConnectionState::AuthRequired;
        assert!(matches!(auth, PlatformConnectionState::AuthRequired));

        let rate_limited = PlatformConnectionState::RateLimited {
            retry_after: std::time::Duration::from_secs(30),
        };
        assert!(
            matches!(rate_limited, PlatformConnectionState::RateLimited { retry_after } if retry_after.as_secs() == 30)
        );
    }

    #[test]
    fn test_platform_connection_state_debug() {
        let state = PlatformConnectionState::Connected;
        let debug = format!("{:?}", state);
        assert!(debug.contains("Connected"));
    }

    // =========================================================================
    // Extension Trait Tests
    // =========================================================================

    #[test]
    fn test_slash_command_def_construction() {
        let def = SlashCommandDef {
            name: "/gorp".to_string(),
            description: "Talk to gorp".to_string(),
        };
        assert_eq!(def.name, "/gorp");
        assert_eq!(def.description, "Talk to gorp");
    }

    #[test]
    fn test_slash_command_def_clone() {
        let def = SlashCommandDef {
            name: "/status".to_string(),
            description: "Check status".to_string(),
        };
        let cloned = def.clone();
        assert_eq!(cloned.name, def.name);
        assert_eq!(cloned.description, def.description);
    }

    #[test]
    fn test_slash_command_def_debug() {
        let def = SlashCommandDef {
            name: "/gorp".to_string(),
            description: "Talk to gorp".to_string(),
        };
        let debug = format!("{:?}", def);
        assert!(debug.contains("/gorp"));
        assert!(debug.contains("Talk to gorp"));
    }

    #[test]
    fn test_slash_command_invocation_construction() {
        let inv = SlashCommandInvocation {
            command: "/gorp".to_string(),
            text: "hello there".to_string(),
            channel_id: "C12345".to_string(),
            user_id: "U67890".to_string(),
            response_url: "https://hooks.slack.com/commands/T123/456/abc".to_string(),
        };
        assert_eq!(inv.command, "/gorp");
        assert_eq!(inv.text, "hello there");
        assert_eq!(inv.channel_id, "C12345");
        assert_eq!(inv.user_id, "U67890");
        assert_eq!(
            inv.response_url,
            "https://hooks.slack.com/commands/T123/456/abc"
        );
    }

    #[test]
    fn test_slash_command_invocation_clone() {
        let inv = SlashCommandInvocation {
            command: "/ask".to_string(),
            text: "what is rust".to_string(),
            channel_id: "C999".to_string(),
            user_id: "U111".to_string(),
            response_url: "https://example.com/respond".to_string(),
        };
        let cloned = inv.clone();
        assert_eq!(cloned.command, inv.command);
        assert_eq!(cloned.text, inv.text);
        assert_eq!(cloned.channel_id, inv.channel_id);
        assert_eq!(cloned.user_id, inv.user_id);
        assert_eq!(cloned.response_url, inv.response_url);
    }

    #[test]
    fn test_slash_command_invocation_debug() {
        let inv = SlashCommandInvocation {
            command: "/gorp".to_string(),
            text: "test".to_string(),
            channel_id: "C1".to_string(),
            user_id: "U1".to_string(),
            response_url: "https://example.com".to_string(),
        };
        let debug = format!("{:?}", inv);
        assert!(debug.contains("/gorp"));
        assert!(debug.contains("test"));
    }

    /// Test that ChatPlatform default extension accessors return None.
    /// Uses a minimal stub implementing ChatPlatform to verify the defaults.
    #[derive(Debug, Clone)]
    struct StubChannel {
        id: String,
    }

    #[async_trait]
    impl ChatChannel for StubChannel {
        fn id(&self) -> &str {
            &self.id
        }
        fn name(&self) -> Option<String> {
            None
        }
        async fn is_direct(&self) -> bool {
            false
        }
        async fn send(&self, _content: MessageContent) -> Result<()> {
            Ok(())
        }
    }

    struct StubPlatform;

    #[async_trait]
    impl MessagingPlatform for StubPlatform {
        async fn event_stream(&self) -> Result<EventStream> {
            anyhow::bail!("stub")
        }
        async fn send(&self, _channel_id: &str, _content: MessageContent) -> Result<()> {
            Ok(())
        }
        fn bot_user_id(&self) -> &str {
            "bot"
        }
        fn platform_id(&self) -> &'static str {
            "stub"
        }
    }

    #[async_trait]
    impl ChatPlatform for StubPlatform {
        type Channel = StubChannel;
        async fn get_channel(&self, _id: &str) -> Option<StubChannel> {
            None
        }
        async fn joined_channels(&self) -> Vec<StubChannel> {
            vec![]
        }
    }

    #[test]
    fn test_chat_platform_threading_default_none() {
        let platform = StubPlatform;
        assert!(platform.threading().is_none());
    }

    #[test]
    fn test_chat_platform_slash_commands_default_none() {
        let platform = StubPlatform;
        assert!(platform.slash_commands().is_none());
    }

    #[test]
    fn test_chat_platform_rich_formatter_default_none() {
        let platform = StubPlatform;
        assert!(platform.rich_formatter().is_none());
    }
}
