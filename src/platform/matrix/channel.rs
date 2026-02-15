// ABOUTME: Matrix channel implementation wrapping matrix_sdk Room
// ABOUTME: Implements ChatChannel, TypingIndicator, and AttachmentHandler traits

use anyhow::{Context, Result};
use async_trait::async_trait;
use gorp_core::traits::{
    AttachmentHandler, ChatChannel, MessageContent, TypingIndicator,
};
use matrix_sdk::{
    media::{MediaFormat, MediaRequestParameters},
    room::Room,
    ruma::events::room::{
        message::{FileMessageEventContent, MessageType, RoomMessageEventContent},
        MediaSource,
    },
    Client,
};
use std::fmt;

/// Matrix-specific implementation of ChatChannel
#[derive(Clone)]
pub struct MatrixChannel {
    room: Room,
    client: Client,
}

impl MatrixChannel {
    pub fn new(room: Room, client: Client) -> Self {
        Self { room, client }
    }

    /// Get the underlying Matrix room
    pub fn inner(&self) -> &Room {
        &self.room
    }

    /// Get the underlying Matrix client
    pub fn client(&self) -> &Client {
        &self.client
    }
}

impl fmt::Debug for MatrixChannel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MatrixChannel")
            .field("room_id", &self.room.room_id().as_str())
            .finish()
    }
}

#[async_trait]
impl ChatChannel for MatrixChannel {
    fn id(&self) -> &str {
        self.room.room_id().as_str()
    }

    fn name(&self) -> Option<String> {
        self.room.name()
    }

    async fn is_direct(&self) -> bool {
        self.room.is_direct().await.unwrap_or(false)
    }

    async fn send(&self, content: MessageContent) -> Result<()> {
        let msg_content = match content {
            MessageContent::Plain(text) => RoomMessageEventContent::text_plain(text),
            MessageContent::Html { plain, html } => RoomMessageEventContent::text_html(plain, html),
            MessageContent::Attachment {
                filename,
                data,
                mime_type,
                caption,
            } => {
                let content_type: mime_guess::mime::Mime = mime_type
                    .parse()
                    .unwrap_or(mime_guess::mime::APPLICATION_OCTET_STREAM);

                // Upload the file to Matrix media server
                let response = self
                    .client
                    .media()
                    .upload(&content_type, data, None)
                    .await
                    .context("Failed to upload attachment")?;

                // Create file message with the MXC URI
                let body = caption.unwrap_or_else(|| filename.clone());
                let source = MediaSource::Plain(response.content_uri);
                let file_content = FileMessageEventContent::new(body, source);

                RoomMessageEventContent::new(MessageType::File(file_content))
            }
        };

        self.room
            .send(msg_content)
            .await
            .context("Failed to send message")?;

        Ok(())
    }

    fn typing_indicator(&self) -> Option<&dyn TypingIndicator> {
        Some(self)
    }

    fn attachment_handler(&self) -> Option<&dyn AttachmentHandler> {
        Some(self)
    }

    async fn member_count(&self) -> Result<usize> {
        let members = self
            .room
            .members(matrix_sdk::RoomMemberships::ACTIVE)
            .await
            .context("Failed to get room members")?;
        Ok(members.len())
    }
}

#[async_trait]
impl TypingIndicator for MatrixChannel {
    async fn set_typing(&self, typing: bool) -> Result<()> {
        self.room
            .typing_notice(typing)
            .await
            .context("Failed to set typing indicator")?;
        Ok(())
    }
}

#[async_trait]
impl AttachmentHandler for MatrixChannel {
    async fn download(&self, source_id: &str) -> Result<(String, Vec<u8>, String)> {
        let source: MediaSource = serde_json::from_str(source_id)
            .context("source_id must be a JSON-serialized MediaSource")?;

        let request = MediaRequestParameters {
            source: source.clone(),
            format: MediaFormat::File,
        };

        let data = self
            .client
            .media()
            .get_media_content(&request, true)
            .await
            .context("Failed to download attachment")?;

        // Extract filename from the source or use a default
        let filename = match &source {
            MediaSource::Plain(uri) => uri.as_str().rsplit('/').next().unwrap_or("attachment"),
            MediaSource::Encrypted(file) => {
                file.url.as_str().rsplit('/').next().unwrap_or("attachment")
            }
        }
        .to_string();

        // Default mime type - caller should detect from content if needed
        let mime_type = "application/octet-stream".to_string();

        Ok((filename, data, mime_type))
    }
}
