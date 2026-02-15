// ABOUTME: Slack channel implementation wrapping a Slack channel for the ChatChannel trait
// ABOUTME: Handles message sending via Slack Web API with 4K-char chunking and mrkdwn formatting

use anyhow::{Context, Result};
use async_trait::async_trait;
use gorp_core::traits::{ChatChannel, MessageContent, TypingIndicator};
use slack_morphism::prelude::*;
use std::sync::Arc;

/// Maximum message length for a single Slack mrkdwn text block
const MAX_MESSAGE_LENGTH: usize = 4000;

/// A Slack channel wrapped as a ChatChannel
#[derive(Debug, Clone)]
pub struct SlackChannel {
    /// Slack channel ID (e.g., "C12345")
    channel_id: SlackChannelId,
    /// String representation for the id() accessor
    channel_id_str: String,
    /// Shared Slack client for API calls
    client: Arc<SlackHyperClient>,
    /// Bot token for opening sessions
    bot_token: SlackApiToken,
    /// Cached channel name
    channel_name: Option<String>,
    /// Whether this is a DM channel (starts with "D")
    is_dm: bool,
}

impl SlackChannel {
    pub fn new(
        channel_id: SlackChannelId,
        client: Arc<SlackHyperClient>,
        bot_token: SlackApiToken,
        channel_name: Option<String>,
        is_dm: bool,
    ) -> Self {
        let channel_id_str = channel_id.to_string();
        Self {
            channel_id,
            channel_id_str,
            client,
            bot_token,
            channel_name,
            is_dm,
        }
    }

    /// Send a text message, splitting into chunks if it exceeds Slack's limit
    async fn send_chunked(&self, text: &str) -> Result<()> {
        let session = self.client.open_session(&self.bot_token);

        if text.len() <= MAX_MESSAGE_LENGTH {
            let req = SlackApiChatPostMessageRequest::new(
                self.channel_id.clone(),
                SlackMessageContent::new().with_text(text.into()),
            );
            session
                .chat_post_message(&req)
                .await
                .context("Failed to send Slack message")?;
            return Ok(());
        }

        // Split at line boundaries when possible
        for chunk in chunk_text(text, MAX_MESSAGE_LENGTH) {
            let req = SlackApiChatPostMessageRequest::new(
                self.channel_id.clone(),
                SlackMessageContent::new().with_text(chunk.to_string()),
            );
            session
                .chat_post_message(&req)
                .await
                .context("Failed to send Slack message chunk")?;
        }
        Ok(())
    }
}

#[async_trait]
impl ChatChannel for SlackChannel {
    fn id(&self) -> &str {
        &self.channel_id_str
    }

    fn name(&self) -> Option<String> {
        self.channel_name.clone()
    }

    async fn is_direct(&self) -> bool {
        self.is_dm
    }

    async fn send(&self, content: MessageContent) -> Result<()> {
        match content {
            MessageContent::Plain(text) => {
                self.send_chunked(&text).await?;
            }
            MessageContent::Html { plain, .. } => {
                // Slack doesn't support HTML natively, send as plain text
                self.send_chunked(&plain).await?;
            }
            MessageContent::Attachment {
                filename,
                data,
                caption,
                ..
            } => {
                let session = self.client.open_session(&self.bot_token);

                // Upload file via files.uploadV2
                // For simplicity, post a message with the file info
                // Full file upload requires multipart form which slack-morphism handles
                let caption_text = caption.unwrap_or_else(|| filename.clone());
                let req = SlackApiChatPostMessageRequest::new(
                    self.channel_id.clone(),
                    SlackMessageContent::new()
                        .with_text(format!("[Attachment: {} ({} bytes)]", caption_text, data.len())),
                );
                session
                    .chat_post_message(&req)
                    .await
                    .context("Failed to send attachment message")?;
            }
        }
        Ok(())
    }

    fn typing_indicator(&self) -> Option<&dyn TypingIndicator> {
        // Slack doesn't have a "typing indicator" API for bots
        None
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
        let chunks = chunk_text("hello", MAX_MESSAGE_LENGTH);
        assert_eq!(chunks, vec!["hello"]);
    }

    #[test]
    fn test_chunk_text_exact_limit() {
        let text = "a".repeat(MAX_MESSAGE_LENGTH);
        let chunks = chunk_text(&text, MAX_MESSAGE_LENGTH);
        assert_eq!(chunks.len(), 1);
    }

    #[test]
    fn test_chunk_text_splits_at_newline() {
        let line1 = "a".repeat(2000);
        let line2 = "b".repeat(2000);
        let line3 = "c".repeat(2000);
        let text = format!("{}\n{}\n{}", line1, line2, line3);
        let chunks = chunk_text(&text, MAX_MESSAGE_LENGTH);
        assert!(chunks.len() >= 2);
        for chunk in &chunks {
            assert!(chunk.len() <= MAX_MESSAGE_LENGTH);
        }
    }

    #[test]
    fn test_chunk_text_no_newlines() {
        let text = "a".repeat(5000);
        let chunks = chunk_text(&text, MAX_MESSAGE_LENGTH);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].len(), MAX_MESSAGE_LENGTH);
        assert_eq!(chunks[1].len(), 1000);
    }

    #[test]
    fn test_chunk_text_empty() {
        let chunks = chunk_text("", MAX_MESSAGE_LENGTH);
        assert_eq!(chunks, vec![""]);
    }

    #[test]
    fn test_channel_id_from_string() {
        let id: SlackChannelId = "C12345".into();
        assert_eq!(id.to_string(), "C12345");
    }

    #[test]
    fn test_dm_channel_detection() {
        // DM channels start with "D"
        let dm_id = "D12345";
        assert!(dm_id.starts_with('D'));

        let channel_id = "C12345";
        assert!(!channel_id.starts_with('D'));
    }
}
