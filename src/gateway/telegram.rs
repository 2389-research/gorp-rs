// ABOUTME: Telegram gateway adapter — bridges Telegram events and the message bus.
// ABOUTME: Translates between Telegram chat events and BusMessage/BusResponse types.

use std::sync::Arc;

use async_trait::async_trait;
use teloxide::prelude::*;
use tokio::sync::Mutex;

use crate::bus::{BusMessage, MessageBus, MessageSource, ResponseContent, SessionTarget};
use crate::gateway::GatewayAdapter;
use gorp_core::config::TelegramConfig;

/// Telegram gateway adapter that bridges Telegram chats and the message bus.
///
/// Handles two directions:
/// - Inbound: Telegram chat messages -> BusMessage published to bus
/// - Outbound: BusResponse -> Telegram chat messages
pub struct TelegramAdapter {
    bot: Bot,
    config: TelegramConfig,
    bus: Mutex<Option<Arc<MessageBus>>>,
}

impl TelegramAdapter {
    pub fn new(bot: Bot, config: TelegramConfig) -> Self {
        Self {
            bot,
            config,
            bus: Mutex::new(None),
        }
    }

    /// Get a reference to the Telegram configuration.
    pub fn config(&self) -> &TelegramConfig {
        &self.config
    }
}

#[async_trait]
impl GatewayAdapter for TelegramAdapter {
    fn platform_id(&self) -> &str {
        "telegram"
    }

    async fn start(&self, bus: Arc<MessageBus>) -> anyhow::Result<()> {
        // Store bus reference for later use
        *self.bus.lock().await = Some(bus.clone());

        // Spawn outbound loop: subscribe to bus responses, filter for sessions
        // bound to telegram chats, and send responses to the appropriate chats
        let bot = self.bot.clone();
        let outbound_bus = bus.clone();
        tokio::spawn(async move {
            let mut rx = outbound_bus.subscribe_responses();
            loop {
                match rx.recv().await {
                    Ok(resp) => {
                        // Find all telegram chats bound to this session
                        let bindings = outbound_bus
                            .bindings_for_session_async(&resp.session_name)
                            .await;
                        for (platform_id, channel_id) in bindings {
                            if platform_id == "telegram" {
                                if let Err(e) =
                                    send_to_chat(&bot, &channel_id, &resp.content).await
                                {
                                    tracing::error!(
                                        chat_id = %channel_id,
                                        error = %e,
                                        "Failed to send response to Telegram chat"
                                    );
                                }
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!(skipped = n, "Telegram adapter outbound lagged");
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        tracing::info!("Bus closed, telegram adapter shutting down");
                        break;
                    }
                }
            }
        });

        tracing::info!("Telegram adapter started");
        Ok(())
    }

    async fn send(&self, channel_id: &str, content: ResponseContent) -> anyhow::Result<()> {
        send_to_chat(&self.bot, channel_id, &content).await
    }

    async fn stop(&self) -> anyhow::Result<()> {
        // Clear bus reference
        *self.bus.lock().await = None;
        tracing::info!("Telegram adapter stopped");
        Ok(())
    }
}

/// Send a ResponseContent to a Telegram chat via the Bot API.
async fn send_to_chat(
    bot: &Bot,
    chat_id: &str,
    content: &ResponseContent,
) -> anyhow::Result<()> {
    let chat_id: ChatId = ChatId(
        chat_id
            .parse::<i64>()
            .map_err(|e| anyhow::anyhow!("Invalid Telegram chat ID '{}': {}", chat_id, e))?,
    );

    let text = response_to_telegram_message(content);

    if text.is_empty() {
        return Ok(());
    }

    bot.send_message(chat_id, text)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to send Telegram message: {}", e))?;

    Ok(())
}

/// Convert a ResponseContent to a plain text message suitable for Telegram.
///
/// Telegram supports a subset of HTML and Markdown, but for gateway-level
/// delivery we use plain text to keep things simple and reliable. Error and
/// SystemNotice variants get unicode-prefixed for visibility.
pub fn response_to_telegram_message(content: &ResponseContent) -> String {
    match content {
        ResponseContent::Chunk(text) | ResponseContent::Complete(text) => text.clone(),
        ResponseContent::Error(error) => format!("\u{26A0}\u{FE0F} Error: {}", error),
        ResponseContent::SystemNotice(text) => format!("\u{2139}\u{FE0F} {}", text),
    }
}

/// Convert a Telegram chat event into a BusMessage.
///
/// Used by event handlers to normalize Telegram events for the bus.
pub fn telegram_event_to_bus_message(
    chat_id: &str,
    message_id: &str,
    sender: &str,
    body: &str,
    session_target: SessionTarget,
) -> BusMessage {
    BusMessage {
        id: message_id.to_string(),
        source: MessageSource::Platform {
            platform_id: "telegram".to_string(),
            channel_id: chat_id.to_string(),
        },
        session_target,
        sender: sender.to_string(),
        body: body.to_string(),
        timestamp: chrono::Utc::now(),
    }
}
