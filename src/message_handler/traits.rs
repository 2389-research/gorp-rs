// ABOUTME: Traits for abstracting Matrix room operations
// ABOUTME: Enables testing command handlers without real Matrix connections
//
// NOTE: This module contains a local `MessageSender` abstraction that predates
// the gorp_core::traits::ChatChannel abstraction. For new code, prefer using
// `ChatChannel` directly. The `MessageSender` trait is kept for backwards
// compatibility with existing command handlers.
//
// Migration path:
// - New code should use `gorp_core::traits::ChatChannel`
// - Existing code using `MessageSender` can be migrated incrementally
// - `platform::matrix::MatrixChannel` implements `ChatChannel`

use anyhow::Result;
use async_trait::async_trait;

/// Trait for sending messages to a room
/// Abstracts Matrix room operations for testability
///
/// DEPRECATED: Prefer `gorp_core::traits::ChatChannel` for new code.
/// This trait is kept for backwards compatibility with existing command handlers.
#[async_trait]
pub trait MessageSender: Send + Sync {
    /// Send a plain text message
    async fn send_text(&self, msg: &str) -> Result<()>;

    /// Send a message with both plain and HTML content
    async fn send_html(&self, plain: &str, html: &str) -> Result<()>;

    /// Send typing indicator
    async fn typing(&self, typing: bool) -> Result<()>;

    /// Get room ID as string
    fn room_id(&self) -> &str;

    /// Check if this is a direct message room
    async fn is_dm(&self) -> bool;
}

// =============================================================================
// Matrix Room Implementation
// =============================================================================

use matrix_sdk::room::Room;
use matrix_sdk::ruma::events::room::message::RoomMessageEventContent;
use matrix_sdk::RoomState;

/// Wrapper around Matrix Room that implements MessageSender
pub struct MatrixRoom {
    room: Room,
    room_id_str: String,
}

impl MatrixRoom {
    pub fn new(room: Room) -> Self {
        let room_id_str = room.room_id().to_string();
        Self { room, room_id_str }
    }

    pub fn inner(&self) -> &Room {
        &self.room
    }

    pub fn into_inner(self) -> Room {
        self.room
    }

    pub fn is_joined(&self) -> bool {
        self.room.state() == RoomState::Joined
    }
}

#[async_trait]
impl MessageSender for MatrixRoom {
    async fn send_text(&self, msg: &str) -> Result<()> {
        self.room
            .send(RoomMessageEventContent::text_plain(msg))
            .await?;
        Ok(())
    }

    async fn send_html(&self, plain: &str, html: &str) -> Result<()> {
        self.room
            .send(RoomMessageEventContent::text_html(plain, html))
            .await?;
        Ok(())
    }

    async fn typing(&self, typing: bool) -> Result<()> {
        self.room.typing_notice(typing).await?;
        Ok(())
    }

    fn room_id(&self) -> &str {
        &self.room_id_str
    }

    async fn is_dm(&self) -> bool {
        self.room.is_direct().await.unwrap_or(false)
    }
}

// =============================================================================
// Mock Implementation for Testing
// =============================================================================

use std::sync::{Arc, Mutex};

/// Mock room for testing command handlers
#[derive(Default, Clone)]
pub struct MockRoom {
    pub room_id: String,
    pub is_dm: bool,
    pub messages: Arc<Mutex<Vec<MockMessage>>>,
    pub typing_state: Arc<Mutex<bool>>,
}

/// A captured message from MockRoom
#[derive(Debug, Clone, PartialEq)]
pub struct MockMessage {
    pub plain: String,
    pub html: Option<String>,
}

impl MockRoom {
    pub fn new(room_id: &str) -> Self {
        Self {
            room_id: room_id.to_string(),
            is_dm: false,
            messages: Arc::new(Mutex::new(Vec::new())),
            typing_state: Arc::new(Mutex::new(false)),
        }
    }

    pub fn dm(room_id: &str) -> Self {
        Self {
            room_id: room_id.to_string(),
            is_dm: true,
            messages: Arc::new(Mutex::new(Vec::new())),
            typing_state: Arc::new(Mutex::new(false)),
        }
    }

    /// Get all messages sent to this room
    pub fn get_messages(&self) -> Vec<MockMessage> {
        self.messages.lock().unwrap().clone()
    }

    /// Get the last message sent
    pub fn last_message(&self) -> Option<MockMessage> {
        self.messages.lock().unwrap().last().cloned()
    }

    /// Check if any message contains the given text
    pub fn has_message_containing(&self, text: &str) -> bool {
        self.messages
            .lock()
            .unwrap()
            .iter()
            .any(|m| m.plain.contains(text))
    }

    /// Clear all messages
    pub fn clear(&self) {
        self.messages.lock().unwrap().clear();
    }
}

#[async_trait]
impl MessageSender for MockRoom {
    async fn send_text(&self, msg: &str) -> Result<()> {
        self.messages.lock().unwrap().push(MockMessage {
            plain: msg.to_string(),
            html: None,
        });
        Ok(())
    }

    async fn send_html(&self, plain: &str, html: &str) -> Result<()> {
        self.messages.lock().unwrap().push(MockMessage {
            plain: plain.to_string(),
            html: Some(html.to_string()),
        });
        Ok(())
    }

    async fn typing(&self, typing: bool) -> Result<()> {
        *self.typing_state.lock().unwrap() = typing;
        Ok(())
    }

    fn room_id(&self) -> &str {
        &self.room_id
    }

    async fn is_dm(&self) -> bool {
        self.is_dm
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_room_send_text() {
        let room = MockRoom::new("!test:matrix.org");
        room.send_text("Hello").await.unwrap();

        assert_eq!(room.get_messages().len(), 1);
        assert_eq!(room.last_message().unwrap().plain, "Hello");
        assert!(room.last_message().unwrap().html.is_none());
    }

    #[tokio::test]
    async fn test_mock_room_send_html() {
        let room = MockRoom::new("!test:matrix.org");
        room.send_html("Hello", "<b>Hello</b>").await.unwrap();

        let msg = room.last_message().unwrap();
        assert_eq!(msg.plain, "Hello");
        assert_eq!(msg.html, Some("<b>Hello</b>".to_string()));
    }

    #[tokio::test]
    async fn test_mock_room_has_message_containing() {
        let room = MockRoom::new("!test:matrix.org");
        room.send_text("Error: something went wrong").await.unwrap();

        assert!(room.has_message_containing("Error"));
        assert!(room.has_message_containing("wrong"));
        assert!(!room.has_message_containing("success"));
    }

    #[tokio::test]
    async fn test_mock_room_dm() {
        let room = MockRoom::dm("!dm:matrix.org");
        assert!(room.is_dm().await);

        let room2 = MockRoom::new("!channel:matrix.org");
        assert!(!room2.is_dm().await);
    }
}

// =============================================================================
// ChatChannel Adapter - Bridge to new abstraction
// =============================================================================

use gorp_core::traits::{ChatChannel, MessageContent, TypingIndicator};

/// Adapter that wraps a `ChatChannel` to implement `MessageSender`
///
/// This enables gradual migration from `MessageSender` to `ChatChannel`.
/// Use this when you have a `ChatChannel` but need to pass it to code
/// expecting a `MessageSender`.
pub struct ChannelAdapter<C: ChatChannel> {
    channel: C,
    channel_id: String,
}

impl<C: ChatChannel> ChannelAdapter<C> {
    pub fn new(channel: C) -> Self {
        let channel_id = channel.id().to_string();
        Self { channel, channel_id }
    }

    /// Get the underlying channel
    pub fn inner(&self) -> &C {
        &self.channel
    }
}

#[async_trait]
impl<C: ChatChannel + 'static> MessageSender for ChannelAdapter<C> {
    async fn send_text(&self, msg: &str) -> Result<()> {
        self.channel.send(MessageContent::plain(msg)).await
    }

    async fn send_html(&self, plain: &str, html: &str) -> Result<()> {
        self.channel.send(MessageContent::html(plain, html)).await
    }

    async fn typing(&self, typing: bool) -> Result<()> {
        if let Some(indicator) = self.channel.typing_indicator() {
            indicator.set_typing(typing).await
        } else {
            // No-op if typing not supported
            Ok(())
        }
    }

    fn room_id(&self) -> &str {
        &self.channel_id
    }

    async fn is_dm(&self) -> bool {
        self.channel.is_direct().await
    }
}
