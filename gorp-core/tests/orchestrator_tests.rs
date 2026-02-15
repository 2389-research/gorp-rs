// ABOUTME: Integration tests for gorp-core orchestrator
// ABOUTME: Uses mock ChatInterface implementation

use async_trait::async_trait;
use gorp_core::traits::{ChatInterface, ChatRoom, ChatUser, IncomingMessage, MessageContent};
use std::sync::{Arc, Mutex};

/// Mock room that records sent messages
#[derive(Debug, Clone)]
pub struct MockRoom {
    id: String,
    name: Option<String>,
    is_dm: bool,
    sent_messages: Arc<Mutex<Vec<MessageContent>>>,
    typing_state: Arc<Mutex<bool>>,
}

impl MockRoom {
    pub fn new(id: &str) -> Self {
        Self {
            id: id.to_string(),
            name: Some(format!("Room {}", id)),
            is_dm: false,
            sent_messages: Arc::new(Mutex::new(Vec::new())),
            typing_state: Arc::new(Mutex::new(false)),
        }
    }

    pub fn dm(id: &str) -> Self {
        Self {
            id: id.to_string(),
            name: None,
            is_dm: true,
            sent_messages: Arc::new(Mutex::new(Vec::new())),
            typing_state: Arc::new(Mutex::new(false)),
        }
    }

    pub fn sent_messages(&self) -> Vec<MessageContent> {
        self.sent_messages.lock().unwrap().clone()
    }

    pub fn is_typing(&self) -> bool {
        *self.typing_state.lock().unwrap()
    }
}

#[async_trait]
impl ChatRoom for MockRoom {
    fn id(&self) -> &str {
        &self.id
    }

    fn name(&self) -> Option<String> {
        self.name.clone()
    }

    async fn is_direct_message(&self) -> bool {
        self.is_dm
    }

    async fn send(&self, content: MessageContent) -> anyhow::Result<()> {
        self.sent_messages.lock().unwrap().push(content);
        Ok(())
    }

    async fn set_typing(&self, typing: bool) -> anyhow::Result<()> {
        *self.typing_state.lock().unwrap() = typing;
        Ok(())
    }

    async fn download_attachment(
        &self,
        _source_id: &str,
    ) -> anyhow::Result<(String, Vec<u8>, String)> {
        Ok((
            "test.txt".to_string(),
            vec![1, 2, 3],
            "text/plain".to_string(),
        ))
    }
}

/// Mock interface for testing
pub struct MockInterface {
    rooms: std::collections::HashMap<String, MockRoom>,
    bot_id: String,
}

impl MockInterface {
    pub fn new() -> Self {
        Self {
            rooms: std::collections::HashMap::new(),
            bot_id: "@bot:test.com".to_string(),
        }
    }

    pub fn with_bot_id(bot_id: &str) -> Self {
        Self {
            rooms: std::collections::HashMap::new(),
            bot_id: bot_id.to_string(),
        }
    }

    pub fn add_room(&mut self, room: MockRoom) {
        self.rooms.insert(room.id.clone(), room);
    }

    pub fn get_room_ref(&self, room_id: &str) -> Option<&MockRoom> {
        self.rooms.get(room_id)
    }
}

impl Default for MockInterface {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ChatInterface for MockInterface {
    type Room = MockRoom;

    async fn get_room(&self, room_id: &str) -> Option<Self::Room> {
        self.rooms.get(room_id).cloned()
    }

    fn bot_user_id(&self) -> &str {
        &self.bot_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gorp_core::commands::{parse_message, ParseResult};

    #[test]
    fn test_mock_room_creation() {
        let room = MockRoom::new("!test:example.com");
        assert_eq!(room.id(), "!test:example.com");
        assert_eq!(room.name(), Some("Room !test:example.com".to_string()));
        assert!(room.sent_messages().is_empty());
    }

    #[test]
    fn test_mock_dm_room() {
        let room = MockRoom::dm("!dm:example.com");
        assert!(room.name.is_none());
        assert!(room.is_dm);
    }

    #[tokio::test]
    async fn test_mock_room_send() {
        let room = MockRoom::new("!test:example.com");
        room.send(MessageContent::plain("Hello")).await.unwrap();
        room.send(MessageContent::html("World", "<b>World</b>"))
            .await
            .unwrap();

        let messages = room.sent_messages();
        assert_eq!(messages.len(), 2);
        assert!(matches!(&messages[0], MessageContent::Plain(s) if s == "Hello"));
    }

    #[tokio::test]
    async fn test_mock_room_typing() {
        let room = MockRoom::new("!test:example.com");
        assert!(!room.is_typing());

        room.set_typing(true).await.unwrap();
        assert!(room.is_typing());

        room.set_typing(false).await.unwrap();
        assert!(!room.is_typing());
    }

    #[tokio::test]
    async fn test_mock_interface_get_room() {
        let mut interface = MockInterface::new();
        let room = MockRoom::new("!test:example.com");
        interface.add_room(room);

        let found = interface.get_room("!test:example.com").await;
        assert!(found.is_some());
        assert_eq!(found.unwrap().id(), "!test:example.com");

        let not_found = interface.get_room("!other:example.com").await;
        assert!(not_found.is_none());
    }

    #[test]
    fn test_mock_interface_bot_id() {
        let interface = MockInterface::new();
        assert_eq!(interface.bot_user_id(), "@bot:test.com");

        let custom = MockInterface::with_bot_id("@claude:matrix.org");
        assert_eq!(custom.bot_user_id(), "@claude:matrix.org");
    }

    #[test]
    fn test_mock_interface_is_self() {
        let interface = MockInterface::new();
        assert!(interface.is_self("@bot:test.com"));
        assert!(!interface.is_self("@user:test.com"));
    }

    #[test]
    fn test_command_parsing_with_prefix() {
        let result = parse_message("!claude help", "!claude");
        assert!(matches!(result, ParseResult::Command(cmd) if cmd.name == "help"));
    }

    #[test]
    fn test_command_parsing_bang_prefix() {
        let result = parse_message("!help", "!claude");
        assert!(matches!(result, ParseResult::Command(cmd) if cmd.name == "help"));
    }

    #[test]
    fn test_command_parsing_not_command() {
        let result = parse_message("hello world", "!claude");
        assert!(matches!(result, ParseResult::Message(_)));
    }

    #[test]
    fn test_command_with_args() {
        let result = parse_message("!backend set mux", "!claude");
        if let ParseResult::Command(cmd) = result {
            assert_eq!(cmd.name, "backend");
            assert_eq!(cmd.args, vec!["set", "mux"]);
            assert_eq!(cmd.raw_args, "set mux");
        } else {
            panic!("Expected command");
        }
    }

    #[test]
    fn test_incoming_message_creation() {
        let msg = IncomingMessage {
            platform_id: "matrix".to_string(),
            channel_id: "!test:example.com".to_string(),
            thread_id: None,
            sender: ChatUser::with_name("@user:test.com", "Test User"),
            body: "Hello, bot!".to_string(),
            is_direct: false,
            formatted: false,
            attachment: None,
            event_id: "$event123".to_string(),
            timestamp: 1234567890,
        };

        assert_eq!(msg.room_id(), "!test:example.com");
        assert_eq!(msg.sender.id, "@user:test.com");
        assert_eq!(msg.sender.display_name, Some("Test User".to_string()));
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

    #[tokio::test]
    async fn test_mock_room_is_direct_message() {
        let regular = MockRoom::new("!room:test.com");
        assert!(!regular.is_direct_message().await);

        let dm = MockRoom::dm("!dm:test.com");
        assert!(dm.is_direct_message().await);
    }

    #[tokio::test]
    async fn test_mock_room_download_attachment() {
        let room = MockRoom::new("!test:example.com");
        let (filename, data, mime_type) = room.download_attachment("test-id").await.unwrap();

        assert_eq!(filename, "test.txt");
        assert_eq!(data, vec![1, 2, 3]);
        assert_eq!(mime_type, "text/plain");
    }

    #[test]
    fn test_message_content_variants() {
        let plain = MessageContent::plain("Hello");
        assert!(matches!(plain, MessageContent::Plain(s) if s == "Hello"));

        let html = MessageContent::html("Hello", "<b>Hello</b>");
        assert!(
            matches!(html, MessageContent::Html { plain, html } if plain == "Hello" && html == "<b>Hello</b>")
        );
    }
}
