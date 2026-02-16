// ABOUTME: Message bus orchestrator -- consumes inbound messages, routes to agent sessions or DISPATCH.
// ABOUTME: DISPATCH is a built-in command handler for session lifecycle and supervisor operations.

use std::collections::HashSet;
use std::sync::Arc;

use chrono::Utc;
use tokio::sync::Mutex;

use crate::bus::{BusMessage, BusResponse, MessageBus, ResponseContent, SessionTarget};

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

/// Maximum size of the dedup set before it gets cleared to prevent unbounded growth.
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
    seen_ids: Arc<Mutex<HashSet<String>>>,
}

impl Orchestrator {
    /// Create an Orchestrator wired to the given message bus.
    pub fn new(bus: Arc<MessageBus>) -> Self {
        Self {
            bus,
            seen_ids: Arc::new(Mutex::new(HashSet::new())),
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
                        let mut seen = self.seen_ids.lock().await;
                        if seen.contains(&msg.id) {
                            false
                        } else {
                            // Cap the dedup set to prevent unbounded growth
                            if seen.len() >= DEDUP_CAP {
                                seen.clear();
                            }
                            seen.insert(msg.id.clone());
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

    /// Handle a DISPATCH-targeted message by parsing and responding to the command.
    async fn handle_dispatch(&self, msg: BusMessage) {
        let cmd = DispatchCommand::parse(&msg.body);

        let response_text = match cmd {
            DispatchCommand::Help => self.help_text(),
            DispatchCommand::List => {
                // Stub — will be wired to SessionStore in Task 7
                "No active sessions. (Session store not yet wired)".to_string()
            }
            DispatchCommand::Unknown(text) => {
                format!(
                    "Unknown command: \"{}\". Type !help for a list of available commands.",
                    text
                )
            }
            // All other recognized commands are not yet wired
            _ => "Command recognized but not yet wired. (Will be connected in a future update.)".to_string(),
        };

        self.bus.publish_response(BusResponse {
            session_name: "DISPATCH".to_string(),
            content: ResponseContent::SystemNotice(response_text),
            timestamp: Utc::now(),
        });
    }

    /// Stub handler for messages targeting a named agent session.
    ///
    /// Publishes a SystemNotice indicating the session is not yet wired.
    /// Task 7 will replace this with actual WarmSessionManager integration.
    async fn handle_agent_message(&self, session_name: String, msg: BusMessage) {
        tracing::info!(session = %session_name, sender = %msg.sender, "Agent message (routing not yet wired)");
        self.bus.publish_response(BusResponse {
            session_name,
            content: ResponseContent::SystemNotice(
                "Agent session not yet wired. (Will be connected in a future update.)".to_string(),
            ),
            timestamp: Utc::now(),
        });
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
