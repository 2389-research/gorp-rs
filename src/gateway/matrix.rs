// ABOUTME: Matrix gateway adapter — bridges Matrix SDK events and the message bus.
// ABOUTME: Translates between Matrix room events and BusMessage/BusResponse types.

use std::sync::Arc;

use async_trait::async_trait;
use matrix_sdk::ruma::events::room::message::RoomMessageEventContent;
use matrix_sdk::ruma::OwnedRoomId;
use matrix_sdk::Client;
use tokio::sync::Mutex;

use crate::bus::{BusMessage, MessageBus, MessageSource, ResponseContent, SessionTarget};
use crate::gateway::GatewayAdapter;
use gorp_core::config::MatrixConfig;

/// Matrix gateway adapter that bridges Matrix rooms and the message bus.
///
/// Handles two directions:
/// - Inbound: Matrix room messages -> BusMessage published to bus
/// - Outbound: BusResponse -> Matrix room messages
pub struct MatrixAdapter {
    client: Client,
    config: MatrixConfig,
    bus: Mutex<Option<Arc<MessageBus>>>,
}

impl MatrixAdapter {
    pub fn new(client: Client, config: MatrixConfig) -> Self {
        Self {
            client,
            config,
            bus: Mutex::new(None),
        }
    }

    /// Get a reference to the Matrix SDK client.
    pub fn client(&self) -> &Client {
        &self.client
    }

    /// Get a reference to the Matrix configuration.
    pub fn config(&self) -> &MatrixConfig {
        &self.config
    }
}

#[async_trait]
impl GatewayAdapter for MatrixAdapter {
    fn platform_id(&self) -> &str {
        "matrix"
    }

    async fn start(&self, bus: Arc<MessageBus>) -> anyhow::Result<()> {
        // Store bus reference for later use
        *self.bus.lock().await = Some(bus.clone());

        // Spawn outbound loop: subscribe to bus responses, filter for sessions
        // bound to matrix channels, and send responses to the appropriate rooms
        let client = self.client.clone();
        let outbound_bus = bus.clone();
        tokio::spawn(async move {
            let mut rx = outbound_bus.subscribe_responses();
            loop {
                match rx.recv().await {
                    Ok(resp) => {
                        // Find all matrix channels bound to this session
                        let bindings = outbound_bus
                            .bindings_for_session_async(&resp.session_name)
                            .await;
                        for (platform_id, channel_id) in bindings {
                            if platform_id == "matrix" {
                                if let Err(e) =
                                    send_to_room(&client, &channel_id, &resp.content).await
                                {
                                    tracing::error!(
                                        room = %channel_id,
                                        error = %e,
                                        "Failed to send response to Matrix room"
                                    );
                                }
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!(skipped = n, "Matrix adapter outbound lagged");
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        tracing::info!("Bus closed, matrix adapter shutting down");
                        break;
                    }
                }
            }
        });

        tracing::info!("Matrix adapter started");
        Ok(())
    }

    async fn send(&self, channel_id: &str, content: ResponseContent) -> anyhow::Result<()> {
        send_to_room(&self.client, channel_id, &content).await
    }

    async fn stop(&self) -> anyhow::Result<()> {
        // Clear bus reference
        *self.bus.lock().await = None;
        tracing::info!("Matrix adapter stopped");
        Ok(())
    }
}

/// Send a ResponseContent to a Matrix room.
async fn send_to_room(
    client: &Client,
    room_id: &str,
    content: &ResponseContent,
) -> anyhow::Result<()> {
    let room_id: OwnedRoomId = room_id
        .parse()
        .map_err(|e| anyhow::anyhow!("Invalid room ID '{}': {}", room_id, e))?;

    let room = client
        .get_room(&room_id)
        .ok_or_else(|| anyhow::anyhow!("Room not found: {}", room_id))?;

    let message_content = response_to_matrix_message(content);
    room.send(message_content).await?;
    Ok(())
}

/// Convert a ResponseContent to a Matrix RoomMessageEventContent.
///
/// Plain text content is also rendered as HTML via markdown conversion
/// for rich display in Matrix clients that support it.
pub fn response_to_matrix_message(content: &ResponseContent) -> RoomMessageEventContent {
    match content {
        ResponseContent::Chunk(text) | ResponseContent::Complete(text) => {
            // Use both plain text and HTML for rich rendering
            let html = crate::utils::markdown_to_html(text);
            RoomMessageEventContent::text_html(text, html)
        }
        ResponseContent::Error(error) => {
            let plain = format!("Error: {}", error);
            let html = format!("<strong>Error:</strong> {}", error);
            RoomMessageEventContent::text_html(plain, html)
        }
        ResponseContent::SystemNotice(text) => {
            let html = format!("<em>{}</em>", text);
            RoomMessageEventContent::text_html(text, html)
        }
    }
}

/// Convert a Matrix room message event into a BusMessage.
///
/// Used by event handlers to normalize Matrix events for the bus.
pub fn matrix_event_to_bus_message(
    room_id: &str,
    event_id: &str,
    sender: &str,
    body: &str,
    session_target: SessionTarget,
) -> BusMessage {
    BusMessage {
        id: event_id.to_string(),
        source: MessageSource::Platform {
            platform_id: "matrix".to_string(),
            channel_id: room_id.to_string(),
        },
        session_target,
        sender: sender.to_string(),
        body: body.to_string(),
        timestamp: chrono::Utc::now(),
    }
}
