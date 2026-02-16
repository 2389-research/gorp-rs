// ABOUTME: Web gateway adapter — bridges admin WebSocket connections and the message bus.
// ABOUTME: Always started regardless of platform config. Converts chat messages to BusMessages.

use std::sync::Arc;

use async_trait::async_trait;

use crate::admin::websocket::{ChatChunkData, ChatCompleteData, ChatErrorData, ServerMessage, WsHub};
use crate::bus::{MessageBus, ResponseContent};
use crate::gateway::GatewayAdapter;

/// Gateway adapter for the admin web chat interface.
///
/// Subscribes to bus responses and broadcasts them as WebSocket ServerMessages
/// via WsHub. Inbound messages are published to the bus from the WebSocket
/// handler in `admin::websocket` (ChatSend), not from this adapter.
pub struct WebAdapter {
    ws_hub: WsHub,
}

impl WebAdapter {
    pub fn new(ws_hub: WsHub) -> Self {
        Self { ws_hub }
    }
}

#[async_trait]
impl GatewayAdapter for WebAdapter {
    fn platform_id(&self) -> &str {
        "web"
    }

    async fn start(&self, bus: Arc<MessageBus>) -> anyhow::Result<()> {
        // Spawn outbound loop: subscribe to bus responses, convert to
        // ServerMessage variants, broadcast via WsHub
        let hub = self.ws_hub.clone();
        tokio::spawn(async move {
            let mut rx = bus.subscribe_responses();
            loop {
                match rx.recv().await {
                    Ok(resp) => {
                        let messages = response_to_server_messages(
                            &resp.session_name,
                            resp.content,
                        );
                        for msg in messages {
                            hub.broadcast(msg);
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!(skipped = n, "Web adapter outbound lagged");
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        tracing::info!("Bus closed, web adapter shutting down");
                        break;
                    }
                }
            }
        });
        Ok(())
    }

    async fn send(&self, channel_id: &str, content: ResponseContent) -> anyhow::Result<()> {
        // Direct send to a specific "channel" (workspace name for web)
        let messages = response_to_server_messages(channel_id, content);
        for msg in messages {
            self.ws_hub.broadcast(msg);
        }
        Ok(())
    }

    async fn stop(&self) -> anyhow::Result<()> {
        // Web adapter doesn't need special shutdown — the spawned task will
        // exit when the bus sender is dropped and the channel closes.
        Ok(())
    }
}

/// Convert a ResponseContent into one or more ServerMessages.
///
/// Most variants produce a single message. SystemNotice produces two:
/// a ChatChunk with the notice text followed by a ChatComplete to signal
/// the frontend that the response is finished.
fn response_to_server_messages(workspace: &str, content: ResponseContent) -> Vec<ServerMessage> {
    match content {
        ResponseContent::Chunk(text) => vec![ServerMessage::ChatChunk {
            data: ChatChunkData {
                workspace: workspace.to_string(),
                text,
            },
        }],
        ResponseContent::Complete(_) => vec![ServerMessage::ChatComplete {
            data: ChatCompleteData {
                workspace: workspace.to_string(),
            },
        }],
        ResponseContent::Error(error) => vec![ServerMessage::ChatError {
            data: ChatErrorData {
                workspace: workspace.to_string(),
                error,
            },
        }],
        ResponseContent::SystemNotice(text) => vec![
            ServerMessage::ChatChunk {
                data: ChatChunkData {
                    workspace: workspace.to_string(),
                    text,
                },
            },
            ServerMessage::ChatComplete {
                data: ChatCompleteData {
                    workspace: workspace.to_string(),
                },
            },
        ],
    }
}
