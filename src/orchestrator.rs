// ABOUTME: Message bus orchestrator -- consumes inbound messages, routes to agent sessions or DISPATCH.
// ABOUTME: DISPATCH is a built-in command handler for session lifecycle and supervisor operations.

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
