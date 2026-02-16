// ABOUTME: Slack gateway adapter — bridges Slack events and the message bus.
// ABOUTME: Translates between Slack channel events and BusMessage/BusResponse types.

use std::sync::Arc;

use async_trait::async_trait;
use slack_morphism::prelude::*;
use tokio::sync::Mutex;

use crate::bus::{BusMessage, MessageBus, MessageSource, ResponseContent, SessionTarget};
use crate::gateway::GatewayAdapter;
use gorp_core::config::SlackConfig;

/// Slack gateway adapter that bridges Slack channels and the message bus.
///
/// Handles two directions:
/// - Inbound: Slack channel messages -> BusMessage published to bus
/// - Outbound: BusResponse -> Slack channel messages
pub struct SlackAdapter {
    client: Arc<SlackHyperClient>,
    bot_token: SlackApiToken,
    config: SlackConfig,
    bus: Mutex<Option<Arc<MessageBus>>>,
}

impl SlackAdapter {
    pub fn new(
        client: Arc<SlackHyperClient>,
        bot_token: SlackApiToken,
        config: SlackConfig,
    ) -> Self {
        Self {
            client,
            bot_token,
            config,
            bus: Mutex::new(None),
        }
    }

    /// Get a reference to the Slack configuration.
    pub fn config(&self) -> &SlackConfig {
        &self.config
    }
}

#[async_trait]
impl GatewayAdapter for SlackAdapter {
    fn platform_id(&self) -> &str {
        "slack"
    }

    async fn start(&self, bus: Arc<MessageBus>) -> anyhow::Result<()> {
        // Store bus reference for later use
        *self.bus.lock().await = Some(bus.clone());

        // Spawn outbound loop: subscribe to bus responses, filter for sessions
        // bound to slack channels, and send responses to the appropriate channels
        let client = Arc::clone(&self.client);
        let bot_token = self.bot_token.clone();
        let outbound_bus = bus.clone();
        tokio::spawn(async move {
            let mut rx = outbound_bus.subscribe_responses();
            loop {
                match rx.recv().await {
                    Ok(resp) => {
                        // Find all slack channels bound to this session
                        let bindings = outbound_bus
                            .bindings_for_session_async(&resp.session_name)
                            .await;
                        for (platform_id, channel_id) in bindings {
                            if platform_id == "slack" {
                                if let Err(e) = send_to_channel(
                                    &client,
                                    &bot_token,
                                    &channel_id,
                                    &resp.content,
                                )
                                .await
                                {
                                    tracing::error!(
                                        channel = %channel_id,
                                        error = %e,
                                        "Failed to send response to Slack channel"
                                    );
                                }
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!(skipped = n, "Slack adapter outbound lagged");
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        tracing::info!("Bus closed, slack adapter shutting down");
                        break;
                    }
                }
            }
        });

        tracing::info!("Slack adapter started");
        Ok(())
    }

    async fn send(&self, channel_id: &str, content: ResponseContent) -> anyhow::Result<()> {
        send_to_channel(&self.client, &self.bot_token, channel_id, &content).await
    }

    async fn stop(&self) -> anyhow::Result<()> {
        // Clear bus reference
        *self.bus.lock().await = None;
        tracing::info!("Slack adapter stopped");
        Ok(())
    }
}

/// Send a ResponseContent to a Slack channel via the Web API.
async fn send_to_channel(
    client: &SlackHyperClient,
    bot_token: &SlackApiToken,
    channel_id: &str,
    content: &ResponseContent,
) -> anyhow::Result<()> {
    let session = client.open_session(bot_token);
    let text = response_to_slack_message(content);

    if text.is_empty() {
        return Ok(());
    }

    let req = SlackApiChatPostMessageRequest::new(
        channel_id.into(),
        SlackMessageContent::new().with_text(text),
    );

    session
        .chat_post_message(&req)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to send Slack message to {}: {}", channel_id, e))?;

    Ok(())
}

/// Convert a ResponseContent to a plain text message suitable for Slack.
///
/// Slack's mrkdwn format is close enough to markdown that we pass text through
/// largely unchanged. Error and SystemNotice variants get prefixed for visibility.
pub fn response_to_slack_message(content: &ResponseContent) -> String {
    match content {
        ResponseContent::Chunk(text) | ResponseContent::Complete(text) => text.clone(),
        ResponseContent::Error(error) => format!(":warning: *Error:* {}", error),
        ResponseContent::SystemNotice(text) => format!(":information_source: {}", text),
    }
}

/// Convert a Slack channel event into a BusMessage.
///
/// Used by event handlers to normalize Slack events for the bus.
pub fn slack_event_to_bus_message(
    channel_id: &str,
    event_id: &str,
    sender: &str,
    body: &str,
    session_target: SessionTarget,
) -> BusMessage {
    BusMessage {
        id: event_id.to_string(),
        source: MessageSource::Platform {
            platform_id: "slack".to_string(),
            channel_id: channel_id.to_string(),
        },
        session_target,
        sender: sender.to_string(),
        body: body.to_string(),
        timestamp: chrono::Utc::now(),
    }
}
