// ABOUTME: Matrix platform implementation for gorp chat abstraction
// ABOUTME: Implements Tier 2 ChatPlatform with full channel management and encryption

pub mod client;

// Re-export client functions for convenience
pub use client::{create_client, create_dm_room, create_room, invite_user, login};

use anyhow::{Context, Result};
use async_trait::async_trait;
use gorp_core::traits::{
    AttachmentHandler, AttachmentInfo, ChannelCreator, ChannelManager, ChatChannel, ChatPlatform,
    ChatUser, EventStream, IncomingMessage, MessageContent, MessagingPlatform, TypingIndicator,
};
use matrix_sdk::{
    media::{MediaFormat, MediaRequestParameters},
    room::Room,
    ruma::{
        events::room::{
            message::{FileMessageEventContent, MessageType, RoomMessageEventContent},
            MediaSource,
        },
        OwnedRoomId, OwnedUserId,
    },
    Client,
};
use std::fmt;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

// =============================================================================
// MatrixChannel - Implements ChatChannel (Tier 2)
// =============================================================================

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

// =============================================================================
// MatrixPlatform - Implements MessagingPlatform + ChatPlatform (Tier 2)
// =============================================================================

/// Matrix-specific implementation of ChatPlatform
pub struct MatrixPlatform {
    client: Client,
    /// Cached user ID - stored at construction to avoid Option handling
    user_id: String,
}

impl MatrixPlatform {
    /// Create a new MatrixPlatform.
    ///
    /// # Panics
    /// Panics if the client is not logged in (user_id is None).
    /// Always create this after successful login.
    pub fn new(client: Client) -> Self {
        let user_id = client
            .user_id()
            .expect("MatrixPlatform requires a logged-in client")
            .to_string();
        Self { client, user_id }
    }

    /// Get the underlying Matrix client
    pub fn client(&self) -> &Client {
        &self.client
    }

    /// Register the event stream handler and return a receiver for incoming messages.
    /// This sets up Matrix SDK event handlers that convert to IncomingMessage.
    pub fn setup_event_stream(&self) -> mpsc::Receiver<IncomingMessage> {
        let (tx, rx) = mpsc::channel(256);
        let client = self.client.clone();
        let bot_user_id = self.user_id.clone();

        // Register message event handler
        client.add_event_handler(
            move |event: matrix_sdk::ruma::events::room::message::SyncRoomMessageEvent,
                  room: Room,
                  _client: Client| {
                let tx = tx.clone();
                let bot_user_id = bot_user_id.clone();
                async move {
                    // Only process original events (not edits/redactions)
                    let Some(original) = event.as_original() else {
                        return;
                    };

                    // Skip messages from ourselves
                    if original.sender.as_str() == bot_user_id {
                        return;
                    }

                    // Convert to IncomingMessage
                    let body = match &original.content.msgtype {
                        MessageType::Text(text) => text.body.clone(),
                        MessageType::Notice(notice) => notice.body.clone(),
                        MessageType::Emote(emote) => emote.body.clone(),
                        _ => return, // Skip non-text messages for now
                    };

                    let is_formatted = matches!(
                        &original.content.msgtype,
                        MessageType::Text(t) if t.formatted.is_some()
                    );

                    // Check for attachment
                    let attachment = match &original.content.msgtype {
                        MessageType::File(f) => Some(AttachmentInfo {
                            source_id: serde_json::to_string(&f.source).unwrap_or_default(),
                            filename: f.filename.clone().unwrap_or_else(|| f.body.clone()),
                            mime_type: f
                                .info
                                .as_ref()
                                .and_then(|i| i.mimetype.clone())
                                .unwrap_or_else(|| "application/octet-stream".to_string()),
                            size: f.info.as_ref().and_then(|i| i.size.map(|s| s.into())),
                        }),
                        MessageType::Image(i) => Some(AttachmentInfo {
                            source_id: serde_json::to_string(&i.source).unwrap_or_default(),
                            filename: i.filename.clone().unwrap_or_else(|| i.body.clone()),
                            mime_type: i
                                .info
                                .as_ref()
                                .and_then(|info| info.mimetype.clone())
                                .unwrap_or_else(|| "image/png".to_string()),
                            size: i.info.as_ref().and_then(|info| info.size.map(|s| s.into())),
                        }),
                        _ => None,
                    };

                    let is_direct = room.is_direct().await.unwrap_or(false);

                    let msg = IncomingMessage {
                        channel_id: room.room_id().to_string(),
                        sender: ChatUser {
                            id: original.sender.to_string(),
                            display_name: room
                                .get_member(&original.sender)
                                .await
                                .ok()
                                .flatten()
                                .and_then(|m| m.display_name().map(|n| n.to_string())),
                        },
                        body,
                        is_direct,
                        formatted: is_formatted,
                        attachment,
                        event_id: original.event_id.to_string(),
                        timestamp: {
                            let millis: u64 = original.origin_server_ts.0.into();
                            (millis / 1000) as i64
                        },
                    };

                    if tx.send(msg).await.is_err() {
                        tracing::warn!("Event stream receiver dropped");
                    }
                }
            },
        );

        rx
    }
}

#[async_trait]
impl MessagingPlatform for MatrixPlatform {
    async fn event_stream(&self) -> Result<EventStream> {
        let rx = self.setup_event_stream();
        let stream = ReceiverStream::new(rx);
        Ok(Box::pin(stream))
    }

    async fn send(&self, channel_id: &str, content: MessageContent) -> Result<()> {
        let channel = self
            .get_channel(channel_id)
            .await
            .context("Channel not found")?;
        channel.send(content).await
    }

    fn bot_user_id(&self) -> &str {
        &self.user_id
    }

    fn platform_id(&self) -> &'static str {
        "matrix"
    }
}

#[async_trait]
impl ChatPlatform for MatrixPlatform {
    type Channel = MatrixChannel;

    async fn get_channel(&self, id: &str) -> Option<Self::Channel> {
        let room_id: OwnedRoomId = id.parse().ok()?;
        let room = self.client.get_room(&room_id)?;
        Some(MatrixChannel::new(room, self.client.clone()))
    }

    async fn joined_channels(&self) -> Vec<Self::Channel> {
        self.client
            .joined_rooms()
            .into_iter()
            .map(|room| MatrixChannel::new(room, self.client.clone()))
            .collect()
    }

    fn channel_creator(&self) -> Option<&dyn ChannelCreator> {
        Some(self)
    }

    fn channel_manager(&self) -> Option<&dyn ChannelManager> {
        Some(self)
    }
}

#[async_trait]
impl ChannelCreator for MatrixPlatform {
    async fn create_channel(&self, name: &str) -> Result<String> {
        let room_id = client::create_room(&self.client, name).await?;
        Ok(room_id.to_string())
    }

    async fn create_dm(&self, user_id: &str) -> Result<String> {
        let user_id: OwnedUserId = user_id.parse().context("Invalid user ID")?;
        let room_id = client::create_dm_room(&self.client, &user_id).await?;
        Ok(room_id.to_string())
    }
}

#[async_trait]
impl ChannelManager for MatrixPlatform {
    async fn join(&self, channel_id: &str) -> Result<()> {
        let room_id: OwnedRoomId = channel_id.parse().context("Invalid room ID")?;
        self.client
            .join_room_by_id(&room_id)
            .await
            .context("Failed to join room")?;
        Ok(())
    }

    async fn leave(&self, channel_id: &str) -> Result<()> {
        let room_id: OwnedRoomId = channel_id.parse().context("Invalid room ID")?;
        if let Some(room) = self.client.get_room(&room_id) {
            room.leave().await.context("Failed to leave room")?;
        }
        Ok(())
    }

    async fn invite(&self, channel_id: &str, user_id: &str) -> Result<()> {
        let room_id: OwnedRoomId = channel_id.parse().context("Invalid room ID")?;
        client::invite_user(&self.client, &room_id, user_id).await
    }

    async fn members(&self, channel_id: &str) -> Result<Vec<ChatUser>> {
        let room_id: OwnedRoomId = channel_id.parse().context("Invalid room ID")?;
        let room = self
            .client
            .get_room(&room_id)
            .context("Room not found")?;

        let members = room
            .members(matrix_sdk::RoomMemberships::ACTIVE)
            .await
            .context("Failed to get room members")?;

        Ok(members
            .into_iter()
            .map(|m| ChatUser {
                id: m.user_id().to_string(),
                display_name: m.display_name().map(|n| n.to_string()),
            })
            .collect())
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_matrix_platform_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<MatrixPlatform>();
    }

    #[test]
    fn test_matrix_channel_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<MatrixChannel>();
    }
}
