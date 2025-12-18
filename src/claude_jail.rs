// ABOUTME: WebSocket client for Claude Jail service.
// ABOUTME: Replaces CLI subprocess with proper Agent SDK integration via WebSocket.

use anyhow::{Context, Result};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tokio_tungstenite::{connect_async, tungstenite::Message};

use crate::claude::{ClaudeEvent, ClaudeUsage};

/// Request message sent to Claude Jail
#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum JailRequest {
    Query {
        channel_id: String,
        workspace: String,
        prompt: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
    },
    CloseSession {
        channel_id: String,
    },
}

/// Response message received from Claude Jail
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum JailResponse {
    Text {
        channel_id: String,
        content: String,
    },
    ToolUse {
        channel_id: String,
        tool: String,
        input: serde_json::Value,
    },
    Done {
        channel_id: String,
        session_id: String,
    },
    Error {
        channel_id: String,
        message: String,
    },
}

type WebSocketStream = tokio_tungstenite::WebSocketStream<
    tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
>;

/// Shared state for pending channel responses
type PendingChannels = Arc<Mutex<HashMap<String, mpsc::Sender<ClaudeEvent>>>>;

/// Accumulated text per channel
type TextAccumulator = Arc<Mutex<HashMap<String, String>>>;

/// Client for communicating with Claude Jail WebSocket service
pub struct ClaudeJailClient {
    url: String,
    write: Arc<Mutex<futures_util::stream::SplitSink<WebSocketStream, Message>>>,
    pending: PendingChannels,
    text_accumulator: TextAccumulator,
}

impl ClaudeJailClient {
    /// Connect to the Claude Jail WebSocket server
    pub async fn connect(url: &str) -> Result<Self> {
        // Validate URL format
        let _ = url::Url::parse(url).context("Invalid Claude Jail URL")?;

        tracing::info!(%url, "Connecting to Claude Jail");
        let (ws_stream, _) = connect_async(url)
            .await
            .context("Failed to connect to Claude Jail")?;

        let (write, read) = ws_stream.split();
        let write = Arc::new(Mutex::new(write));
        let pending: PendingChannels = Arc::new(Mutex::new(HashMap::new()));
        let text_accumulator: TextAccumulator = Arc::new(Mutex::new(HashMap::new()));

        // Spawn task to read messages and route to appropriate channels
        let pending_clone = pending.clone();
        let text_accumulator_clone = text_accumulator.clone();
        tokio::spawn(async move {
            Self::read_loop(read, pending_clone, text_accumulator_clone).await;
        });

        tracing::info!("Connected to Claude Jail");

        Ok(Self {
            url: url.to_string(),
            write,
            pending,
            text_accumulator,
        })
    }

    /// Read loop that routes messages to appropriate channel handlers
    async fn read_loop(
        mut read: futures_util::stream::SplitStream<WebSocketStream>,
        pending: PendingChannels,
        text_accumulator: TextAccumulator,
    ) {
        while let Some(msg_result) = read.next().await {
            match msg_result {
                Ok(Message::Text(text)) => {
                    if let Err(e) =
                        Self::handle_message(text.as_str(), &pending, &text_accumulator).await
                    {
                        tracing::error!(error = %e, "Failed to handle message");
                    }
                }
                Ok(Message::Close(_)) => {
                    tracing::info!("Claude Jail connection closed");
                    break;
                }
                Ok(_) => {} // Ignore pings, pongs, binary
                Err(e) => {
                    tracing::error!(error = %e, "WebSocket error");
                    break;
                }
            }
        }

        // Connection closed - notify all pending channels
        let mut pending = pending.lock().await;
        for (channel_id, tx) in pending.drain() {
            let _ = tx
                .send(ClaudeEvent::Error("Connection to Claude Jail lost".to_string()))
                .await;
            tracing::warn!(%channel_id, "Notified channel of connection loss");
        }
    }

    /// Handle a single message from Claude Jail
    async fn handle_message(
        text: &str,
        pending: &PendingChannels,
        text_accumulator: &TextAccumulator,
    ) -> Result<()> {
        let response: JailResponse =
            serde_json::from_str(text).context("Failed to parse response")?;

        let (channel_id, event) = match response {
            JailResponse::Text { channel_id, content } => {
                // Accumulate text for this channel
                let mut accumulator = text_accumulator.lock().await;
                accumulator
                    .entry(channel_id.clone())
                    .or_insert_with(String::new)
                    .push_str(&content);
                tracing::debug!(%channel_id, content_len = content.len(), "Text chunk received");
                return Ok(()); // Don't emit individual text chunks, wait for Done
            }
            JailResponse::ToolUse {
                channel_id,
                tool,
                input,
            } => {
                let input_preview = get_input_preview(&input, &tool);
                tracing::info!(%channel_id, %tool, %input_preview, "Tool use");
                (
                    channel_id,
                    ClaudeEvent::ToolUse {
                        name: tool,
                        input_preview,
                    },
                )
            }
            JailResponse::Done {
                channel_id,
                session_id,
            } => {
                // Get accumulated text for this channel
                let accumulated_text = {
                    let mut accumulator = text_accumulator.lock().await;
                    accumulator.remove(&channel_id).unwrap_or_default()
                };
                tracing::info!(%channel_id, %session_id, text_len = accumulated_text.len(), "Query complete");
                (
                    channel_id,
                    ClaudeEvent::Result {
                        text: accumulated_text,
                        usage: ClaudeUsage::default(),
                    },
                )
            }
            JailResponse::Error { channel_id, message } => {
                // Clear any accumulated text on error
                {
                    let mut accumulator = text_accumulator.lock().await;
                    accumulator.remove(&channel_id);
                }
                tracing::error!(%channel_id, %message, "Claude Jail error");
                (channel_id, ClaudeEvent::Error(message))
            }
        };

        // Route to the appropriate channel handler and REMOVE from pending to signal completion
        // Removing the sender causes the receiver to close, exiting the rx.recv() loop
        if let Some(tx) = pending.lock().await.remove(&channel_id) {
            if let Err(e) = tx.send(event).await {
                tracing::warn!(%channel_id, error = %e, "Failed to send event to channel");
            }
            // tx is dropped here, closing the channel
        } else {
            tracing::warn!(%channel_id, "Received message for unknown channel");
        }

        Ok(())
    }

    /// Send a query and return a receiver for events
    pub async fn query(
        &self,
        channel_id: &str,
        workspace: &str,
        prompt: &str,
        session_id: Option<&str>,
    ) -> Result<mpsc::Receiver<ClaudeEvent>> {
        let (tx, rx) = mpsc::channel(32);

        // Register this channel for responses
        {
            let mut pending = self.pending.lock().await;
            pending.insert(channel_id.to_string(), tx);
        }

        // Send the query
        let request = JailRequest::Query {
            channel_id: channel_id.to_string(),
            workspace: workspace.to_string(),
            prompt: prompt.to_string(),
            session_id: session_id.map(|s| s.to_string()),
        };

        let json = serde_json::to_string(&request).context("Failed to serialize request")?;

        {
            let mut write = self.write.lock().await;
            write
                .send(Message::Text(json.into()))
                .await
                .context("Failed to send query")?;
        }

        tracing::info!(
            %channel_id,
            prompt_len = prompt.len(),
            "Sent query to Claude Jail"
        );

        Ok(rx)
    }

    /// Close a session explicitly
    pub async fn close_session(&self, channel_id: &str) -> Result<()> {
        let request = JailRequest::CloseSession {
            channel_id: channel_id.to_string(),
        };

        let json = serde_json::to_string(&request)?;

        let mut write = self.write.lock().await;
        write.send(Message::Text(json.into())).await?;

        // Remove from pending
        let mut pending = self.pending.lock().await;
        pending.remove(channel_id);

        Ok(())
    }

    /// Get the URL this client is connected to
    pub fn url(&self) -> &str {
        &self.url
    }
}

/// Get a brief preview of tool input for display
fn get_input_preview(input: &serde_json::Value, tool_name: &str) -> String {
    let truncate = |s: &str, max: usize| -> String {
        if s.len() <= max {
            s.to_string()
        } else {
            format!("{}…", s.chars().take(max - 1).collect::<String>())
        }
    };

    let short_path = |p: &str| -> String {
        let parts: Vec<&str> = p.split('/').collect();
        if parts.len() <= 2 {
            p.to_string()
        } else {
            parts[parts.len() - 2..].join("/")
        }
    };

    match tool_name {
        "Read" => input
            .get("file_path")
            .and_then(|p| p.as_str())
            .map(short_path)
            .unwrap_or_default(),
        "Edit" | "Write" => input
            .get("file_path")
            .and_then(|p| p.as_str())
            .map(short_path)
            .unwrap_or_default(),
        "Bash" => input
            .get("command")
            .and_then(|c| c.as_str())
            .map(|c| truncate(c.lines().next().unwrap_or(""), 60))
            .unwrap_or_default(),
        "Grep" => input
            .get("pattern")
            .and_then(|p| p.as_str())
            .map(|p| format!("/{}/", truncate(p, 40)))
            .unwrap_or_default(),
        "Glob" => input
            .get("pattern")
            .and_then(|p| p.as_str())
            .map(|p| truncate(p, 50))
            .unwrap_or_default(),
        _ if tool_name.starts_with("mcp__") => input
            .get("content")
            .or_else(|| input.get("message"))
            .or_else(|| input.get("query"))
            .and_then(|v| v.as_str())
            .map(|s| truncate(s, 50))
            .unwrap_or_default(),
        _ => String::new(),
    }
}

/// Invoke Claude via the Jail service with streaming events
/// This is a compatibility wrapper that matches the existing invoke_claude_streaming interface
pub async fn invoke_claude_streaming(
    jail_client: &ClaudeJailClient,
    channel_id: &str,
    workspace: &str,
    prompt: &str,
    session_id: Option<&str>,
) -> Result<mpsc::Receiver<ClaudeEvent>> {
    jail_client
        .query(channel_id, workspace, prompt, session_id)
        .await
}
