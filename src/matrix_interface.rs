// ABOUTME: Matrix SDK implementation of gorp-core chat interface traits
// ABOUTME: Wraps matrix_sdk::Room and Client to implement ChatRoom and ChatInterface

use anyhow::{Context, Result};
use async_trait::async_trait;
use gorp_core::traits::{ChatInterface, ChatRoom, MessageContent};
use matrix_sdk::{
    media::{MediaFormat, MediaRequestParameters},
    room::Room,
    ruma::events::room::message::RoomMessageEventContent,
    Client,
};
use std::fmt;

/// Matrix-specific implementation of ChatRoom
#[derive(Clone)]
pub struct MatrixRoom {
    room: Room,
    client: Client,
}

impl MatrixRoom {
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

impl fmt::Debug for MatrixRoom {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MatrixRoom")
            .field("room_id", &self.room.room_id().as_str())
            .finish()
    }
}

#[async_trait]
impl ChatRoom for MatrixRoom {
    fn id(&self) -> &str {
        self.room.room_id().as_str()
    }

    fn name(&self) -> Option<String> {
        self.room.name()
    }

    async fn is_direct_message(&self) -> bool {
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
                // For attachments, we need to upload to Matrix media server first
                use matrix_sdk::ruma::events::room::message::{
                    FileMessageEventContent, MessageType,
                };

                let content_type: mime_guess::mime::Mime = mime_type
                    .parse()
                    .unwrap_or(mime_guess::mime::APPLICATION_OCTET_STREAM);

                // Upload the file
                let response = self
                    .client
                    .media()
                    .upload(&content_type, data, None)
                    .await
                    .context("Failed to upload attachment")?;

                // Create file message with the MXC URI
                let body = caption.unwrap_or_else(|| filename.clone());
                let source =
                    matrix_sdk::ruma::events::room::MediaSource::Plain(response.content_uri);
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

    async fn set_typing(&self, typing: bool) -> Result<()> {
        self.room
            .typing_notice(typing)
            .await
            .context("Failed to set typing indicator")?;
        Ok(())
    }

    async fn download_attachment(&self, source_id: &str) -> Result<(String, Vec<u8>, String)> {
        // The source_id should be a JSON-serialized MediaSource or just an MXC URI
        // For simplicity, we treat it as a JSON-serialized MediaSource
        use matrix_sdk::ruma::events::room::MediaSource;

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
            MediaSource::Encrypted(file) => file.url.as_str().rsplit('/').next().unwrap_or("attachment"),
        }
        .to_string();

        // Default mime type - caller should detect from content if needed
        let mime_type = "application/octet-stream".to_string();

        Ok((filename, data, mime_type))
    }
}

/// Matrix-specific implementation of ChatInterface
pub struct MatrixInterface {
    client: Client,
    /// Cached user ID - stored at construction to avoid Option handling on every call
    user_id: String,
}

impl MatrixInterface {
    /// Create a new MatrixInterface.
    ///
    /// # Panics
    /// Panics if the client is not logged in (user_id is None).
    /// Always create this after successful login.
    pub fn new(client: Client) -> Self {
        let user_id = client
            .user_id()
            .expect("MatrixInterface requires a logged-in client")
            .to_string();
        Self { client, user_id }
    }

    /// Get the underlying Matrix client
    pub fn client(&self) -> &Client {
        &self.client
    }
}

#[async_trait]
impl ChatInterface for MatrixInterface {
    type Room = MatrixRoom;

    async fn get_room(&self, room_id: &str) -> Option<Self::Room> {
        use matrix_sdk::ruma::OwnedRoomId;

        let room_id: OwnedRoomId = room_id.parse().ok()?;
        let room = self.client.get_room(&room_id)?;
        Some(MatrixRoom::new(room, self.client.clone()))
    }

    fn bot_user_id(&self) -> &str {
        &self.user_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Integration tests would require a real Matrix client
    // Unit tests for the wrapper types

    #[test]
    fn test_matrix_interface_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<MatrixInterface>();
    }
}
