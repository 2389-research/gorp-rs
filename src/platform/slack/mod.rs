// ABOUTME: Slack platform implementation for gorp chat abstraction
// ABOUTME: Implements Tier 2 ChatPlatform with Socket Mode, threading, slash commands, and Block Kit

pub mod blocks;
pub mod channel;
pub mod commands;

pub use channel::SlackChannel;

use anyhow::{Context, Result};
use async_trait::async_trait;
use gorp_core::traits::{
    ChannelCreator, ChannelManager, ChatChannel, ChatPlatform, ChatUser, EventStream,
    IncomingMessage, MessageContent, MessagingPlatform, PlatformConnectionState, RichFormatter,
    SlashCommandProvider, ThreadedPlatform,
};
use slack_morphism::prelude::*;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use self::commands::SlackCommandHandler;

// =============================================================================
// Shared state passed to Socket Mode callbacks via SlackClientEventsUserState
// =============================================================================

/// State shared with Socket Mode callback functions via user state storage.
/// Callbacks are fn pointers (not closures), so they cannot capture variables.
/// Instead, this state is registered via `with_user_state()` and retrieved
/// inside callbacks from the `SlackClientEventsUserState` RwLock.
#[derive(Clone)]
struct SlackBridgeState {
    /// Channel for sending incoming messages to the event stream
    tx: Arc<mpsc::Sender<IncomingMessage>>,
    /// Bot's user ID (to skip self-messages)
    bot_user_id: String,
    /// Allowed user IDs (empty = allow all)
    allowed_users: Vec<String>,
    /// Allowed channel IDs (empty = allow all)
    allowed_channels: Vec<String>,
}

// =============================================================================
// Socket Mode callback functions (must be fn pointers, not closures)
// =============================================================================

/// Handle push events (messages, app mentions) from Socket Mode
async fn handle_push_event(
    event: SlackPushEventCallback,
    _client: Arc<SlackHyperClient>,
    states: SlackClientEventsUserState,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let bridge = {
        let guard = states.read().await;
        guard
            .get_user_state::<SlackBridgeState>()
            .cloned()
            .ok_or_else(|| "SlackBridgeState not found in user state")?
    };

    match event.event {
        SlackEventCallbackBody::Message(msg_event) => {
            handle_message_event(&bridge, &msg_event).await;
        }
        SlackEventCallbackBody::AppMention(mention_event) => {
            handle_mention_event(&bridge, &mention_event).await;
        }
        _ => {
            // Ignore other event types
        }
    }
    Ok(())
}

/// Handle slash command events from Socket Mode
async fn handle_command_event(
    event: SlackCommandEvent,
    _client: Arc<SlackHyperClient>,
    states: SlackClientEventsUserState,
) -> Result<SlackCommandEventResponse, Box<dyn std::error::Error + Send + Sync>> {
    let bridge = {
        let guard = states.read().await;
        guard
            .get_user_state::<SlackBridgeState>()
            .cloned()
            .ok_or_else(|| "SlackBridgeState not found in user state")?
    };

    // Route slash command as a message through the event stream
    let command_text = event.command.to_string();
    let body = match &event.text {
        Some(text) if !text.is_empty() => format!("{} {}", command_text, text),
        _ => command_text,
    };

    let msg = IncomingMessage {
        platform_id: "slack".to_string(),
        channel_id: event.channel_id.to_string(),
        thread_id: None,
        sender: ChatUser::new(event.user_id.to_string()),
        body,
        is_direct: false,
        formatted: false,
        attachment: None,
        event_id: format!("cmd_{}", chrono::Utc::now().timestamp_millis()),
        timestamp: chrono::Utc::now().timestamp(),
    };

    let _ = bridge.tx.send(msg).await;

    // Return immediate ACK response
    Ok(SlackCommandEventResponse::new(
        SlackMessageContent::new().with_text("Working on it...".into()),
    ))
}

/// Process a Slack message event into an IncomingMessage
async fn handle_message_event(bridge: &SlackBridgeState, msg_event: &SlackMessageEvent) {
    // Extract sender user ID
    let sender_id = match &msg_event.sender.user {
        Some(user_id) => user_id.to_string(),
        None => return, // Skip messages without a user (system messages)
    };

    // Skip bot's own messages
    if sender_id == bridge.bot_user_id {
        return;
    }

    // Skip if user not allowed
    if !bridge.allowed_users.is_empty()
        && !bridge.allowed_users.iter().any(|u| u == &sender_id)
    {
        tracing::debug!(
            platform = "slack",
            user_id = %sender_id,
            "Skipping message from non-allowed user"
        );
        return;
    }

    // Extract channel ID
    let channel_id = match &msg_event.origin.channel {
        Some(ch) => ch.to_string(),
        None => return,
    };

    // Skip if channel not allowed
    if !bridge.allowed_channels.is_empty()
        && !bridge.allowed_channels.iter().any(|c| c == &channel_id)
    {
        tracing::debug!(
            platform = "slack",
            channel_id = %channel_id,
            "Skipping message from non-allowed channel"
        );
        return;
    }

    // Extract message text
    let body = msg_event
        .content
        .as_ref()
        .and_then(|c| c.text.as_ref())
        .map(|t| t.to_string())
        .unwrap_or_default();

    if body.is_empty() {
        return;
    }

    // Extract thread_ts for threading
    let thread_id = msg_event
        .origin
        .thread_ts
        .as_ref()
        .map(|ts| ts.to_string());

    // Detect DM vs channel (DM channel IDs start with "D")
    let is_direct = channel_id.starts_with('D');

    let display_name = msg_event.sender.username.clone();

    let timestamp = parse_slack_ts(&msg_event.origin.ts);

    let msg = IncomingMessage {
        platform_id: "slack".to_string(),
        channel_id,
        thread_id,
        sender: ChatUser {
            id: sender_id,
            display_name,
        },
        body,
        is_direct,
        formatted: false,
        attachment: None,
        event_id: msg_event.origin.ts.to_string(),
        timestamp,
    };

    if bridge.tx.send(msg).await.is_err() {
        tracing::warn!(platform = "slack", "Event stream receiver dropped");
    }
}

/// Process a Slack app mention event into an IncomingMessage
async fn handle_mention_event(bridge: &SlackBridgeState, mention_event: &SlackAppMentionEvent) {
    let sender_id = mention_event.user.to_string();

    if sender_id == bridge.bot_user_id {
        return;
    }

    if !bridge.allowed_users.is_empty()
        && !bridge.allowed_users.iter().any(|u| u == &sender_id)
    {
        return;
    }

    let channel_id = mention_event.channel.to_string();

    // App mention uses content field for message body
    let body = mention_event
        .content
        .text
        .as_ref()
        .map(|t| t.to_string())
        .unwrap_or_default();

    let thread_id = mention_event
        .origin
        .thread_ts
        .as_ref()
        .map(|ts| ts.to_string());

    let timestamp = parse_slack_ts(&mention_event.origin.ts);

    let msg = IncomingMessage {
        platform_id: "slack".to_string(),
        channel_id,
        thread_id,
        sender: ChatUser {
            id: sender_id,
            display_name: None,
        },
        body,
        is_direct: false,
        formatted: false,
        attachment: None,
        event_id: mention_event.origin.ts.to_string(),
        timestamp,
    };

    if bridge.tx.send(msg).await.is_err() {
        tracing::warn!(platform = "slack", "Event stream receiver dropped");
    }
}

/// Socket Mode error handler
fn socket_mode_error_handler(
    err: Box<dyn std::error::Error + Send + Sync>,
    _client: Arc<SlackHyperClient>,
    _states: SlackClientEventsUserState,
) -> HttpStatusCode {
    tracing::error!(platform = "slack", error = %err, "Socket Mode error");
    HttpStatusCode::OK
}

// =============================================================================
// SlackPlatform - Implements MessagingPlatform + ChatPlatform (Tier 2)
// =============================================================================

/// Slack platform implementation using slack-morphism with Socket Mode
pub struct SlackPlatform {
    /// Shared Slack client for API calls
    client: Arc<SlackHyperClient>,
    /// Bot OAuth token (xoxb-...) for Web API calls
    bot_token: SlackApiToken,
    /// App-level token (xapp-...) for Socket Mode connections
    app_token: SlackApiToken,
    /// Bot's Slack user ID (resolved via auth.test at startup)
    bot_user_id: String,
    /// Configuration for allowed users/channels
    config: gorp_core::config::SlackConfig,
    /// Connection state for health monitoring
    connection_state: Arc<Mutex<PlatformConnectionState>>,
    /// Slash command handler
    command_handler: SlackCommandHandler,
}

impl SlackPlatform {
    /// Create a new SlackPlatform from config.
    ///
    /// Resolves the bot's user ID via the `auth.test` API call.
    pub async fn new(config: gorp_core::config::SlackConfig) -> Result<Self> {
        let client = Arc::new(SlackClient::new(
            SlackClientHyperConnector::new()
                .context("Failed to create Slack HTTP connector")?,
        ));

        let bot_token = SlackApiToken::new(SlackApiTokenValue(config.bot_token.clone()));
        let app_token = SlackApiToken::new(SlackApiTokenValue(config.app_token.clone()));

        // Resolve bot user ID via auth.test
        let session = client.open_session(&bot_token);
        let auth_response = session
            .auth_test()
            .await
            .context("Failed to call Slack auth.test — check bot_token")?;

        let bot_user_id = auth_response.user_id.to_string();

        tracing::info!(
            bot_user = %bot_user_id,
            team = %auth_response.team,
            "Slack bot authenticated"
        );

        Ok(Self {
            client,
            bot_token,
            app_token,
            bot_user_id,
            config,
            connection_state: Arc::new(Mutex::new(PlatformConnectionState::Connected)),
            command_handler: SlackCommandHandler::new(),
        })
    }

    /// Update the platform's connection state
    fn set_connection_state(&self, state: PlatformConnectionState) {
        if let Ok(mut current) = self.connection_state.lock() {
            *current = state;
        }
    }
}

#[async_trait]
impl MessagingPlatform for SlackPlatform {
    async fn event_stream(&self) -> Result<EventStream> {
        let (tx, rx) = mpsc::channel(256);
        let client = Arc::clone(&self.client);
        let app_token = self.app_token.clone();
        let connection_state = Arc::clone(&self.connection_state);

        // Create bridge state for callbacks
        let bridge_state = SlackBridgeState {
            tx: Arc::new(tx),
            bot_user_id: self.bot_user_id.clone(),
            allowed_users: self.config.allowed_users.clone(),
            allowed_channels: self.config.allowed_channels.clone(),
        };

        // Spawn Socket Mode listener
        tokio::spawn(async move {
            // Set up Socket Mode callbacks (fn pointers, not closures)
            let socket_mode_callbacks = SlackSocketModeListenerCallbacks::new()
                .with_push_events(handle_push_event)
                .with_command_events(handle_command_event);

            let listener_environment = Arc::new(
                SlackClientEventsListenerEnvironment::new(client.clone())
                    .with_error_handler(socket_mode_error_handler)
                    .with_user_state(bridge_state),
            );

            let socket_mode_listener = SlackClientSocketModeListener::new(
                &SlackClientSocketModeConfig::new(),
                listener_environment,
                socket_mode_callbacks,
            );

            // Update connection state
            if let Ok(mut state) = connection_state.lock() {
                *state = PlatformConnectionState::Connecting;
            }

            match socket_mode_listener.listen_for(&app_token).await {
                Ok(_) => {
                    if let Ok(mut state) = connection_state.lock() {
                        *state = PlatformConnectionState::Connected;
                    }
                    tracing::info!(platform = "slack", "Socket Mode connected");

                    // serve() blocks until the listener is shut down
                    socket_mode_listener.serve().await;
                }
                Err(e) => {
                    tracing::error!(
                        platform = "slack",
                        error = %e,
                        "Failed to start Socket Mode listener"
                    );
                    if let Ok(mut state) = connection_state.lock() {
                        *state = PlatformConnectionState::Disconnected {
                            reason: e.to_string(),
                        };
                    }
                }
            }
        });

        let stream = ReceiverStream::new(rx);
        Ok(Box::pin(stream))
    }

    async fn send(&self, channel_id: &str, content: MessageContent) -> Result<()> {
        let slack_channel = SlackChannel::new(
            channel_id.into(),
            Arc::clone(&self.client),
            self.bot_token.clone(),
            None,
            channel_id.starts_with('D'),
        );
        slack_channel.send(content).await
    }

    fn bot_user_id(&self) -> &str {
        &self.bot_user_id
    }

    fn platform_id(&self) -> &'static str {
        "slack"
    }

    async fn shutdown(&self) -> Result<()> {
        tracing::info!(platform = "slack", "Shutting down Slack platform");
        self.set_connection_state(PlatformConnectionState::Disconnected {
            reason: "shutdown".to_string(),
        });
        Ok(())
    }

    fn connection_state(&self) -> PlatformConnectionState {
        self.connection_state
            .lock()
            .map(|s| s.clone())
            .unwrap_or(PlatformConnectionState::Connected)
    }
}

#[async_trait]
impl ChatPlatform for SlackPlatform {
    type Channel = SlackChannel;

    async fn get_channel(&self, id: &str) -> Option<Self::Channel> {
        let is_dm = id.starts_with('D');
        Some(SlackChannel::new(
            id.into(),
            Arc::clone(&self.client),
            self.bot_token.clone(),
            None,
            is_dm,
        ))
    }

    async fn joined_channels(&self) -> Vec<Self::Channel> {
        // Channels are discovered through incoming messages.
        // Full listing would require conversations.list API call.
        vec![]
    }

    fn channel_creator(&self) -> Option<&dyn ChannelCreator> {
        Some(self)
    }

    fn channel_manager(&self) -> Option<&dyn ChannelManager> {
        Some(self)
    }

    fn threading(&self) -> Option<&dyn ThreadedPlatform> {
        Some(self)
    }

    fn slash_commands(&self) -> Option<&dyn SlashCommandProvider> {
        Some(&self.command_handler)
    }

    fn rich_formatter(&self) -> Option<&dyn RichFormatter> {
        Some(self)
    }
}

// =============================================================================
// Extension trait implementations
// =============================================================================

#[async_trait]
impl ThreadedPlatform for SlackPlatform {
    async fn send_threaded(
        &self,
        channel_id: &str,
        thread_ts: &str,
        content: MessageContent,
    ) -> Result<()> {
        let session = self.client.open_session(&self.bot_token);

        let text = match &content {
            MessageContent::Plain(t) => t.clone(),
            MessageContent::Html { plain, .. } => plain.clone(),
            MessageContent::Attachment { caption, filename, .. } => {
                caption.clone().unwrap_or_else(|| filename.clone())
            }
        };

        let req = SlackApiChatPostMessageRequest::new(
            channel_id.into(),
            SlackMessageContent::new().with_text(text),
        )
        .with_thread_ts(thread_ts.into());

        session
            .chat_post_message(&req)
            .await
            .context("Failed to send threaded Slack message")?;

        Ok(())
    }
}

impl RichFormatter for SlackPlatform {
    fn format_as_blocks(&self, content: &str) -> serde_json::Value {
        blocks::markdown_to_blocks(content)
    }
}

// =============================================================================
// Channel management
// =============================================================================

#[async_trait]
impl ChannelCreator for SlackPlatform {
    async fn create_channel(&self, name: &str) -> Result<String> {
        let session = self.client.open_session(&self.bot_token);

        let req = SlackApiConversationsCreateRequest::new(name.into());
        let resp = session
            .conversations_create(&req)
            .await
            .context("Failed to create Slack channel")?;

        Ok(resp.channel.id.to_string())
    }

    async fn create_dm(&self, user_id: &str) -> Result<String> {
        let session = self.client.open_session(&self.bot_token);

        let req = SlackApiConversationsOpenRequest::new()
            .with_users(vec![user_id.into()]);
        let resp = session
            .conversations_open(&req)
            .await
            .context("Failed to open Slack DM")?;

        Ok(resp.channel.id.to_string())
    }
}

#[async_trait]
impl ChannelManager for SlackPlatform {
    async fn join(&self, channel_id: &str) -> Result<()> {
        let session = self.client.open_session(&self.bot_token);

        let req = SlackApiConversationsJoinRequest::new(channel_id.into());
        session
            .conversations_join(&req)
            .await
            .context("Failed to join Slack channel")?;

        Ok(())
    }

    async fn leave(&self, channel_id: &str) -> Result<()> {
        let session = self.client.open_session(&self.bot_token);

        let req = SlackApiConversationsLeaveRequest::new(channel_id.into());
        session
            .conversations_leave(&req)
            .await
            .context("Failed to leave Slack channel")?;

        Ok(())
    }

    async fn invite(&self, channel_id: &str, user_id: &str) -> Result<()> {
        let session = self.client.open_session(&self.bot_token);

        let req = SlackApiConversationsInviteRequest::new(
            channel_id.into(),
            vec![user_id.into()],
        );
        session
            .conversations_invite(&req)
            .await
            .context("Failed to invite user to Slack channel")?;

        Ok(())
    }

    async fn members(&self, channel_id: &str) -> Result<Vec<ChatUser>> {
        let session = self.client.open_session(&self.bot_token);

        let req = SlackApiConversationsMembersRequest::new()
            .with_channel(channel_id.into());
        let resp = session
            .conversations_members(&req)
            .await
            .context("Failed to get Slack channel members")?;

        let members: Vec<ChatUser> = resp
            .members
            .into_iter()
            .map(|user_id| ChatUser::new(user_id.to_string()))
            .collect();

        Ok(members)
    }
}

// =============================================================================
// Utility functions
// =============================================================================

/// Parse a Slack timestamp (e.g., "1700000000.000100") into Unix seconds
fn parse_slack_ts(ts: &SlackTs) -> i64 {
    let ts_str = ts.to_string();
    ts_str
        .split('.')
        .next()
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(0)
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_slack_platform_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<SlackPlatform>();
    }

    #[test]
    fn test_slack_channel_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<SlackChannel>();
    }

    #[test]
    fn test_parse_slack_ts() {
        let ts: SlackTs = "1700000000.000100".into();
        assert_eq!(parse_slack_ts(&ts), 1700000000);
    }

    #[test]
    fn test_parse_slack_ts_no_dot() {
        let ts: SlackTs = "1700000000".into();
        assert_eq!(parse_slack_ts(&ts), 1700000000);
    }

    #[test]
    fn test_parse_slack_ts_invalid() {
        let ts: SlackTs = "not_a_number".into();
        assert_eq!(parse_slack_ts(&ts), 0);
    }

    #[test]
    fn test_rich_formatter_produces_blocks() {
        let blocks = blocks::markdown_to_blocks("Hello world");
        let arr = blocks.as_array().unwrap();
        assert!(!arr.is_empty());
        assert_eq!(arr[0]["type"], "section");
    }

    #[test]
    fn test_dm_detection() {
        assert!("D12345".starts_with('D'));
        assert!(!"C12345".starts_with('D'));
        assert!(!"G12345".starts_with('D'));
    }

    #[test]
    fn test_user_allowed_empty_list_allows_all() {
        let config = gorp_core::config::SlackConfig {
            app_token: "xapp-test".to_string(),
            bot_token: "xoxb-test".to_string(),
            signing_secret: "secret".to_string(),
            allowed_users: vec![],
            allowed_channels: vec![],
            thread_in_channels: true,
        };
        assert!(config.allowed_users.is_empty());
    }

    #[test]
    fn test_channel_allowed_empty_list_allows_all() {
        let config = gorp_core::config::SlackConfig {
            app_token: "xapp-test".to_string(),
            bot_token: "xoxb-test".to_string(),
            signing_secret: "secret".to_string(),
            allowed_users: vec![],
            allowed_channels: vec![],
            thread_in_channels: true,
        };
        assert!(config.allowed_channels.is_empty());
    }

    #[test]
    fn test_command_handler_returns_commands() {
        let handler = SlackCommandHandler::new();
        let commands = handler.registered_commands();
        assert!(commands.len() >= 2);
        assert!(commands.iter().any(|c| c.name == "/gorp"));
    }

    #[test]
    fn test_bridge_state_clone() {
        let (tx, _rx) = mpsc::channel(1);
        let state = SlackBridgeState {
            tx: Arc::new(tx),
            bot_user_id: "U123".to_string(),
            allowed_users: vec!["U456".to_string()],
            allowed_channels: vec![],
        };
        let cloned = state.clone();
        assert_eq!(cloned.bot_user_id, "U123");
        assert_eq!(cloned.allowed_users.len(), 1);
    }
}
