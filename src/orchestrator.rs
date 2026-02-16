// ABOUTME: Message bus orchestrator -- consumes inbound messages, routes to agent sessions or DISPATCH.
// ABOUTME: DISPATCH is a built-in command handler for session lifecycle and supervisor operations.

use std::collections::{HashSet, VecDeque};
use std::sync::Arc;

use chrono::Utc;
use tokio::sync::Mutex;

use crate::bus::{BusMessage, BusResponse, MessageBus, MessageSource, ResponseContent, SessionTarget};
use gorp_core::session::SessionStore;
use gorp_core::warm_session::{prepare_session_async, send_prompt_with_handle, SharedWarmSessionManager};

/// DISPATCH commands parsed from message bodies.
///
/// These commands form the control plane for session management. Users send
/// them as `!command` messages in channels bound to DISPATCH (unmapped channels).
/// The Orchestrator's dispatch handler interprets these and acts on the session store.
#[derive(Debug, PartialEq)]
pub enum DispatchCommand {
    /// Create a named agent session, optionally in a specific workspace directory
    Create {
        name: String,
        workspace: Option<String>,
    },
    /// Delete an agent session by name
    Delete { name: String },
    /// List all active sessions
    List,
    /// Show status details for a named session
    Status { name: String },
    /// Bind the current platform channel to a named session
    Join { name: String },
    /// Unbind the current platform channel from its session
    Leave,
    /// Inject a message into another session from the current channel
    Tell { session: String, message: String },
    /// Read recent messages from a session's history
    Read {
        session: String,
        count: Option<usize>,
    },
    /// Send a message to all active sessions
    Broadcast { message: String },
    /// Show available DISPATCH commands
    Help,
    /// Unrecognized input (no `!` prefix, unknown `!` command, or missing required args)
    Unknown(String),
}

impl DispatchCommand {
    /// Parse a raw message body into a DispatchCommand.
    ///
    /// Commands must start with `!`. The command name is case-insensitive.
    /// Missing required arguments produce `Unknown`.
    pub fn parse(input: &str) -> Self {
        let input = input.trim();

        if !input.starts_with('!') {
            return Self::Unknown(input.to_string());
        }

        // Split into at most 3 parts: command, first_arg, rest
        let parts: Vec<&str> = input.splitn(3, ' ').collect();
        let cmd = parts[0].to_lowercase();

        match cmd.as_str() {
            "!create" => match parts.get(1) {
                Some(name) if !name.is_empty() => Self::Create {
                    name: name.to_string(),
                    workspace: parts.get(2).map(|s| s.to_string()),
                },
                _ => Self::Unknown(input.to_string()),
            },

            "!delete" => match parts.get(1) {
                Some(name) if !name.is_empty() => Self::Delete {
                    name: name.to_string(),
                },
                _ => Self::Unknown(input.to_string()),
            },

            "!list" => Self::List,

            "!status" => match parts.get(1) {
                Some(name) if !name.is_empty() => Self::Status {
                    name: name.to_string(),
                },
                _ => Self::Unknown(input.to_string()),
            },

            "!join" => match parts.get(1) {
                Some(name) if !name.is_empty() => Self::Join {
                    name: name.to_string(),
                },
                _ => Self::Unknown(input.to_string()),
            },

            "!leave" => Self::Leave,

            "!tell" => {
                let session = parts.get(1).map(|s| s.to_string());
                let message = parts.get(2).map(|s| s.to_string());
                match (session, message) {
                    (Some(session), Some(message)) if !session.is_empty() && !message.is_empty() => {
                        Self::Tell { session, message }
                    }
                    _ => Self::Unknown(input.to_string()),
                }
            }

            "!read" => match parts.get(1) {
                Some(session) if !session.is_empty() => {
                    let count = parts.get(2).and_then(|s| s.parse::<usize>().ok());
                    Self::Read {
                        session: session.to_string(),
                        count,
                    }
                }
                _ => Self::Unknown(input.to_string()),
            },

            "!broadcast" => {
                // Everything after "!broadcast " is the message
                let prefix = "!broadcast ";
                if input.len() > prefix.len() {
                    let message = input[prefix.len()..].to_string();
                    if message.is_empty() {
                        Self::Unknown(input.to_string())
                    } else {
                        Self::Broadcast { message }
                    }
                } else {
                    Self::Unknown(input.to_string())
                }
            }

            "!help" => Self::Help,

            _ => Self::Unknown(input.to_string()),
        }
    }
}

/// Maximum size of the dedup set before the oldest half is evicted.
const DEDUP_CAP: usize = 10_000;

/// Bus-based message orchestrator.
///
/// Subscribes to inbound messages on the `MessageBus`, deduplicates by message
/// ID, and routes each message to either the DISPATCH command handler or a named
/// agent session. One tokio task is spawned per message so sessions don't block
/// each other.
#[derive(Clone)]
pub struct Orchestrator {
    bus: Arc<MessageBus>,
    seen_ids: Arc<Mutex<(HashSet<String>, VecDeque<String>)>>,
    session_store: SessionStore,
    warm_manager: Option<SharedWarmSessionManager>,
}

impl Orchestrator {
    /// Create an Orchestrator wired to the given message bus, session store,
    /// and optional warm session manager for agent backends.
    pub fn new(
        bus: Arc<MessageBus>,
        session_store: SessionStore,
        warm_manager: Option<SharedWarmSessionManager>,
    ) -> Self {
        Self {
            bus,
            seen_ids: Arc::new(Mutex::new((HashSet::new(), VecDeque::new()))),
            session_store,
            warm_manager,
        }
    }

    /// Main run loop — subscribes to the bus and processes messages until the
    /// bus closes.
    pub async fn run(&self) {
        let mut rx = self.bus.subscribe_inbound();

        loop {
            match rx.recv().await {
                Ok(msg) => {
                    // Dedup: skip messages we've already seen
                    let is_new = {
                        let mut guard = self.seen_ids.lock().await;
                        let (ref mut set, ref mut order) = *guard;
                        if set.contains(&msg.id) {
                            false
                        } else {
                            // Evict the oldest half when we hit the cap
                            if set.len() >= DEDUP_CAP {
                                let evict_count = DEDUP_CAP / 2;
                                for _ in 0..evict_count {
                                    if let Some(old_id) = order.pop_front() {
                                        set.remove(&old_id);
                                    }
                                }
                            }
                            set.insert(msg.id.clone());
                            order.push_back(msg.id.clone());
                            true
                        }
                    };

                    if !is_new {
                        tracing::debug!(msg_id = %msg.id, "Skipping duplicate message");
                        continue;
                    }

                    // Spawn a task per message so sessions don't block each other
                    let orch = self.clone();
                    tokio::spawn(async move {
                        orch.handle(msg).await;
                    });
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!(skipped = n, "Orchestrator lagged on inbound bus, skipped messages");
                    continue;
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    tracing::info!("Inbound bus closed, orchestrator shutting down");
                    break;
                }
            }
        }
    }

    /// Route a single message to the appropriate handler.
    async fn handle(&self, msg: BusMessage) {
        match &msg.session_target {
            SessionTarget::Dispatch => {
                self.handle_dispatch(msg).await;
            }
            SessionTarget::Session { name } => {
                self.handle_agent_message(name.clone(), msg).await;
            }
        }
    }

    /// Extract (platform_id, channel_id) from a MessageSource.
    fn extract_source_ids(source: &MessageSource) -> (String, String) {
        match source {
            MessageSource::Platform {
                platform_id,
                channel_id,
            } => (platform_id.clone(), channel_id.clone()),
            MessageSource::Web { connection_id } => ("web".to_string(), connection_id.clone()),
            MessageSource::Api { token_hint } => ("api".to_string(), token_hint.clone()),
        }
    }

    /// Handle a DISPATCH-targeted message by parsing and responding to the command.
    async fn handle_dispatch(&self, msg: BusMessage) {
        let cmd = DispatchCommand::parse(&msg.body);

        let response_text = match cmd {
            DispatchCommand::Help => self.help_text(),

            DispatchCommand::Create { name, workspace: _ } => {
                // workspace arg parsed but not yet plumbed — SessionStore::create_channel
                // auto-generates directory from its workspace_path + channel_name
                let room_id = format!("bus:{}", name);
                match self.session_store.create_channel(&name, &room_id) {
                    Ok(channel) => format!(
                        "Session '{}' created (session_id: {})",
                        channel.channel_name, channel.session_id
                    ),
                    Err(e) => format!("Failed to create session '{}': {}", name, e),
                }
            }

            DispatchCommand::Delete { name } => {
                // Also unbind any channels bound to this session
                if let Ok(bindings) = self.session_store.list_bindings_for_session(&name) {
                    for (pid, cid) in bindings {
                        let _ = self.session_store.unbind_channel(&pid, &cid);
                        self.bus.unbind_channel_async(&pid, &cid).await;
                    }
                }
                match self.session_store.delete_channel(&name) {
                    Ok(()) => format!("Session '{}' deleted", name),
                    Err(e) => format!("Failed to delete session '{}': {}", name, e),
                }
            }

            DispatchCommand::List => {
                match self.session_store.list_all() {
                    Ok(channels) => {
                        if channels.is_empty() {
                            "No active sessions. Use !create <name> to create one.".to_string()
                        } else {
                            let mut lines = vec!["Active sessions:".to_string()];
                            for ch in &channels {
                                let started_marker = if ch.started { "*" } else { " " };
                                lines.push(format!(
                                    "  {} {} (session: {})",
                                    started_marker,
                                    ch.channel_name,
                                    &ch.session_id[..8.min(ch.session_id.len())]
                                ));
                            }
                            lines.push(format!(
                                "\n{} session(s) total. (* = started)",
                                channels.len()
                            ));
                            lines.join("\n")
                        }
                    }
                    Err(e) => format!("Failed to list sessions: {}", e),
                }
            }

            DispatchCommand::Status { name } => {
                match self.session_store.get_by_name(&name) {
                    Ok(Some(ch)) => {
                        let bindings = self
                            .session_store
                            .list_bindings_for_session(&name)
                            .unwrap_or_default();
                        let binding_lines: Vec<String> = bindings
                            .iter()
                            .map(|(pid, cid)| format!("    {}:{}", pid, cid))
                            .collect();
                        format!(
                            "Session '{}':\n  Session ID: {}\n  Directory: {}\n  Started: {}\n  Created: {}\n  Bindings:\n{}",
                            ch.channel_name,
                            ch.session_id,
                            ch.directory,
                            ch.started,
                            ch.created_at,
                            if binding_lines.is_empty() {
                                "    (none)".to_string()
                            } else {
                                binding_lines.join("\n")
                            }
                        )
                    }
                    Ok(None) => format!("Session '{}' not found", name),
                    Err(e) => format!("Failed to get status for '{}': {}", name, e),
                }
            }

            DispatchCommand::Join { name } => {
                match self.session_store.get_by_name(&name) {
                    Ok(Some(_)) => {
                        let (platform_id, channel_id) = Self::extract_source_ids(&msg.source);
                        // Bind in both bus (in-memory) and store (persistent)
                        self.bus
                            .bind_channel_async(&platform_id, &channel_id, &name)
                            .await;
                        match self
                            .session_store
                            .bind_channel(&platform_id, &channel_id, &name)
                        {
                            Ok(()) => format!(
                                "Channel bound to session '{}'. Messages here will now route to this agent.",
                                name
                            ),
                            Err(e) => format!("Failed to bind channel: {}", e),
                        }
                    }
                    Ok(None) => format!(
                        "Session '{}' not found. Create it first with !create {}",
                        name, name
                    ),
                    Err(e) => format!("Error: {}", e),
                }
            }

            DispatchCommand::Leave => {
                let (platform_id, channel_id) = Self::extract_source_ids(&msg.source);
                self.bus
                    .unbind_channel_async(&platform_id, &channel_id)
                    .await;
                match self
                    .session_store
                    .unbind_channel(&platform_id, &channel_id)
                {
                    Ok(()) => {
                        "Channel unbound. Messages here will now route to DISPATCH.".to_string()
                    }
                    Err(e) => format!("Failed to unbind channel: {}", e),
                }
            }

            DispatchCommand::Tell { session, message } => {
                match self.session_store.get_by_name(&session) {
                    Ok(Some(_)) => {
                        // Publish a new BusMessage targeting the session
                        self.bus.publish_inbound(BusMessage {
                            id: format!("{}-tell", msg.id),
                            source: msg.source.clone(),
                            session_target: SessionTarget::Session {
                                name: session.clone(),
                            },
                            sender: msg.sender.clone(),
                            body: message,
                            timestamp: Utc::now(),
                        });
                        format!("Message sent to session '{}'", session)
                    }
                    Ok(None) => format!("Session '{}' not found", session),
                    Err(e) => format!("Error: {}", e),
                }
            }

            DispatchCommand::Read {
                session,
                count: _,
            } => match self.session_store.get_by_name(&session) {
                Ok(Some(ch)) => {
                    format!(
                        "Session '{}': session_id={}, started={}, created_at={}\n(Message history not yet available)",
                        ch.channel_name, ch.session_id, ch.started, ch.created_at
                    )
                }
                Ok(None) => format!("Session '{}' not found", session),
                Err(e) => format!("Error: {}", e),
            },

            DispatchCommand::Broadcast { message } => {
                match self.session_store.list_all() {
                    Ok(channels) => {
                        let mut sent_count = 0;
                        for ch in channels {
                            if !ch.is_dispatch_room {
                                self.bus.publish_inbound(BusMessage {
                                    id: format!("{}-bc-{}", msg.id, ch.channel_name),
                                    source: msg.source.clone(),
                                    session_target: SessionTarget::Session {
                                        name: ch.channel_name.clone(),
                                    },
                                    sender: msg.sender.clone(),
                                    body: message.clone(),
                                    timestamp: Utc::now(),
                                });
                                sent_count += 1;
                            }
                        }
                        format!("Message broadcast to {} session(s)", sent_count)
                    }
                    Err(e) => format!("Failed to broadcast: {}", e),
                }
            }

            DispatchCommand::Unknown(text) => {
                format!(
                    "Unknown command: \"{}\". Type !help for a list of available commands.",
                    text
                )
            }
        };

        self.bus.publish_response(BusResponse {
            session_name: "DISPATCH".to_string(),
            content: ResponseContent::SystemNotice(response_text),
            timestamp: Utc::now(),
        });
    }

    /// Handle messages targeting a named agent session by routing to the WarmSessionManager.
    ///
    /// If no WarmSessionManager is configured (warm_manager is None), publishes an error
    /// response indicating no agent backend is available.
    async fn handle_agent_message(&self, session_name: String, msg: BusMessage) {
        tracing::info!(session = %session_name, sender = %msg.sender, "Routing to agent session");

        let warm_manager = match &self.warm_manager {
            Some(wm) => wm.clone(),
            None => {
                self.bus.publish_response(BusResponse {
                    session_name,
                    content: ResponseContent::Error(
                        "No agent backend configured".to_string(),
                    ),
                    timestamp: Utc::now(),
                });
                return;
            }
        };

        // Look up the channel in the session store
        let channel = match self.session_store.get_by_name(&session_name) {
            Ok(Some(ch)) => ch,
            Ok(None) => {
                let err_msg = format!("Session '{}' not found in store", session_name);
                self.bus.publish_response(BusResponse {
                    session_name,
                    content: ResponseContent::Error(err_msg),
                    timestamp: Utc::now(),
                });
                return;
            }
            Err(e) => {
                let err_msg = format!("Store error: {}", e);
                self.bus.publish_response(BusResponse {
                    session_name,
                    content: ResponseContent::Error(err_msg),
                    timestamp: Utc::now(),
                });
                return;
            }
        };

        // Prepare warm session
        let (handle, session_id, is_new) = match prepare_session_async(&warm_manager, &channel).await {
            Ok(result) => result,
            Err(e) => {
                self.bus.publish_response(BusResponse {
                    session_name,
                    content: ResponseContent::Error(format!(
                        "Failed to prepare session: {}",
                        e
                    )),
                    timestamp: Utc::now(),
                });
                return;
            }
        };

        // Update session ID in store if it changed
        if is_new {
            if let Err(e) = self.session_store.update_session_id(&channel.room_id, &session_id) {
                tracing::error!(error = %e, "Failed to update session ID in store");
            }
        }

        // Send prompt and stream response
        match send_prompt_with_handle(&handle, &session_id, &msg.body).await {
            Ok(mut receiver) => {
                let mut response_text = String::new();

                while let Some(event) = receiver.recv().await {
                    match event {
                        gorp_agent::AgentEvent::Text(text) => {
                            response_text.push_str(&text);
                            self.bus.publish_response(BusResponse {
                                session_name: session_name.clone(),
                                content: ResponseContent::Chunk(text),
                                timestamp: Utc::now(),
                            });
                        }
                        gorp_agent::AgentEvent::Result { text, .. } => {
                            if response_text.is_empty() {
                                response_text = text.clone();
                            }
                            break;
                        }
                        gorp_agent::AgentEvent::Error { message, .. } => {
                            self.bus.publish_response(BusResponse {
                                session_name: session_name.clone(),
                                content: ResponseContent::Error(message),
                                timestamp: Utc::now(),
                            });
                            return;
                        }
                        gorp_agent::AgentEvent::SessionChanged { new_session_id } => {
                            if let Err(e) = self.session_store.update_session_id(
                                &channel.room_id,
                                &new_session_id,
                            ) {
                                tracing::error!(error = %e, "Failed to update changed session ID");
                            }
                            let mut session = handle.lock().await;
                            session.set_session_id(new_session_id);
                        }
                        _ => {} // Ignore tool events etc. for now
                    }
                }

                // Publish complete response
                self.bus.publish_response(BusResponse {
                    session_name: session_name.clone(),
                    content: ResponseContent::Complete(response_text),
                    timestamp: Utc::now(),
                });

                // Mark session as started
                if let Err(e) = self.session_store.mark_started(&channel.room_id) {
                    tracing::error!(error = %e, "Failed to mark session started");
                }
            }
            Err(e) => {
                self.bus.publish_response(BusResponse {
                    session_name,
                    content: ResponseContent::Error(format!(
                        "Failed to send prompt: {}",
                        e
                    )),
                    timestamp: Utc::now(),
                });
            }
        }
    }

    /// Build the help text listing all available DISPATCH commands.
    fn help_text(&self) -> String {
        "DISPATCH commands:\n\
         \n\
         !create <name> [workspace]  — Create a named agent session\n\
         !delete <name>              — Delete an agent session\n\
         !list                       — List all active sessions\n\
         !status <name>              — Show session status details\n\
         !join <name>                — Bind this channel to a session\n\
         !leave                      — Unbind this channel from its session\n\
         !tell <session> <message>   — Inject a message into another session\n\
         !read <session> [count]     — Read recent messages from a session\n\
         !broadcast <message>        — Send a message to all active sessions\n\
         !help                       — Show this help"
            .to_string()
    }
}
