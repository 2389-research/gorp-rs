// ABOUTME: Slash command registration and handling for the Slack platform
// ABOUTME: Provides /gorp command that routes user input to the message handler

use anyhow::Result;
use async_trait::async_trait;
use gorp_core::traits::{MessageContent, SlashCommandDef, SlashCommandInvocation, SlashCommandProvider};

/// Slash command handler for the Slack platform.
///
/// Registered commands are declared here; actual execution is handled
/// by converting the command invocation into an IncomingMessage and
/// routing it through the standard message handler pipeline.
pub struct SlackCommandHandler;

impl SlackCommandHandler {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl SlashCommandProvider for SlackCommandHandler {
    fn registered_commands(&self) -> Vec<SlashCommandDef> {
        vec![
            SlashCommandDef {
                name: "/gorp".to_string(),
                description: "Talk to gorp — your AI assistant".to_string(),
            },
            SlashCommandDef {
                name: "/gorp-status".to_string(),
                description: "Check gorp platform connection status".to_string(),
            },
        ]
    }

    async fn handle_command(&self, cmd: SlashCommandInvocation) -> Result<MessageContent> {
        match cmd.command.as_str() {
            "/gorp" => {
                // The actual processing happens asynchronously via the event stream.
                // This returns the immediate ACK response shown to the user.
                Ok(MessageContent::plain("Working on it..."))
            }
            "/gorp-status" => {
                Ok(MessageContent::plain("Checking status..."))
            }
            _ => {
                Ok(MessageContent::plain(format!(
                    "Unknown command: {}",
                    cmd.command
                )))
            }
        }
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registered_commands() {
        let handler = SlackCommandHandler::new();
        let commands = handler.registered_commands();
        assert_eq!(commands.len(), 2);
        assert_eq!(commands[0].name, "/gorp");
        assert_eq!(commands[1].name, "/gorp-status");
    }

    #[tokio::test]
    async fn test_handle_gorp_command() {
        let handler = SlackCommandHandler::new();
        let cmd = SlashCommandInvocation {
            command: "/gorp".to_string(),
            text: "hello".to_string(),
            channel_id: "C12345".to_string(),
            user_id: "U67890".to_string(),
            response_url: "https://hooks.slack.com/commands/T1/2/abc".to_string(),
        };
        let result = handler.handle_command(cmd).await.unwrap();
        assert!(matches!(result, MessageContent::Plain(text) if text.contains("Working")));
    }

    #[tokio::test]
    async fn test_handle_status_command() {
        let handler = SlackCommandHandler::new();
        let cmd = SlashCommandInvocation {
            command: "/gorp-status".to_string(),
            text: String::new(),
            channel_id: "C12345".to_string(),
            user_id: "U67890".to_string(),
            response_url: "https://hooks.slack.com/commands/T1/2/abc".to_string(),
        };
        let result = handler.handle_command(cmd).await.unwrap();
        assert!(matches!(result, MessageContent::Plain(text) if text.contains("status")));
    }

    #[tokio::test]
    async fn test_handle_unknown_command() {
        let handler = SlackCommandHandler::new();
        let cmd = SlashCommandInvocation {
            command: "/unknown".to_string(),
            text: String::new(),
            channel_id: "C12345".to_string(),
            user_id: "U67890".to_string(),
            response_url: "https://hooks.slack.com/commands/T1/2/abc".to_string(),
        };
        let result = handler.handle_command(cmd).await.unwrap();
        assert!(matches!(result, MessageContent::Plain(text) if text.contains("Unknown")));
    }

    #[test]
    fn test_command_handler_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<SlackCommandHandler>();
    }
}
