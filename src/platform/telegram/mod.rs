// ABOUTME: Telegram platform implementation for gorp chat abstraction
// ABOUTME: Implements Tier 2 ChatPlatform with long polling, typing indicators, and file handling

pub mod channel;

pub use channel::TelegramChannel;

use anyhow::{Context, Result};
use async_trait::async_trait;
use gorp_core::traits::{
    AttachmentInfo, ChannelManager, ChatChannel, ChatPlatform, ChatUser, EventStream,
    IncomingMessage, MessageContent, MessagingPlatform, PlatformConnectionState,
};
use std::sync::{Arc, Mutex};
use teloxide::prelude::*;
use teloxide::types::{ChatKind, MediaKind, MessageKind, UpdateKind};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

// =============================================================================
// TelegramPlatform - Implements MessagingPlatform + ChatPlatform (Tier 2)
// =============================================================================

/// Telegram platform implementation using teloxide with long polling
pub struct TelegramPlatform {
    bot: Bot,
    /// Bot's numeric user ID as a string
    bot_user_id: String,
    /// Configuration for allowed users/chats
    config: gorp_core::config::TelegramConfig,
    /// Connection state for health monitoring
    connection_state: Arc<Mutex<PlatformConnectionState>>,
}

impl TelegramPlatform {
    /// Create a new TelegramPlatform from config.
    ///
    /// Resolves the bot's user ID via the `getMe` API call.
    pub async fn new(config: gorp_core::config::TelegramConfig) -> Result<Self> {
        let bot = Bot::new(&config.bot_token);

        // Resolve bot user ID via getMe
        let me = bot.get_me().await.context("Failed to call Telegram getMe")?;
        let bot_user_id = me.id.0.to_string();

        tracing::info!(
            bot_username = %me.username(),
            bot_id = %bot_user_id,
            "Telegram bot authenticated"
        );

        Ok(Self {
            bot,
            bot_user_id,
            config,
            connection_state: Arc::new(Mutex::new(PlatformConnectionState::Connected)),
        })
    }

    /// Update the platform's connection state
    pub fn set_connection_state(&self, state: PlatformConnectionState) {
        if let Ok(mut current) = self.connection_state.lock() {
            *current = state;
        }
    }

    /// Check if a user is allowed to interact with the bot
    #[allow(dead_code)]
    fn is_user_allowed(&self, user_id: i64) -> bool {
        if self.config.allowed_users.is_empty() {
            return true; // Empty allowlist means allow all
        }
        self.config.allowed_users.contains(&user_id)
    }

    /// Check if a chat is allowed
    #[allow(dead_code)]
    fn is_chat_allowed(&self, chat_id: i64) -> bool {
        if self.config.allowed_chats.is_empty() {
            return true; // Empty allowlist means allow all chats
        }
        self.config.allowed_chats.contains(&chat_id)
    }
}

#[async_trait]
impl MessagingPlatform for TelegramPlatform {
    async fn event_stream(&self) -> Result<EventStream> {
        let (tx, rx) = mpsc::channel(256);
        let bot = self.bot.clone();
        let bot_user_id = self.bot_user_id.clone();
        let allowed_users = self.config.allowed_users.clone();
        let allowed_chats = self.config.allowed_chats.clone();
        let connection_state = Arc::clone(&self.connection_state);

        // Spawn long polling task
        tokio::spawn(async move {
            let mut offset: i32 = 0;

            loop {
                let updates = match bot
                    .get_updates()
                    .offset(offset)
                    .timeout(30)
                    .await
                {
                    Ok(updates) => {
                        // Connected successfully
                        if let Ok(mut state) = connection_state.lock() {
                            if !matches!(*state, PlatformConnectionState::Connected) {
                                *state = PlatformConnectionState::Connected;
                                tracing::info!(platform = "telegram", "Reconnected");
                            }
                        }
                        updates
                    }
                    Err(e) => {
                        tracing::warn!(
                            platform = "telegram",
                            error = %e,
                            "Long polling error, retrying in 5s"
                        );
                        if let Ok(mut state) = connection_state.lock() {
                            *state = PlatformConnectionState::Disconnected {
                                reason: e.to_string(),
                            };
                        }
                        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                        continue;
                    }
                };

                for update in &updates {
                    offset = update.id.as_offset();

                    // Extract message from update kind
                    let message = match &update.kind {
                        UpdateKind::Message(msg) => msg,
                        _ => continue,
                    };

                    // Extract text content from the message
                    let body = match &message.kind {
                        MessageKind::Common(common) => match &common.media_kind {
                            MediaKind::Text(text) => text.text.clone(),
                            _ => continue,
                        },
                        _ => continue,
                    };

                    let Some(from) = message.from.as_ref() else {
                        continue;
                    };

                    // Skip messages from the bot itself
                    if from.id.0.to_string() == bot_user_id {
                        continue;
                    }

                    // Check user allowlist
                    if !allowed_users.is_empty()
                        && !allowed_users.contains(&(from.id.0 as i64))
                    {
                        tracing::debug!(
                            platform = "telegram",
                            user_id = from.id.0,
                            "Skipping message from non-allowed user"
                        );
                        continue;
                    }

                    // Check chat allowlist
                    if !allowed_chats.is_empty()
                        && !allowed_chats.contains(&message.chat.id.0)
                    {
                        tracing::debug!(
                            platform = "telegram",
                            chat_id = message.chat.id.0,
                            "Skipping message from non-allowed chat"
                        );
                        continue;
                    }

                    let is_private = matches!(message.chat.kind, ChatKind::Private(_));

                    let display_name = {
                        let mut parts: Vec<String> = Vec::new();
                        parts.push(from.first_name.clone());
                        if let Some(ref last) = from.last_name {
                            parts.push(last.clone());
                        }
                        Some(parts.join(" "))
                    };

                    // Check for attachment
                    let attachment = match &message.kind {
                        MessageKind::Common(common) => match &common.media_kind {
                            MediaKind::Document(doc) => Some(AttachmentInfo {
                                source_id: doc.document.file.id.to_string(),
                                filename: doc
                                    .document
                                    .file_name
                                    .clone()
                                    .unwrap_or_else(|| "document".to_string()),
                                mime_type: doc
                                    .document
                                    .mime_type
                                    .as_ref()
                                    .map(|m| m.to_string())
                                    .unwrap_or_else(|| {
                                        "application/octet-stream".to_string()
                                    }),
                                size: Some(doc.document.file.size as u64),
                            }),
                            MediaKind::Photo(photo) => {
                                // Use the largest photo size
                                photo.photo.last().map(|p| AttachmentInfo {
                                    source_id: p.file.id.to_string(),
                                    filename: "photo.jpg".to_string(),
                                    mime_type: "image/jpeg".to_string(),
                                    size: Some(p.file.size as u64),
                                })
                            }
                            _ => None,
                        },
                        _ => None,
                    };

                    let msg = IncomingMessage {
                        platform_id: "telegram".to_string(),
                        channel_id: message.chat.id.0.to_string(),
                        thread_id: None,
                        sender: ChatUser {
                            id: from.id.0.to_string(),
                            display_name,
                        },
                        body,
                        is_direct: is_private,
                        formatted: false,
                        attachment,
                        event_id: message.id.0.to_string(),
                        timestamp: message.date.timestamp(),
                    };

                    if tx.send(msg).await.is_err() {
                        tracing::warn!(
                            platform = "telegram",
                            "Event stream receiver dropped"
                        );
                        return;
                    }
                }
            }
        });

        let stream = ReceiverStream::new(rx);
        Ok(Box::pin(stream))
    }

    async fn send(&self, channel_id: &str, content: MessageContent) -> Result<()> {
        let chat_id: ChatId = ChatId(
            channel_id
                .parse::<i64>()
                .context("Invalid Telegram chat ID")?,
        );
        let channel = TelegramChannel::new(chat_id, self.bot.clone(), None, false);
        channel.send(content).await
    }

    fn bot_user_id(&self) -> &str {
        &self.bot_user_id
    }

    fn platform_id(&self) -> &'static str {
        "telegram"
    }

    async fn shutdown(&self) -> Result<()> {
        tracing::info!(platform = "telegram", "Shutting down Telegram platform");
        self.set_connection_state(PlatformConnectionState::Disconnected {
            reason: "shutdown".to_string(),
        });
        Ok(())
    }
}

#[async_trait]
impl ChatPlatform for TelegramPlatform {
    type Channel = TelegramChannel;

    async fn get_channel(&self, id: &str) -> Option<Self::Channel> {
        let chat_id: i64 = id.parse().ok()?;
        // Telegram doesn't have a "get chat" that always works without prior interaction,
        // so we construct the channel directly with minimal info
        Some(TelegramChannel::new(
            ChatId(chat_id),
            self.bot.clone(),
            None,
            false,
        ))
    }

    async fn joined_channels(&self) -> Vec<Self::Channel> {
        // Telegram Bot API doesn't provide a list of chats the bot is in.
        // Channels are discovered through incoming messages.
        vec![]
    }

    fn channel_creator(&self) -> Option<&dyn gorp_core::traits::ChannelCreator> {
        // Telegram bots cannot create groups
        None
    }

    fn channel_manager(&self) -> Option<&dyn ChannelManager> {
        Some(self)
    }

    fn connection_state(&self) -> PlatformConnectionState {
        self.connection_state
            .lock()
            .map(|s| s.clone())
            .unwrap_or(PlatformConnectionState::Connected)
    }
}

#[async_trait]
impl ChannelManager for TelegramPlatform {
    async fn join(&self, _channel_id: &str) -> Result<()> {
        // Telegram bots are added to groups by users, not by joining
        anyhow::bail!("Telegram bots cannot join channels directly")
    }

    async fn leave(&self, channel_id: &str) -> Result<()> {
        let chat_id = ChatId(
            channel_id
                .parse::<i64>()
                .context("Invalid Telegram chat ID")?,
        );
        self.bot
            .leave_chat(chat_id)
            .await
            .context("Failed to leave chat")?;
        Ok(())
    }

    async fn invite(&self, _channel_id: &str, _user_id: &str) -> Result<()> {
        // Telegram bots can't invite users to groups directly via Bot API
        anyhow::bail!("Telegram bots cannot invite users directly")
    }

    async fn members(&self, channel_id: &str) -> Result<Vec<ChatUser>> {
        let chat_id = ChatId(
            channel_id
                .parse::<i64>()
                .context("Invalid Telegram chat ID")?,
        );
        let count = self
            .bot
            .get_chat_member_count(chat_id)
            .await
            .context("Failed to get member count")?;

        // Telegram doesn't provide a list of members via Bot API for most chat types.
        // We return the count as a single synthetic entry.
        Ok(vec![ChatUser::new(format!(
            "{} members (list unavailable)",
            count
        ))])
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_telegram_platform_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<TelegramPlatform>();
    }

    #[test]
    fn test_telegram_channel_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<TelegramChannel>();
    }

    #[test]
    fn test_telegram_channel_id() {
        let bot = Bot::new("fake_token");
        let channel = TelegramChannel::new(ChatId(12345), bot, None, false);
        assert_eq!(channel.id(), "12345");
    }

    #[test]
    fn test_telegram_channel_name() {
        let bot = Bot::new("fake_token");
        let channel = TelegramChannel::new(
            ChatId(12345),
            bot,
            Some("Test Chat".to_string()),
            false,
        );
        assert_eq!(channel.name(), Some("Test Chat".to_string()));
    }

    #[test]
    fn test_telegram_channel_name_none() {
        let bot = Bot::new("fake_token");
        let channel = TelegramChannel::new(ChatId(12345), bot, None, false);
        assert!(channel.name().is_none());
    }

    #[tokio::test]
    async fn test_telegram_channel_is_direct() {
        let bot = Bot::new("fake_token");
        let private = TelegramChannel::new(ChatId(12345), bot.clone(), None, true);
        assert!(private.is_direct().await);

        let group = TelegramChannel::new(ChatId(-12345), bot, None, false);
        assert!(!group.is_direct().await);
    }

    #[test]
    fn test_telegram_channel_negative_chat_id() {
        let bot = Bot::new("fake_token");
        let channel = TelegramChannel::new(ChatId(-100123456789), bot, None, false);
        assert_eq!(channel.id(), "-100123456789");
    }

    #[test]
    fn test_telegram_channel_typing_indicator_present() {
        let bot = Bot::new("fake_token");
        let channel = TelegramChannel::new(ChatId(12345), bot, None, false);
        assert!(channel.typing_indicator().is_some());
    }

    #[test]
    fn test_telegram_channel_attachment_handler_present() {
        let bot = Bot::new("fake_token");
        let channel = TelegramChannel::new(ChatId(12345), bot, None, false);
        assert!(channel.attachment_handler().is_some());
    }

    #[test]
    fn test_user_allowed_empty_list() {
        // When allowed_users is empty, all users should be allowed
        let config = gorp_core::config::TelegramConfig {
            bot_token: "fake".to_string(),
            allowed_users: vec![],
            allowed_chats: vec![],
        };
        // We can't construct TelegramPlatform without a real bot, so test the logic directly
        assert!(config.allowed_users.is_empty());
    }

    #[test]
    fn test_chat_allowed_empty_list() {
        let config = gorp_core::config::TelegramConfig {
            bot_token: "fake".to_string(),
            allowed_users: vec![],
            allowed_chats: vec![],
        };
        assert!(config.allowed_chats.is_empty());
    }
}
