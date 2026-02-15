// ABOUTME: WebSocket endpoint for real-time updates in the admin panel
// ABOUTME: Subscribe/unsubscribe model for feed, status, and chat channels

use axum::{
    extract::{
        ws::{Message, WebSocket},
        State, WebSocketUpgrade,
    },
    response::IntoResponse,
};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc};

use super::routes::AdminState;

// =============================================================================
// WebSocket Messages
// =============================================================================

/// Messages from the client to the server
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum ClientMessage {
    #[serde(rename = "subscribe")]
    Subscribe { channels: Vec<String> },
    #[serde(rename = "unsubscribe")]
    Unsubscribe { channels: Vec<String> },
    #[serde(rename = "chat.send")]
    ChatSend { workspace: String, body: String },
    #[serde(rename = "chat.cancel")]
    ChatCancel { workspace: String },
    #[serde(rename = "chat.select_workspace")]
    ChatSelectWorkspace { workspace: String },
}

/// Messages from the server to the client
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum ServerMessage {
    #[serde(rename = "feed.message")]
    FeedMessage {
        html: String,
        data: FeedMessageData,
    },
    #[serde(rename = "status.platform")]
    StatusPlatform { data: PlatformStatusData },
    #[serde(rename = "chat.chunk")]
    ChatChunk { data: ChatChunkData },
    #[serde(rename = "chat.tool_use")]
    ChatToolUse { data: ChatToolUseData },
    #[serde(rename = "chat.complete")]
    ChatComplete { data: ChatCompleteData },
    #[serde(rename = "chat.error")]
    ChatError { data: ChatErrorData },
}

#[derive(Debug, Clone, Serialize)]
pub struct FeedMessageData {
    pub platform: String,
    pub channel_id: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlatformStatusData {
    pub platform: String,
    pub state: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChatChunkData {
    pub workspace: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChatToolUseData {
    pub workspace: String,
    pub tool: String,
    pub input: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChatCompleteData {
    pub workspace: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChatErrorData {
    pub workspace: String,
    pub error: String,
}

// =============================================================================
// WebSocket Hub — broadcasts to connected clients
// =============================================================================

/// Hub for broadcasting messages to WebSocket clients
#[derive(Clone)]
pub struct WsHub {
    sender: broadcast::Sender<ServerMessage>,
}

impl WsHub {
    pub fn new() -> Self {
        let (sender, _) = broadcast::channel(256);
        Self { sender }
    }

    /// Broadcast a message to all connected clients
    pub fn broadcast(&self, msg: ServerMessage) {
        // Ignore send errors (no receivers connected)
        let _ = self.sender.send(msg);
    }

    /// Get a receiver for subscribing to broadcasts
    pub fn subscribe(&self) -> broadcast::Receiver<ServerMessage> {
        self.sender.subscribe()
    }
}

impl Default for WsHub {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// WebSocket Handler
// =============================================================================

/// WebSocket upgrade handler at /admin/ws
pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<AdminState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_ws(socket, state))
}

/// Handle a WebSocket connection
async fn handle_ws(socket: WebSocket, state: AdminState) {
    let (mut ws_sink, mut ws_stream) =
        socket.split::<Message>();
    let (_tx, mut rx) = mpsc::channel::<ServerMessage>(64);

    // Track which channels this client is subscribed to
    let subscriptions = Arc::new(tokio::sync::Mutex::new(HashSet::<String>::new()));

    // Get broadcast receiver from the hub
    let hub = state.ws_hub.clone();
    let mut broadcast_rx = hub.subscribe();

    // Writer task: sends messages to the client
    let writer_task = tokio::spawn(async move {
        loop {
            tokio::select! {
                // Direct messages (from command handling)
                Some(msg) = rx.recv() => {
                    let json = match serde_json::to_string(&msg) {
                        Ok(j) => j,
                        Err(e) => {
                            tracing::warn!(error = %e, "Failed to serialize WebSocket message");
                            continue;
                        }
                    };
                    if ws_sink.send(Message::Text(json.into())).await.is_err() {
                        break;
                    }
                }
                // Broadcast messages (from hub)
                Ok(msg) = broadcast_rx.recv() => {
                    let json = match serde_json::to_string(&msg) {
                        Ok(j) => j,
                        Err(e) => {
                            tracing::warn!(error = %e, "Failed to serialize broadcast message");
                            continue;
                        }
                    };
                    if ws_sink.send(Message::Text(json.into())).await.is_err() {
                        break;
                    }
                }
                else => break,
            }
        }
    });

    // Reader task: reads messages from the client
    let subs = Arc::clone(&subscriptions);
    let reader_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = ws_stream.next().await {
            match msg {
                Message::Text(text) => {
                    let parsed: ClientMessage = match serde_json::from_str(&text) {
                        Ok(m) => m,
                        Err(e) => {
                            tracing::debug!(error = %e, "Invalid WebSocket message from client");
                            continue;
                        }
                    };

                    match parsed {
                        ClientMessage::Subscribe { channels } => {
                            let mut locked = subs.lock().await;
                            for ch in channels {
                                locked.insert(ch);
                            }
                            tracing::debug!(subscriptions = ?*locked, "Client updated subscriptions");
                        }
                        ClientMessage::Unsubscribe { channels } => {
                            let mut locked = subs.lock().await;
                            for ch in &channels {
                                locked.remove(ch);
                            }
                            tracing::debug!(subscriptions = ?*locked, "Client removed subscriptions");
                        }
                        ClientMessage::ChatSend { workspace, body } => {
                            tracing::info!(
                                workspace = %workspace,
                                body_len = body.len(),
                                "Chat message received via WebSocket"
                            );
                            // Chat integration will be wired in the web-chat-page task
                        }
                        ClientMessage::ChatCancel { workspace } => {
                            tracing::info!(
                                workspace = %workspace,
                                "Chat cancel received via WebSocket"
                            );
                        }
                        ClientMessage::ChatSelectWorkspace { workspace } => {
                            tracing::info!(
                                workspace = %workspace,
                                "Workspace selection received via WebSocket"
                            );
                        }
                    }
                }
                Message::Close(_) => break,
                _ => {}
            }
        }
    });

    // Wait for either task to finish
    tokio::select! {
        _ = writer_task => {},
        _ = reader_task => {},
    }

    tracing::debug!("WebSocket connection closed");
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_message_subscribe_deserialize() {
        let json = r#"{"type": "subscribe", "channels": ["feed", "status"]}"#;
        let msg: ClientMessage = serde_json::from_str(json).unwrap();
        match msg {
            ClientMessage::Subscribe { channels } => {
                assert_eq!(channels, vec!["feed", "status"]);
            }
            _ => panic!("Expected Subscribe"),
        }
    }

    #[test]
    fn test_client_message_unsubscribe_deserialize() {
        let json = r#"{"type": "unsubscribe", "channels": ["feed"]}"#;
        let msg: ClientMessage = serde_json::from_str(json).unwrap();
        match msg {
            ClientMessage::Unsubscribe { channels } => {
                assert_eq!(channels, vec!["feed"]);
            }
            _ => panic!("Expected Unsubscribe"),
        }
    }

    #[test]
    fn test_client_message_chat_send_deserialize() {
        let json = r#"{"type": "chat.send", "workspace": "research", "body": "Hello"}"#;
        let msg: ClientMessage = serde_json::from_str(json).unwrap();
        match msg {
            ClientMessage::ChatSend { workspace, body } => {
                assert_eq!(workspace, "research");
                assert_eq!(body, "Hello");
            }
            _ => panic!("Expected ChatSend"),
        }
    }

    #[test]
    fn test_client_message_chat_cancel_deserialize() {
        let json = r#"{"type": "chat.cancel", "workspace": "research"}"#;
        let msg: ClientMessage = serde_json::from_str(json).unwrap();
        match msg {
            ClientMessage::ChatCancel { workspace } => {
                assert_eq!(workspace, "research");
            }
            _ => panic!("Expected ChatCancel"),
        }
    }

    #[test]
    fn test_client_message_select_workspace_deserialize() {
        let json = r#"{"type": "chat.select_workspace", "workspace": "research"}"#;
        let msg: ClientMessage = serde_json::from_str(json).unwrap();
        match msg {
            ClientMessage::ChatSelectWorkspace { workspace } => {
                assert_eq!(workspace, "research");
            }
            _ => panic!("Expected ChatSelectWorkspace"),
        }
    }

    #[test]
    fn test_server_message_feed_serialize() {
        let msg = ServerMessage::FeedMessage {
            html: "<div>test</div>".to_string(),
            data: FeedMessageData {
                platform: "matrix".to_string(),
                channel_id: "!abc:matrix.org".to_string(),
            },
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"type\":\"feed.message\""));
        assert!(json.contains("\"platform\":\"matrix\""));
    }

    #[test]
    fn test_server_message_status_serialize() {
        let msg = ServerMessage::StatusPlatform {
            data: PlatformStatusData {
                platform: "telegram".to_string(),
                state: "connected".to_string(),
            },
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"type\":\"status.platform\""));
        assert!(json.contains("\"state\":\"connected\""));
    }

    #[test]
    fn test_server_message_chat_chunk_serialize() {
        let msg = ServerMessage::ChatChunk {
            data: ChatChunkData {
                workspace: "research".to_string(),
                text: "Hello world".to_string(),
            },
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"type\":\"chat.chunk\""));
        assert!(json.contains("\"text\":\"Hello world\""));
    }

    #[test]
    fn test_server_message_chat_error_serialize() {
        let msg = ServerMessage::ChatError {
            data: ChatErrorData {
                workspace: "research".to_string(),
                error: "Backend timeout".to_string(),
            },
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"type\":\"chat.error\""));
        assert!(json.contains("\"error\":\"Backend timeout\""));
    }

    #[test]
    fn test_ws_hub_broadcast() {
        let hub = WsHub::new();
        let mut rx = hub.subscribe();

        hub.broadcast(ServerMessage::StatusPlatform {
            data: PlatformStatusData {
                platform: "test".to_string(),
                state: "connected".to_string(),
            },
        });

        let msg = rx.try_recv().unwrap();
        match msg {
            ServerMessage::StatusPlatform { data } => {
                assert_eq!(data.platform, "test");
                assert_eq!(data.state, "connected");
            }
            _ => panic!("Expected StatusPlatform"),
        }
    }

    #[test]
    fn test_ws_hub_no_receivers_doesnt_panic() {
        let hub = WsHub::new();
        // Should not panic even with no receivers
        hub.broadcast(ServerMessage::StatusPlatform {
            data: PlatformStatusData {
                platform: "test".to_string(),
                state: "connected".to_string(),
            },
        });
    }

    #[test]
    fn test_client_message_invalid_json() {
        let result = serde_json::from_str::<ClientMessage>("not json");
        assert!(result.is_err());
    }

    #[test]
    fn test_client_message_unknown_type() {
        let json = r#"{"type": "unknown", "data": {}}"#;
        let result = serde_json::from_str::<ClientMessage>(json);
        assert!(result.is_err());
    }
}
