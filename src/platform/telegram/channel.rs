// ABOUTME: Telegram channel implementation wrapping a chat for the ChatChannel trait
// ABOUTME: Handles message sending with 4096-char chunking and typing indicators

use anyhow::{Context, Result};
use async_trait::async_trait;
use gorp_core::traits::{AttachmentHandler, ChatChannel, MessageContent, TypingIndicator};
use teloxide::net::Download;
use teloxide::prelude::*;
use teloxide::types::{ChatAction, FileId, InputFile, ParseMode};

/// Maximum message length for Telegram Bot API
const MAX_MESSAGE_LENGTH: usize = 4096;

/// A Telegram chat wrapped as a ChatChannel
#[derive(Debug, Clone)]
pub struct TelegramChannel {
    chat_id: ChatId,
    /// String representation of chat_id for the id() accessor
    chat_id_str: String,
    bot: Bot,
    /// Cached chat title or user display name
    chat_name: Option<String>,
    /// Whether this is a private (DM) chat
    is_private: bool,
}

impl TelegramChannel {
    pub fn new(chat_id: ChatId, bot: Bot, chat_name: Option<String>, is_private: bool) -> Self {
        Self {
            chat_id_str: chat_id.0.to_string(),
            chat_id,
            bot,
            chat_name,
            is_private,
        }
    }
}

#[async_trait]
impl ChatChannel for TelegramChannel {
    fn id(&self) -> &str {
        &self.chat_id_str
    }

    fn name(&self) -> Option<String> {
        self.chat_name.clone()
    }

    async fn is_direct(&self) -> bool {
        self.is_private
    }

    async fn send(&self, content: MessageContent) -> Result<()> {
        match content {
            MessageContent::Plain(text) => {
                self.send_chunked(&text, None).await?;
            }
            MessageContent::Html { html, .. } => {
                self.send_chunked(&html, Some(ParseMode::Html)).await?;
            }
            MessageContent::Attachment {
                filename,
                data,
                mime_type,
                caption,
            } => {
                let input_file = InputFile::memory(data).file_name(filename);
                if mime_type.starts_with("image/") {
                    let mut req = self.bot.send_photo(self.chat_id, input_file);
                    if let Some(cap) = caption {
                        req = req.caption(cap);
                    }
                    req.await.context("Failed to send photo")?;
                } else {
                    let mut req = self.bot.send_document(self.chat_id, input_file);
                    if let Some(cap) = caption {
                        req = req.caption(cap);
                    }
                    req.await.context("Failed to send document")?;
                }
            }
        }
        Ok(())
    }

    fn typing_indicator(&self) -> Option<&dyn TypingIndicator> {
        Some(self)
    }

    fn attachment_handler(&self) -> Option<&dyn AttachmentHandler> {
        Some(self)
    }

    async fn member_count(&self) -> Result<usize> {
        let count = self
            .bot
            .get_chat_member_count(self.chat_id)
            .await
            .context("Failed to get chat member count")?;
        Ok(count as usize)
    }
}

impl TelegramChannel {
    /// Send a text message, splitting into chunks if it exceeds Telegram's limit
    async fn send_chunked(&self, text: &str, parse_mode: Option<ParseMode>) -> Result<()> {
        if text.len() <= MAX_MESSAGE_LENGTH {
            let mut req = self.bot.send_message(self.chat_id, text);
            if let Some(pm) = parse_mode {
                req = req.parse_mode(pm);
            }
            req.await.context("Failed to send message")?;
            return Ok(());
        }

        // Split at line boundaries when possible
        for chunk in chunk_text(text, MAX_MESSAGE_LENGTH) {
            let mut req = self.bot.send_message(self.chat_id, chunk);
            if let Some(pm) = parse_mode {
                req = req.parse_mode(pm);
            }
            req.await.context("Failed to send message chunk")?;
        }
        Ok(())
    }
}

#[async_trait]
impl TypingIndicator for TelegramChannel {
    async fn set_typing(&self, typing: bool) -> Result<()> {
        if typing {
            self.bot
                .send_chat_action(self.chat_id, ChatAction::Typing)
                .await
                .context("Failed to send typing action")?;
        }
        // Telegram typing indicators auto-expire; no explicit "stop typing" API
        Ok(())
    }
}

#[async_trait]
impl AttachmentHandler for TelegramChannel {
    async fn download(&self, source_id: &str) -> Result<(String, Vec<u8>, String)> {
        let file = self
            .bot
            .get_file(FileId(source_id.to_string()))
            .await
            .context("Failed to get file info from Telegram")?;

        let mut data = Vec::new();
        self.bot
            .download_file(&file.path, &mut data)
            .await
            .context("Failed to download file from Telegram")?;

        // Telegram doesn't always provide filename or mime_type in the file object,
        // so we use sensible defaults
        let filename = file
            .path
            .split('/')
            .last()
            .unwrap_or("attachment")
            .to_string();
        let mime_type = mime_guess::from_path(&filename)
            .first_or_octet_stream()
            .to_string();

        Ok((filename, data, mime_type))
    }
}

/// Split text into chunks at line boundaries, falling back to character boundaries
fn chunk_text(text: &str, max_len: usize) -> Vec<&str> {
    if text.len() <= max_len {
        return vec![text];
    }

    let mut chunks = Vec::new();
    let mut remaining = text;

    while !remaining.is_empty() {
        if remaining.len() <= max_len {
            chunks.push(remaining);
            break;
        }

        // Try to split at a newline within the limit
        let split_at = remaining[..max_len]
            .rfind('\n')
            .map(|pos| pos + 1)
            .unwrap_or(max_len);

        chunks.push(&remaining[..split_at]);
        remaining = &remaining[split_at..];
    }

    chunks
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chunk_text_short() {
        let chunks = chunk_text("hello", 4096);
        assert_eq!(chunks, vec!["hello"]);
    }

    #[test]
    fn test_chunk_text_exact_limit() {
        let text = "a".repeat(4096);
        let chunks = chunk_text(&text, 4096);
        assert_eq!(chunks.len(), 1);
    }

    #[test]
    fn test_chunk_text_splits_at_newline() {
        let line1 = "a".repeat(2000);
        let line2 = "b".repeat(2000);
        let line3 = "c".repeat(2000);
        let text = format!("{}\n{}\n{}", line1, line2, line3);
        let chunks = chunk_text(&text, 4096);
        assert!(chunks.len() >= 2);
        // Each chunk should be within limits
        for chunk in &chunks {
            assert!(chunk.len() <= 4096);
        }
    }

    #[test]
    fn test_chunk_text_no_newlines() {
        let text = "a".repeat(5000);
        let chunks = chunk_text(&text, 4096);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].len(), 4096);
        assert_eq!(chunks[1].len(), 904);
    }

    #[test]
    fn test_chunk_text_empty() {
        let chunks = chunk_text("", 4096);
        assert_eq!(chunks, vec![""]);
    }

    #[test]
    fn test_telegram_channel_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<TelegramChannel>();
    }
}
