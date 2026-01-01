// ABOUTME: Core traits for platform-agnostic chat interfaces
// ABOUTME: ChatRoom, ChatInterface, ChatUser abstractions

use anyhow::Result;
use async_trait::async_trait;
use std::fmt::Debug;

/// Content that can be sent to a chat room
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

/// Abstraction over a chat room/channel
#[async_trait]
pub trait ChatRoom: Send + Sync + Debug + Clone {
    /// Unique identifier for this room (e.g., Matrix room ID, Slack channel ID)
    fn id(&self) -> &str;

    /// Human-readable name of the room, if available
    fn name(&self) -> Option<String>;

    /// Whether this is a direct message (1:1) conversation
    async fn is_direct_message(&self) -> bool;

    /// Send a message to this room
    async fn send(&self, content: MessageContent) -> Result<()>;

    /// Set typing indicator on/off
    async fn set_typing(&self, typing: bool) -> Result<()>;

    /// Download an attachment by its source identifier
    /// Returns (filename, data, mime_type)
    async fn download_attachment(&self, source_id: &str) -> Result<(String, Vec<u8>, String)>;
}

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

/// Incoming message from a chat platform
#[derive(Debug, Clone)]
pub struct IncomingMessage {
    /// The room this message was sent in
    pub room_id: String,
    /// The user who sent the message
    pub sender: ChatUser,
    /// Message body (text content)
    pub body: String,
    /// Whether this message is formatted (HTML, markdown, etc.)
    pub formatted: bool,
    /// Attachment info if present: (source_id, filename, mime_type)
    pub attachment: Option<(String, String, String)>,
    /// Platform-specific event ID
    pub event_id: String,
    /// Timestamp in seconds since Unix epoch
    pub timestamp: i64,
}

/// Interface for a chat platform (Matrix, Slack, Discord, etc.)
#[async_trait]
pub trait ChatInterface: Send + Sync {
    /// The room type for this platform
    type Room: ChatRoom;

    /// Get a room by its ID
    async fn get_room(&self, room_id: &str) -> Option<Self::Room>;

    /// Get the bot's own user ID
    fn bot_user_id(&self) -> &str;

    /// Check if a user ID is the bot itself
    fn is_self(&self, user_id: &str) -> bool {
        user_id == self.bot_user_id()
    }
}

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
        assert!(matches!(content, MessageContent::Html { plain, html }
            if plain == "Hello" && html == "<b>Hello</b>"));
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
}
