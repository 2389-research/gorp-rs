// ABOUTME: Platform-agnostic channel wrapper for the message handler
// ABOUTME: Bridges dyn MessagingPlatform into ChatChannel for command handler reuse

use anyhow::Result;
use async_trait::async_trait;
use gorp_core::traits::{
    AttachmentHandler, ChatChannel, MessageContent, MessagingPlatform, TypingIndicator,
};

/// A platform-agnostic channel implementation that wraps a `MessagingPlatform`.
///
/// Allows the command handler (which requires `ChatChannel`) to work with any
/// platform through the `MessagingPlatform::send()` method. Features like typing
/// indicators and attachments gracefully degrade to no-ops.
#[derive(Clone)]
pub struct GenericChannel<'a> {
    platform: &'a dyn MessagingPlatform,
    channel_id: String,
    is_dm: bool,
}

impl<'a> GenericChannel<'a> {
    pub fn new(platform: &'a dyn MessagingPlatform, channel_id: &str, is_dm: bool) -> Self {
        Self {
            platform,
            channel_id: channel_id.to_string(),
            is_dm,
        }
    }
}

impl<'a> std::fmt::Debug for GenericChannel<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GenericChannel")
            .field("platform_id", &self.platform.platform_id())
            .field("channel_id", &self.channel_id)
            .field("is_dm", &self.is_dm)
            .finish()
    }
}

#[async_trait]
impl<'a> ChatChannel for GenericChannel<'a> {
    fn id(&self) -> &str {
        &self.channel_id
    }

    fn name(&self) -> Option<String> {
        None
    }

    async fn is_direct(&self) -> bool {
        self.is_dm
    }

    async fn send(&self, content: MessageContent) -> Result<()> {
        self.platform.send(&self.channel_id, content).await
    }

    fn typing_indicator(&self) -> Option<&dyn TypingIndicator> {
        None
    }

    fn attachment_handler(&self) -> Option<&dyn AttachmentHandler> {
        None
    }

    async fn member_count(&self) -> Result<usize> {
        Ok(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gorp_core::traits::EventStream;
    use std::sync::Mutex;

    struct TestPlatform {
        sent: Mutex<Vec<(String, MessageContent)>>,
    }

    impl TestPlatform {
        fn new() -> Self {
            Self {
                sent: Mutex::new(Vec::new()),
            }
        }

        fn sent_messages(&self) -> Vec<(String, MessageContent)> {
            self.sent.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl MessagingPlatform for TestPlatform {
        async fn event_stream(&self) -> Result<EventStream> {
            Ok(Box::pin(tokio_stream::empty()))
        }

        async fn send(&self, channel_id: &str, content: MessageContent) -> Result<()> {
            self.sent
                .lock()
                .unwrap()
                .push((channel_id.to_string(), content));
            Ok(())
        }

        fn bot_user_id(&self) -> &str {
            "@bot:test"
        }

        fn platform_id(&self) -> &'static str {
            "test"
        }
    }

    #[test]
    fn test_generic_channel_id() {
        let platform = TestPlatform::new();
        let channel = GenericChannel::new(&platform, "chan-123", false);
        assert_eq!(channel.id(), "chan-123");
    }

    #[test]
    fn test_generic_channel_name_is_none() {
        let platform = TestPlatform::new();
        let channel = GenericChannel::new(&platform, "chan-123", false);
        assert!(channel.name().is_none());
    }

    #[tokio::test]
    async fn test_generic_channel_is_direct() {
        let platform = TestPlatform::new();
        let regular = GenericChannel::new(&platform, "chan-123", false);
        assert!(!regular.is_direct().await);

        let dm = GenericChannel::new(&platform, "dm-123", true);
        assert!(dm.is_direct().await);
    }

    #[tokio::test]
    async fn test_generic_channel_send_delegates_to_platform() {
        let platform = TestPlatform::new();
        let channel = GenericChannel::new(&platform, "chan-123", false);

        channel.send(MessageContent::plain("hello")).await.unwrap();

        let msgs = platform.sent_messages();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].0, "chan-123");
        assert!(matches!(&msgs[0].1, MessageContent::Plain(s) if s == "hello"));
    }

    #[test]
    fn test_generic_channel_typing_indicator_none() {
        let platform = TestPlatform::new();
        let channel = GenericChannel::new(&platform, "chan-123", false);
        assert!(channel.typing_indicator().is_none());
    }

    #[test]
    fn test_generic_channel_attachment_handler_none() {
        let platform = TestPlatform::new();
        let channel = GenericChannel::new(&platform, "chan-123", false);
        assert!(channel.attachment_handler().is_none());
    }

    #[tokio::test]
    async fn test_generic_channel_member_count_zero() {
        let platform = TestPlatform::new();
        let channel = GenericChannel::new(&platform, "chan-123", false);
        assert_eq!(channel.member_count().await.unwrap(), 0);
    }

    #[test]
    fn test_generic_channel_debug() {
        let platform = TestPlatform::new();
        let channel = GenericChannel::new(&platform, "chan-123", true);
        let debug = format!("{:?}", channel);
        assert!(debug.contains("test"));
        assert!(debug.contains("chan-123"));
        assert!(debug.contains("true"));
    }
}
