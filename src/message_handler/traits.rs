// ABOUTME: Mock implementations for testing message handlers
// ABOUTME: Provides MockChannel implementing ChatChannel for unit tests

use anyhow::Result;
use async_trait::async_trait;
use gorp_core::traits::{ChatChannel, MessageContent, TypingIndicator};
use std::sync::{Arc, Mutex};

// =============================================================================
// Mock Implementation for Testing
// =============================================================================

/// A captured message from MockChannel
#[derive(Debug, Clone, PartialEq)]
pub struct MockMessage {
    pub plain: String,
    pub html: Option<String>,
}

/// Mock channel for testing command handlers
/// Implements ChatChannel trait for use in tests
#[derive(Default, Clone)]
pub struct MockChannel {
    pub channel_id: String,
    pub channel_name: Option<String>,
    pub is_dm: bool,
    pub messages: Arc<Mutex<Vec<MockMessage>>>,
    pub typing_state: Arc<Mutex<bool>>,
}

impl MockChannel {
    pub fn new(channel_id: &str) -> Self {
        Self {
            channel_id: channel_id.to_string(),
            channel_name: None,
            is_dm: false,
            messages: Arc::new(Mutex::new(Vec::new())),
            typing_state: Arc::new(Mutex::new(false)),
        }
    }

    pub fn dm(channel_id: &str) -> Self {
        Self {
            channel_id: channel_id.to_string(),
            channel_name: None,
            is_dm: true,
            messages: Arc::new(Mutex::new(Vec::new())),
            typing_state: Arc::new(Mutex::new(false)),
        }
    }

    /// Get all messages sent to this channel
    pub fn get_messages(&self) -> Vec<MockMessage> {
        self.messages
            .lock()
            .expect("MockChannel messages mutex poisoned")
            .clone()
    }

    /// Get the last message sent
    pub fn last_message(&self) -> Option<MockMessage> {
        self.messages
            .lock()
            .expect("MockChannel messages mutex poisoned")
            .last()
            .cloned()
    }

    /// Check if any message contains the given text
    pub fn has_message_containing(&self, text: &str) -> bool {
        self.messages
            .lock()
            .expect("MockChannel messages mutex poisoned")
            .iter()
            .any(|m| m.plain.contains(text))
    }

    /// Clear all messages
    pub fn clear(&self) {
        self.messages
            .lock()
            .expect("MockChannel messages mutex poisoned")
            .clear();
    }
}

impl std::fmt::Debug for MockChannel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message_count = self.messages.lock().map(|m| m.len()).unwrap_or(0);
        f.debug_struct("MockChannel")
            .field("channel_id", &self.channel_id)
            .field("channel_name", &self.channel_name)
            .field("is_dm", &self.is_dm)
            .field("message_count", &message_count)
            .finish()
    }
}

#[async_trait]
impl ChatChannel for MockChannel {
    fn id(&self) -> &str {
        &self.channel_id
    }

    fn name(&self) -> Option<String> {
        self.channel_name.clone()
    }

    async fn is_direct(&self) -> bool {
        self.is_dm
    }

    async fn send(&self, content: MessageContent) -> Result<()> {
        let msg = match content {
            MessageContent::Plain(text) => MockMessage {
                plain: text,
                html: None,
            },
            MessageContent::Html { plain, html } => MockMessage {
                plain,
                html: Some(html),
            },
            MessageContent::Attachment {
                filename, caption, ..
            } => MockMessage {
                // Match MatrixChannel behavior: use caption if provided, otherwise filename
                plain: caption.unwrap_or(filename),
                html: None,
            },
        };
        self.messages
            .lock()
            .expect("MockChannel messages mutex poisoned")
            .push(msg);
        Ok(())
    }

    fn typing_indicator(&self) -> Option<&dyn TypingIndicator> {
        Some(self)
    }

    fn attachment_handler(&self) -> Option<&dyn gorp_core::traits::AttachmentHandler> {
        None
    }
}

#[async_trait]
impl TypingIndicator for MockChannel {
    async fn set_typing(&self, typing: bool) -> Result<()> {
        *self
            .typing_state
            .lock()
            .expect("MockChannel typing_state mutex poisoned") = typing;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_channel_send_text() {
        let channel = MockChannel::new("!test:matrix.org");
        channel.send(MessageContent::plain("Hello")).await.unwrap();

        assert_eq!(channel.get_messages().len(), 1);
        assert_eq!(channel.last_message().unwrap().plain, "Hello");
        assert!(channel.last_message().unwrap().html.is_none());
    }

    #[tokio::test]
    async fn test_mock_channel_send_html() {
        let channel = MockChannel::new("!test:matrix.org");
        channel
            .send(MessageContent::html("Hello", "<b>Hello</b>"))
            .await
            .unwrap();

        let msg = channel.last_message().unwrap();
        assert_eq!(msg.plain, "Hello");
        assert_eq!(msg.html, Some("<b>Hello</b>".to_string()));
    }

    #[tokio::test]
    async fn test_mock_channel_has_message_containing() {
        let channel = MockChannel::new("!test:matrix.org");
        channel
            .send(MessageContent::plain("Error: something went wrong"))
            .await
            .unwrap();

        assert!(channel.has_message_containing("Error"));
        assert!(channel.has_message_containing("wrong"));
        assert!(!channel.has_message_containing("success"));
    }

    #[tokio::test]
    async fn test_mock_channel_dm() {
        let channel = MockChannel::dm("!dm:matrix.org");
        assert!(channel.is_direct().await);

        let channel2 = MockChannel::new("!channel:matrix.org");
        assert!(!channel2.is_direct().await);
    }

    #[tokio::test]
    async fn test_mock_channel_typing() {
        let channel = MockChannel::new("!test:matrix.org");

        if let Some(indicator) = channel.typing_indicator() {
            indicator.set_typing(true).await.unwrap();
            assert!(*channel
                .typing_state
                .lock()
                .expect("MockChannel typing_state mutex poisoned"));

            indicator.set_typing(false).await.unwrap();
            assert!(!*channel
                .typing_state
                .lock()
                .expect("MockChannel typing_state mutex poisoned"));
        }
    }
}
