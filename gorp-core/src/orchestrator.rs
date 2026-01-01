// ABOUTME: Core message orchestration loop for AI agent interactions
// ABOUTME: Platform-agnostic message handling using ChatInterface trait

use crate::{
    commands::{parse_message, Command, ParseResult},
    metrics,
    session::{Channel, SessionStore},
    traits::{ChatInterface, ChatRoom, IncomingMessage, MessageContent},
    utils::{chunk_message, markdown_to_html, strip_function_calls, MAX_CHUNK_SIZE},
    warm_session::{
        prepare_session_async, send_prompt_with_handle, SharedWarmSessionManager, WarmSessionHandle,
    },
};
use anyhow::Result;
use gorp_agent::{AgentEvent, ErrorCode};
use std::sync::Arc;

/// Configuration for the orchestrator
#[derive(Debug, Clone)]
pub struct OrchestratorConfig {
    /// Bot command prefix (e.g., "!claude")
    pub bot_prefix: String,
    /// Allowed user IDs (empty = allow all)
    pub allowed_users: Vec<String>,
    /// Management room ID for admin commands
    pub management_room: Option<String>,
    /// Whether debug mode is enabled globally
    pub debug_mode: bool,
}

impl Default for OrchestratorConfig {
    fn default() -> Self {
        Self {
            bot_prefix: "!claude".to_string(),
            allowed_users: Vec::new(),
            management_room: None,
            debug_mode: false,
        }
    }
}

/// Result of handling a message
#[derive(Debug)]
pub enum HandleResult {
    /// Message was handled, response sent
    Handled,
    /// Message was ignored (not for us, not authorized, etc.)
    Ignored,
    /// Error occurred during handling
    Error(String),
}

/// Standard commands recognized by the orchestrator
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StandardCommand {
    Help,
    Status,
    Reset,
    Backend(BackendSubcommand),
    Unknown(String),
}

/// Backend-related subcommands
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackendSubcommand {
    Get,
    List,
    Set { backend: String },
    Reset,
}

impl Command {
    /// Convert a parsed command to a standard command enum
    pub fn as_standard(&self) -> StandardCommand {
        match self.name.as_str() {
            "help" | "h" => StandardCommand::Help,
            "status" | "s" => StandardCommand::Status,
            "reset" | "r" => StandardCommand::Reset,
            "backend" | "b" => {
                match self.first_arg() {
                    None | Some("get") => StandardCommand::Backend(BackendSubcommand::Get),
                    Some("list") | Some("ls") => StandardCommand::Backend(BackendSubcommand::List),
                    Some("set") => {
                        if let Some(backend) = self.arg(1) {
                            StandardCommand::Backend(BackendSubcommand::Set {
                                backend: backend.to_string(),
                            })
                        } else {
                            StandardCommand::Backend(BackendSubcommand::Get)
                        }
                    }
                    Some("reset") | Some("clear") => {
                        StandardCommand::Backend(BackendSubcommand::Reset)
                    }
                    Some(other) => {
                        // Treat unknown subcommand as backend name to set
                        StandardCommand::Backend(BackendSubcommand::Set {
                            backend: other.to_string(),
                        })
                    }
                }
            }
            other => StandardCommand::Unknown(other.to_string()),
        }
    }
}

/// Orchestrates message handling between chat interface and AI agent
pub struct Orchestrator<I: ChatInterface> {
    interface: Arc<I>,
    session_store: SessionStore,
    warm_manager: SharedWarmSessionManager,
    config: OrchestratorConfig,
}

impl<I: ChatInterface> Orchestrator<I> {
    /// Create a new orchestrator
    pub fn new(
        interface: Arc<I>,
        session_store: SessionStore,
        warm_manager: SharedWarmSessionManager,
        config: OrchestratorConfig,
    ) -> Self {
        Self {
            interface,
            session_store,
            warm_manager,
            config,
        }
    }

    /// Handle an incoming message
    pub async fn handle_message(&self, msg: IncomingMessage) -> Result<HandleResult> {
        // Skip our own messages
        if self.interface.is_self(&msg.sender.id) {
            return Ok(HandleResult::Ignored);
        }

        // Check allowed users (empty list = allow all)
        if !self.config.allowed_users.is_empty()
            && !self.config.allowed_users.contains(&msg.sender.id)
        {
            tracing::debug!(sender = %msg.sender.id, "User not in allowed list");
            return Ok(HandleResult::Ignored);
        }

        // Get the room
        let room = match self.interface.get_room(&msg.room_id).await {
            Some(r) => r,
            None => {
                tracing::warn!(room_id = %msg.room_id, "Room not found");
                return Ok(HandleResult::Ignored);
            }
        };

        // Parse the message
        let parsed = parse_message(&msg.body, &self.config.bot_prefix);

        match parsed {
            ParseResult::Ignore => Ok(HandleResult::Ignored),
            ParseResult::Command(cmd) => self.handle_command(&room, &msg, cmd).await,
            ParseResult::Message(body) => self.handle_agent_message(&room, &msg, &body).await,
        }
    }

    /// Handle a parsed command
    async fn handle_command(
        &self,
        room: &I::Room,
        _msg: &IncomingMessage,
        cmd: Command,
    ) -> Result<HandleResult> {
        let std_cmd = cmd.as_standard();
        metrics::record_command(&cmd.name);

        match std_cmd {
            StandardCommand::Help => {
                room.send(MessageContent::plain(self.help_text())).await?;
                Ok(HandleResult::Handled)
            }
            StandardCommand::Status => {
                let status = self.get_status(room).await?;
                room.send(MessageContent::plain(status)).await?;
                Ok(HandleResult::Handled)
            }
            StandardCommand::Reset => self.handle_reset(room).await,
            StandardCommand::Backend(sub) => self.handle_backend_command(room, sub).await,
            StandardCommand::Unknown(name) => {
                room.send(MessageContent::plain(format!(
                    "Unknown command: {}. Try !help for available commands.",
                    name
                )))
                .await?;
                Ok(HandleResult::Handled)
            }
        }
    }

    /// Handle a message that should go to the AI agent
    async fn handle_agent_message(
        &self,
        room: &I::Room,
        _msg: &IncomingMessage,
        body: &str,
    ) -> Result<HandleResult> {
        let start_time = std::time::Instant::now();

        // Get channel for this room
        let channel = match self.session_store.get_by_room(room.id())? {
            Some(c) => c,
            None => {
                room.send(MessageContent::plain(
                    "No channel configured for this room. Use !help for setup instructions.",
                ))
                .await?;
                return Ok(HandleResult::Handled);
            }
        };

        // Start typing indicator
        room.set_typing(true).await?;

        // Record the claude invocation
        metrics::record_claude_invocation("orchestrator");

        // Prepare session (creates or resumes)
        let (session_handle, session_id, is_new_session) =
            match prepare_session_async(&self.warm_manager, &channel).await {
                Ok(result) => result,
                Err(e) => {
                    room.set_typing(false).await?;
                    metrics::record_error("warm_session");
                    room.send(MessageContent::plain(format!(
                        "Failed to prepare session: {}",
                        e
                    )))
                    .await?;
                    return Ok(HandleResult::Error(e.to_string()));
                }
            };

        // Update session store if a new session was created
        if is_new_session {
            if let Err(e) = self.session_store.update_session_id(room.id(), &session_id) {
                tracing::warn!(error = %e, "Failed to update session ID in store");
            }
        }

        // Send prompt to agent
        tracing::info!(
            channel = %channel.channel_name,
            session_id = %session_id,
            "Sending prompt to agent"
        );

        let mut event_rx = match send_prompt_with_handle(&session_handle, &session_id, body).await {
            Ok(rx) => rx,
            Err(e) => {
                room.set_typing(false).await?;
                metrics::record_error("prompt_send");
                room.send(MessageContent::plain(format!(
                    "Failed to send prompt: {}",
                    e
                )))
                .await?;
                return Ok(HandleResult::Error(e.to_string()));
            }
        };

        // Process agent events
        let result = self
            .process_agent_events(room, &channel, &session_handle, &session_id, &mut event_rx)
            .await;

        // Stop typing indicator
        room.set_typing(false).await?;

        // Record total processing time
        let duration = start_time.elapsed().as_secs_f64();
        metrics::record_message_processing_duration(duration);

        result
    }

    /// Process events from the agent
    async fn process_agent_events(
        &self,
        room: &I::Room,
        channel: &Channel,
        session_handle: &WarmSessionHandle,
        session_id: &str,
        event_rx: &mut gorp_agent::EventReceiver,
    ) -> Result<HandleResult> {
        let mut full_response = String::new();
        let mut tools_used: Vec<String> = Vec::new();
        let mut session_id_from_event: Option<String> = None;
        let claude_start = std::time::Instant::now();

        while let Some(event) = event_rx.recv().await {
            match event {
                AgentEvent::ToolStart { name, input, .. } => {
                    tools_used.push(name.clone());
                    metrics::record_tool_used(&name);

                    // Send tool notification if debug mode is enabled
                    if self.config.debug_mode || self.is_debug_enabled(&channel.directory) {
                        let input_preview: String = input
                            .as_object()
                            .and_then(|o| {
                                o.get("command").or(o.get("file_path")).or(o.get("pattern"))
                            })
                            .and_then(|v| v.as_str())
                            .map(|s| s.chars().take(50).collect())
                            .unwrap_or_default();

                        let (plain, html) = if input_preview.is_empty() {
                            (format!("Tool: {}", name), format!("<code>{}</code>", name))
                        } else {
                            (
                                format!("Tool: {} - {}", name, input_preview),
                                format!("<code>{}</code> - <code>{}</code>", name, input_preview),
                            )
                        };

                        if let Err(e) = room.send(MessageContent::html(&plain, &html)).await {
                            tracing::warn!(error = %e, "Failed to send tool notification");
                        }
                    }
                }

                AgentEvent::ToolEnd { .. } => {
                    tracing::debug!("Tool completed");
                }

                AgentEvent::ToolProgress { .. } => {
                    tracing::debug!("Tool progress update");
                }

                AgentEvent::Text(text) => {
                    full_response.push_str(&text);
                }

                AgentEvent::Result { text, usage, .. } => {
                    // Use accumulated text if we have it, otherwise use result text
                    if full_response.is_empty() {
                        full_response = text;
                    }

                    // Record usage metrics if available
                    if let Some(usage) = usage {
                        metrics::record_claude_tokens(
                            usage.input_tokens,
                            usage.output_tokens,
                            usage.cache_read_tokens.unwrap_or(0),
                            usage.cache_write_tokens.unwrap_or(0),
                        );
                        if let Some(cost) = usage.cost_usd {
                            metrics::record_claude_cost_cents((cost * 100.0) as u64);
                        }
                    }

                    tracing::info!(
                        response_len = full_response.len(),
                        tools_count = tools_used.len(),
                        "Agent session completed"
                    );
                    break;
                }

                AgentEvent::Error {
                    code,
                    message,
                    recoverable,
                } => {
                    tracing::error!(code = ?code, message = %message, recoverable, "Agent error");

                    // Handle session orphaned errors
                    if code == ErrorCode::SessionOrphaned {
                        return self
                            .handle_session_orphaned(room, channel, session_handle)
                            .await;
                    }

                    metrics::record_error("agent_error");
                    room.send(MessageContent::plain(format!("Agent error: {}", message)))
                        .await?;
                    return Ok(HandleResult::Error(message));
                }

                AgentEvent::SessionInvalid { reason } => {
                    tracing::warn!(reason = %reason, "Session invalid");
                    return self
                        .handle_session_orphaned(room, channel, session_handle)
                        .await;
                }

                AgentEvent::SessionChanged { new_session_id } => {
                    tracing::info!(
                        old_session = %session_id,
                        new_session = %new_session_id,
                        "Session ID changed during execution"
                    );
                    session_id_from_event = Some(new_session_id);
                }

                AgentEvent::Custom { kind, .. } => {
                    tracing::debug!(kind = %kind, "Received custom event");
                }
            }
        }

        // Record claude response metrics
        let claude_duration = claude_start.elapsed().as_secs_f64();
        metrics::record_claude_duration(claude_duration);
        metrics::record_claude_response_length(full_response.len());

        // Check if we got a response
        if full_response.is_empty() {
            metrics::record_error("no_response");
            room.send(MessageContent::plain(
                "Agent finished without a response. Please try again.",
            ))
            .await?;
            return Ok(HandleResult::Error("No response from agent".to_string()));
        }

        // Update session ID if it changed
        if let Some(ref new_session_id) = session_id_from_event {
            if let Err(e) = self
                .session_store
                .update_session_id(room.id(), new_session_id)
            {
                tracing::error!(error = %e, "Failed to update session ID");
            } else {
                // Also update in warm cache
                let mut session = session_handle.lock().await;
                session.set_session_id(new_session_id.clone());
            }
        }

        // Mark session as started
        self.session_store.mark_started(room.id())?;

        // Send response (strip function calls, convert to HTML, chunk if needed)
        let response = strip_function_calls(&full_response);
        self.send_response(room, &response).await?;

        Ok(HandleResult::Handled)
    }

    /// Handle session orphaned/invalid errors
    async fn handle_session_orphaned(
        &self,
        room: &I::Room,
        channel: &Channel,
        session_handle: &WarmSessionHandle,
    ) -> Result<HandleResult> {
        // Reset session in database
        if let Err(e) = self.session_store.reset_orphaned_session(room.id()) {
            tracing::error!(error = %e, "Failed to reset orphaned session");
        }

        // Mark session as invalidated
        {
            let mut session = session_handle.lock().await;
            session.set_invalidated(true);
        }

        // Evict from warm cache
        {
            let mut mgr = self.warm_manager.write().await;
            mgr.evict(&channel.channel_name);
        }

        metrics::record_error("session_orphaned");
        room.send(MessageContent::plain(
            "Session was reset (conversation data was lost). Please send your message again.",
        ))
        .await?;

        Ok(HandleResult::Error("Session orphaned".to_string()))
    }

    /// Send a response, chunking if necessary
    async fn send_response(&self, room: &I::Room, response: &str) -> Result<()> {
        if response.len() <= MAX_CHUNK_SIZE {
            let html = markdown_to_html(response);
            room.send(MessageContent::html(response, &html)).await?;
            metrics::record_message_sent();
        } else {
            let chunks = chunk_message(response, MAX_CHUNK_SIZE);
            for chunk in chunks {
                let html = markdown_to_html(&chunk);
                room.send(MessageContent::html(&chunk, &html)).await?;
                metrics::record_message_sent();
                // Small delay between chunks
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            }
        }
        Ok(())
    }

    /// Get help text
    fn help_text(&self) -> String {
        format!(
            "Available commands:\n\
            - !help - Show this help\n\
            - !status - Show channel status\n\
            - !reset - Reset Claude session\n\
            - !backend [list|set <type>] - Manage backend\n\
            \n\
            Or just type a message to chat with Claude.\n\
            \n\
            Prefix: {}",
            self.config.bot_prefix
        )
    }

    /// Get status for a room
    async fn get_status(&self, room: &I::Room) -> Result<String> {
        match self.session_store.get_by_room(room.id())? {
            Some(channel) => Ok(format!(
                "Channel: {}\nSession: {}\nBackend: {}\nDirectory: {}",
                channel.channel_name,
                channel.session_id,
                channel.backend_type.as_deref().unwrap_or("default"),
                channel.directory
            )),
            None => Ok("No channel configured for this room.".to_string()),
        }
    }

    /// Handle reset command
    async fn handle_reset(&self, room: &I::Room) -> Result<HandleResult> {
        if let Some(channel) = self.session_store.get_by_room(room.id())? {
            // Invalidate warm session
            {
                let mut mgr = self.warm_manager.write().await;
                mgr.invalidate_session(&channel.channel_name);
            }

            // Reset in database
            self.session_store.reset_orphaned_session(room.id())?;

            room.send(MessageContent::plain(
                "Session reset. Next message will start fresh.",
            ))
            .await?;
        } else {
            room.send(MessageContent::plain("No channel to reset."))
                .await?;
        }
        Ok(HandleResult::Handled)
    }

    /// Handle backend subcommands
    async fn handle_backend_command(
        &self,
        room: &I::Room,
        sub: BackendSubcommand,
    ) -> Result<HandleResult> {
        match sub {
            BackendSubcommand::Get => {
                if let Some(channel) = self.session_store.get_by_room(room.id())? {
                    let backend = channel.backend_type.as_deref().unwrap_or("default");
                    room.send(MessageContent::plain(format!(
                        "Current backend: {}",
                        backend
                    )))
                    .await?;
                } else {
                    room.send(MessageContent::plain("No channel configured."))
                        .await?;
                }
            }
            BackendSubcommand::List => {
                room.send(MessageContent::plain(
                    "Available backends: acp, mux, direct",
                ))
                .await?;
            }
            BackendSubcommand::Set { backend } => {
                if let Some(channel) = self.session_store.get_by_room(room.id())? {
                    self.session_store
                        .update_backend_type(&channel.channel_name, Some(&backend))?;
                    {
                        let mut mgr = self.warm_manager.write().await;
                        mgr.invalidate_session(&channel.channel_name);
                    }
                    room.send(MessageContent::plain(format!(
                        "Backend changed to: {}",
                        backend
                    )))
                    .await?;
                } else {
                    room.send(MessageContent::plain("No channel configured."))
                        .await?;
                }
            }
            BackendSubcommand::Reset => {
                if let Some(channel) = self.session_store.get_by_room(room.id())? {
                    self.session_store
                        .update_backend_type(&channel.channel_name, None)?;
                    {
                        let mut mgr = self.warm_manager.write().await;
                        mgr.invalidate_session(&channel.channel_name);
                    }
                    room.send(MessageContent::plain("Backend reset to default."))
                        .await?;
                } else {
                    room.send(MessageContent::plain("No channel configured."))
                        .await?;
                }
            }
        }
        Ok(HandleResult::Handled)
    }

    /// Check if debug mode is enabled for a channel directory
    fn is_debug_enabled(&self, channel_dir: &str) -> bool {
        let debug_path = std::path::Path::new(channel_dir)
            .join(".gorp")
            .join("enable-debug");
        debug_path.exists()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_as_standard_help() {
        let cmd = Command::new("help", vec![], "");
        assert_eq!(cmd.as_standard(), StandardCommand::Help);

        let cmd = Command::new("h", vec![], "");
        assert_eq!(cmd.as_standard(), StandardCommand::Help);
    }

    #[test]
    fn test_command_as_standard_status() {
        let cmd = Command::new("status", vec![], "");
        assert_eq!(cmd.as_standard(), StandardCommand::Status);

        let cmd = Command::new("s", vec![], "");
        assert_eq!(cmd.as_standard(), StandardCommand::Status);
    }

    #[test]
    fn test_command_as_standard_reset() {
        let cmd = Command::new("reset", vec![], "");
        assert_eq!(cmd.as_standard(), StandardCommand::Reset);

        let cmd = Command::new("r", vec![], "");
        assert_eq!(cmd.as_standard(), StandardCommand::Reset);
    }

    #[test]
    fn test_command_as_standard_backend_get() {
        let cmd = Command::new("backend", vec![], "");
        assert_eq!(
            cmd.as_standard(),
            StandardCommand::Backend(BackendSubcommand::Get)
        );

        let cmd = Command::new("backend", vec!["get".to_string()], "get");
        assert_eq!(
            cmd.as_standard(),
            StandardCommand::Backend(BackendSubcommand::Get)
        );
    }

    #[test]
    fn test_command_as_standard_backend_list() {
        let cmd = Command::new("backend", vec!["list".to_string()], "list");
        assert_eq!(
            cmd.as_standard(),
            StandardCommand::Backend(BackendSubcommand::List)
        );

        let cmd = Command::new("backend", vec!["ls".to_string()], "ls");
        assert_eq!(
            cmd.as_standard(),
            StandardCommand::Backend(BackendSubcommand::List)
        );
    }

    #[test]
    fn test_command_as_standard_backend_set() {
        let cmd = Command::new(
            "backend",
            vec!["set".to_string(), "mux".to_string()],
            "set mux",
        );
        assert_eq!(
            cmd.as_standard(),
            StandardCommand::Backend(BackendSubcommand::Set {
                backend: "mux".to_string()
            })
        );

        // Direct backend name (shorthand)
        let cmd = Command::new("backend", vec!["mux".to_string()], "mux");
        assert_eq!(
            cmd.as_standard(),
            StandardCommand::Backend(BackendSubcommand::Set {
                backend: "mux".to_string()
            })
        );
    }

    #[test]
    fn test_command_as_standard_backend_reset() {
        let cmd = Command::new("backend", vec!["reset".to_string()], "reset");
        assert_eq!(
            cmd.as_standard(),
            StandardCommand::Backend(BackendSubcommand::Reset)
        );

        let cmd = Command::new("backend", vec!["clear".to_string()], "clear");
        assert_eq!(
            cmd.as_standard(),
            StandardCommand::Backend(BackendSubcommand::Reset)
        );
    }

    #[test]
    fn test_command_as_standard_unknown() {
        let cmd = Command::new("foobar", vec![], "");
        assert_eq!(
            cmd.as_standard(),
            StandardCommand::Unknown("foobar".to_string())
        );
    }

    #[test]
    fn test_orchestrator_config_default() {
        let config = OrchestratorConfig::default();
        assert_eq!(config.bot_prefix, "!claude");
        assert!(config.allowed_users.is_empty());
        assert!(config.management_room.is_none());
        assert!(!config.debug_mode);
    }

    #[test]
    fn test_handle_result_debug() {
        let result = HandleResult::Handled;
        assert!(format!("{:?}", result).contains("Handled"));

        let result = HandleResult::Ignored;
        assert!(format!("{:?}", result).contains("Ignored"));

        let result = HandleResult::Error("test error".to_string());
        assert!(format!("{:?}", result).contains("Error"));
        assert!(format!("{:?}", result).contains("test error"));
    }
}
