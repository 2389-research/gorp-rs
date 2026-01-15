// ABOUTME: Command handler for Matrix bot commands
// ABOUTME: Processes !help, !create, !status, etc. using ChatChannel trait for testability

use anyhow::Result;
use gorp_core::traits::{ChatChannel, MessageContent};
use matrix_sdk::Client;

use crate::{
    commands::Command,
    config::Config,
    metrics,
    scheduler::SchedulerStore,
    session::SessionStore,
    utils::markdown_to_html,
    warm_session::SharedWarmSessionManager,
};

use super::helpers::is_debug_enabled;

/// Help documentation loaded at compile time
const HELP_MD: &str = include_str!("../../docs/HELP.md");
/// Message of the day shown on boot
const MOTD_MD: &str = include_str!("../../docs/MOTD.md");
/// Changelog documentation
const CHANGELOG_MD: &str = include_str!("../../docs/CHANGELOG.md");

/// Handle a parsed command
///
/// This function is designed to be testable - it takes a ChatChannel trait
/// instead of a concrete Room, allowing mock implementations for testing.
/// The client parameter is optional since it's only needed for delegated commands.
#[allow(clippy::too_many_arguments)]
pub async fn handle_command<C: ChatChannel>(
    channel: &C,
    cmd: &Command,
    session_store: &SessionStore,
    _scheduler_store: &SchedulerStore,
    _client: Option<&Client>,
    _sender: &str,
    is_dm: bool,
    config: &Config,
    warm_manager: &SharedWarmSessionManager,
) -> Result<()> {
    let command = cmd.name.as_str();
    let command_parts: Vec<&str> = std::iter::once(command)
        .chain(cmd.args.iter().map(|s| s.as_str()))
        .collect();

    if command.is_empty() {
        let help_msg = if is_dm {
            "💬 Orchestrator Commands:\n\
            !create <name> - Create new channel\n\
            !join <name> - Get invited to a channel\n\
            !delete <name> - Remove channel (keeps workspace)\n\
            !reset <name> - Reset channel session remotely\n\
            !cleanup - Leave orphaned rooms\n\
            !restore-rooms - Restore channels from workspace directories\n\
            !list - Show all channels\n\
            !help - Show detailed help"
        } else {
            "Available commands:\n\
            !help - Show detailed help\n\
            !status - Show current channel info\n\
            !backend - View/change backend for this channel\n\
            !debug - Toggle tool usage display\n\
            !leave - Bot leaves this room"
        };
        channel.send(MessageContent::plain(help_msg)).await?;
        return Ok(());
    }

    metrics::record_command(command);

    match command {
        "help" => {
            let help_html = markdown_to_html(HELP_MD);
            channel.send(MessageContent::html(HELP_MD, &help_html)).await?;
        }
        "changelog" => {
            let changelog_html = markdown_to_html(CHANGELOG_MD);
            channel.send(MessageContent::html(CHANGELOG_MD, &changelog_html)).await?;
        }
        "motd" => {
            let motd_html = markdown_to_html(MOTD_MD);
            channel.send(MessageContent::html(MOTD_MD, &motd_html)).await?;
        }
        "status" => {
            if let Some(ch) = session_store.get_by_room(channel.id())? {
                let debug_status = if is_debug_enabled(&ch.directory) {
                    "🔧 Enabled (tool usage shown)"
                } else {
                    "🔇 Disabled (tool usage hidden)"
                };
                let backend_display = ch
                    .backend_type
                    .as_deref()
                    .unwrap_or(&config.backend.backend_type);
                let status = format!(
                    "📊 Channel Status\n\n\
                    Channel: {}\n\
                    Session ID: {}\n\
                    Directory: {}\n\
                    Backend: {}\n\
                    Started: {}\n\
                    Debug Mode: {}\n\n\
                    Webhook URL:\n\
                    POST http://{}:{}/webhook/session/{}\n\n\
                    This room is backed by a persistent Claude session.",
                    ch.channel_name,
                    ch.session_id,
                    ch.directory,
                    backend_display,
                    if ch.started {
                        "Yes"
                    } else {
                        "No (first message will start it)"
                    },
                    debug_status,
                    config.webhook.host,
                    config.webhook.port,
                    ch.session_id
                );
                channel.send(MessageContent::plain(&status)).await?;
            } else {
                channel.send(MessageContent::plain(
                    "📊 Channel Status\n\n\
                    No channel attached.\n\n\
                    DM me to create one: !create <name>",
                ))
                .await?;
            }
        }
        "list" => {
            if !is_dm {
                channel.send(MessageContent::plain("❌ The !list command only works in DMs.")).await?;
                return Ok(());
            }

            let channels = session_store.list_all()?;
            if channels.is_empty() {
                channel.send(MessageContent::plain(
                    "📋 No channels yet.\n\nCreate one with: !create <name>",
                ))
                .await?;
            } else {
                let mut msg = String::from("📋 Channels:\n\n");
                for ch in &channels {
                    let status = if ch.started { "🟢" } else { "⚪" };
                    msg.push_str(&format!("{} {} - {}\n", status, ch.channel_name, ch.directory));
                }
                msg.push_str("\nUse !join <name> to get invited to a channel.");
                channel.send(MessageContent::plain(&msg)).await?;
            }
        }
        "debug" => {
            if is_dm {
                channel.send(MessageContent::plain("❌ The !debug command only works in channel rooms.")).await?;
                return Ok(());
            }

            let Some(ch) = session_store.get_by_room(channel.id())? else {
                channel.send(MessageContent::plain("No channel attached to this room.")).await?;
                return Ok(());
            };

            let channel_path = std::path::Path::new(&ch.directory);
            let debug_dir = channel_path.join(".gorp");
            let debug_file = debug_dir.join("enable-debug");

            let subcommand = command_parts.get(1).map(|s| s.to_lowercase());
            match subcommand.as_deref() {
                Some("on") | Some("enable") => {
                    if let Err(e) = std::fs::create_dir_all(&debug_dir) {
                        channel.send(MessageContent::plain(&format!("⚠️ Failed to create debug directory: {}", e))).await?;
                        return Ok(());
                    }
                    if let Err(e) = std::fs::write(&debug_file, "") {
                        channel.send(MessageContent::plain(&format!("⚠️ Failed to enable debug: {}", e))).await?;
                        return Ok(());
                    }
                    channel.send(MessageContent::plain(
                        "🔧 Debug mode ENABLED\n\nTool usage will now be shown in this channel.",
                    ))
                    .await?;
                    tracing::info!(channel = %ch.channel_name, "Debug mode enabled");
                }
                Some("off") | Some("disable") => {
                    if debug_file.exists() {
                        if let Err(e) = std::fs::remove_file(&debug_file) {
                            channel.send(MessageContent::plain(&format!("⚠️ Failed to disable debug: {}", e))).await?;
                            return Ok(());
                        }
                    }
                    channel.send(MessageContent::plain(
                        "🔇 Debug mode DISABLED\n\nTool usage will be hidden in this channel.",
                    ))
                    .await?;
                    tracing::info!(channel = %ch.channel_name, "Debug mode disabled");
                }
                _ => {
                    let status = if debug_file.exists() {
                        "🔧 Debug mode is ENABLED\n\nTool usage is shown in this channel."
                    } else {
                        "🔇 Debug mode is DISABLED\n\nTool usage is hidden in this channel."
                    };
                    channel.send(MessageContent::plain(&format!(
                        "{}\n\nCommands:\n  !debug on - Show tool usage\n  !debug off - Hide tool usage",
                        status
                    )))
                    .await?;
                }
            }
        }
        "backend" => {
            if is_dm {
                channel.send(MessageContent::plain("❌ The !backend command only works in channel rooms.")).await?;
                return Ok(());
            }

            let Some(ch) = session_store.get_by_room(channel.id())? else {
                channel.send(MessageContent::plain("No channel attached to this room.")).await?;
                return Ok(());
            };

            let subcommand = command_parts.get(1).map(|s| s.to_lowercase());
            match subcommand.as_deref() {
                Some("list") => {
                    let available = "acp, mux, direct";
                    let current = ch
                        .backend_type
                        .as_deref()
                        .unwrap_or("(global default)");
                    channel.send(MessageContent::plain(&format!(
                        "📋 Available Backends\n\n\
                        Current: {}\n\
                        Available: {}\n\n\
                        Use `!backend set <name>` to change.",
                        current, available
                    )))
                    .await?;
                }
                Some("set") => {
                    let Some(new_backend) = command_parts.get(2) else {
                        channel.send(MessageContent::plain(
                            "Usage: !backend set <name>\n\n\
                            Available: acp, mux, direct\n\n\
                            Example: !backend set mux",
                        ))
                        .await?;
                        return Ok(());
                    };

                    let new_backend = new_backend.to_lowercase();
                    let valid_backends = ["acp", "mux", "direct"];
                    if !valid_backends.contains(&new_backend.as_str()) {
                        channel.send(MessageContent::plain(&format!(
                            "❌ Unknown backend: {}\n\nAvailable: {}",
                            new_backend,
                            valid_backends.join(", ")
                        )))
                        .await?;
                        return Ok(());
                    }

                    session_store.update_backend_type(&ch.channel_name, Some(&new_backend))?;
                    {
                        let mut mgr = warm_manager.write().await;
                        mgr.invalidate_session(&ch.channel_name);
                    }

                    channel.send(MessageContent::plain(&format!(
                        "✅ Backend changed to: {}\n\nSession has been reset. Next message will use the new backend.",
                        new_backend
                    )))
                    .await?;

                    tracing::info!(
                        channel = %ch.channel_name,
                        backend = %new_backend,
                        "Backend changed via command"
                    );
                }
                Some("reset") | Some("default") => {
                    session_store.update_backend_type(&ch.channel_name, None)?;
                    {
                        let mut mgr = warm_manager.write().await;
                        mgr.invalidate_session(&ch.channel_name);
                    }

                    channel.send(MessageContent::plain(
                        "✅ Backend reset to global default.\n\nSession has been reset. Next message will use the default backend.",
                    ))
                    .await?;

                    tracing::info!(
                        channel = %ch.channel_name,
                        "Backend reset to default via command"
                    );
                }
                _ => {
                    let current = ch
                        .backend_type
                        .as_deref()
                        .unwrap_or("(global default)");
                    let global_default = &config.backend.backend_type;
                    channel.send(MessageContent::plain(&format!(
                        "🔌 Backend Status\n\n\
                        Channel backend: {}\n\
                        Global default: {}\n\n\
                        Commands:\n  \
                        !backend list - Show available backends\n  \
                        !backend set <name> - Change backend\n  \
                        !backend reset - Use global default",
                        current, global_default
                    )))
                    .await?;
                }
            }
        }
        // Commands that need Matrix client operations are delegated back
        // For now, return a placeholder - these will be handled in mod.rs
        "create" | "join" | "delete" | "leave" | "cleanup" | "restore-rooms" | "setup" | "schedule" | "reset" => {
            // These commands need the Matrix client for room operations
            // or have more complete implementations in matrix_commands.rs
            // Reset is delegated to ensure consistent use of reset_session (which resets started flag)
            return Err(anyhow::anyhow!("DELEGATE_TO_MATRIX:{}", command));
        }
        _ => {
            let help_msg = if is_dm {
                "Unknown command. Available commands:\n\
                !create <name> - Create new channel\n\
                !join <name> - Get invited to channel\n\
                !delete <name> - Remove channel\n\
                !reset <name> - Reset channel session remotely\n\
                !cleanup - Leave orphaned rooms\n\
                !restore-rooms - Restore channels from workspace\n\
                !list - Show all channels\n\
                !help - Show detailed help"
            } else {
                "Unknown command. Available commands:\n\
                !status - Show channel info\n\
                !debug - Toggle tool usage display\n\
                !reset - Reset Claude session (reload MCP tools)\n\
                !schedule <time> <prompt> - Schedule a prompt\n\
                !schedule list - View schedules\n\
                !schedule export/import - Backup/restore schedules\n\
                !leave - Bot leaves room\n\
                !help - Show detailed help"
            };
            channel.send(MessageContent::plain(help_msg)).await?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message_handler::traits::MockChannel;
    use crate::scheduler::SchedulerStore;
    use crate::session::SessionStore;
    use crate::warm_session::create_shared_manager;
    use gorp_core::config::{
        BackendConfig, MatrixConfig, SchedulerConfig, WebhookConfig, WorkspaceConfig,
    };
    use gorp_core::warm_session::WarmConfig;
    use std::time::Duration;
    use tempfile::TempDir;

    fn make_command(name: &str, args: Vec<&str>) -> Command {
        let raw_args = args.join(" ");
        Command {
            name: name.to_string(),
            args: args.into_iter().map(String::from).collect(),
            raw_args,
        }
    }

    fn make_test_config(workspace_path: &str) -> Config {
        Config {
            matrix: MatrixConfig {
                home_server: "https://matrix.example.com".to_string(),
                user_id: "@bot:matrix.example.com".to_string(),
                password: None,
                access_token: Some("test_token".to_string()),
                device_name: "test-device".to_string(),
                allowed_users: vec!["@user:matrix.example.com".to_string()],
                room_prefix: "Test".to_string(),
                recovery_key: None,
            },
            backend: BackendConfig::default(),
            webhook: WebhookConfig {
                port: 13000,
                api_key: None,
                host: "localhost".to_string(),
            },
            workspace: WorkspaceConfig {
                path: workspace_path.to_string(),
            },
            scheduler: SchedulerConfig {
                timezone: "UTC".to_string(),
            },
        }
    }

    struct TestContext {
        _temp_dir: TempDir,
        session_store: SessionStore,
        scheduler_store: SchedulerStore,
        config: Config,
        warm_manager: SharedWarmSessionManager,
    }

    impl TestContext {
        fn new() -> Self {
            let temp_dir = TempDir::new().unwrap();
            let workspace_path = temp_dir.path().to_str().unwrap();
            let session_store = SessionStore::new(temp_dir.path()).unwrap();
            let scheduler_store = SchedulerStore::new(session_store.db_connection());
            let config = make_test_config(workspace_path);
            let warm_config = WarmConfig {
                keep_alive_duration: Duration::from_secs(60),
                pre_warm_lead_time: Duration::from_secs(30),
                agent_binary: "claude".to_string(),
                backend_type: "acp".to_string(),
                model: None,
                max_tokens: None,
                global_system_prompt_path: None,
                mcp_servers: vec![],
            };
            let warm_manager = create_shared_manager(warm_config);

            Self {
                _temp_dir: temp_dir,
                session_store,
                scheduler_store,
                config,
                warm_manager,
            }
        }

        fn create_channel(&self, name: &str, room_id: &str) {
            self.session_store.create_channel(name, room_id).unwrap();
        }
    }

    // =========================================================================
    // Empty Command Tests
    // =========================================================================

    #[tokio::test]
    async fn test_empty_command_shows_dm_help() {
        let ctx = TestContext::new();
        let room = MockChannel::dm("!test:matrix.org");
        let cmd = make_command("", vec![]);

        let result = handle_command(
            &room,
            &cmd,
            &ctx.session_store,
            &ctx.scheduler_store,
            None, // Client not needed for these tests
            "@user:matrix.org",
            true, // is_dm
            &ctx.config,
            &ctx.warm_manager,
        )
        .await;

        assert!(result.is_ok());
        assert!(room.has_message_containing("Orchestrator Commands"));
        assert!(room.has_message_containing("!create"));
    }

    #[tokio::test]
    async fn test_empty_command_shows_channel_help() {
        let ctx = TestContext::new();
        let room = MockChannel::new("!channel:matrix.org");
        let cmd = make_command("", vec![]);

        let result = handle_command(
            &room,
            &cmd,
            &ctx.session_store,
            &ctx.scheduler_store,
            None,
            "@user:matrix.org",
            false, // is_dm
            &ctx.config,
            &ctx.warm_manager,
        )
        .await;

        assert!(result.is_ok());
        assert!(room.has_message_containing("Available commands"));
        assert!(room.has_message_containing("!help"));
    }

    // =========================================================================
    // Help/Changelog/MOTD Tests
    // =========================================================================

    #[tokio::test]
    async fn test_help_command() {
        let ctx = TestContext::new();
        let room = MockChannel::new("!channel:matrix.org");
        let cmd = make_command("help", vec![]);

        let result = handle_command(
            &room,
            &cmd,
            &ctx.session_store,
            &ctx.scheduler_store,
            None,
            "@user:matrix.org",
            false,
            &ctx.config,
            &ctx.warm_manager,
        )
        .await;

        assert!(result.is_ok());
        // Help is sent as HTML, check the message was sent
        assert_eq!(room.get_messages().len(), 1);
    }

    #[tokio::test]
    async fn test_changelog_command() {
        let ctx = TestContext::new();
        let room = MockChannel::new("!channel:matrix.org");
        let cmd = make_command("changelog", vec![]);

        let result = handle_command(
            &room,
            &cmd,
            &ctx.session_store,
            &ctx.scheduler_store,
            None,
            "@user:matrix.org",
            false,
            &ctx.config,
            &ctx.warm_manager,
        )
        .await;

        assert!(result.is_ok());
        assert_eq!(room.get_messages().len(), 1);
    }

    #[tokio::test]
    async fn test_motd_command() {
        let ctx = TestContext::new();
        let room = MockChannel::new("!channel:matrix.org");
        let cmd = make_command("motd", vec![]);

        let result = handle_command(
            &room,
            &cmd,
            &ctx.session_store,
            &ctx.scheduler_store,
            None,
            "@user:matrix.org",
            false,
            &ctx.config,
            &ctx.warm_manager,
        )
        .await;

        assert!(result.is_ok());
        assert_eq!(room.get_messages().len(), 1);
    }

    // =========================================================================
    // Status Command Tests
    // =========================================================================

    #[tokio::test]
    async fn test_status_with_channel() {
        let ctx = TestContext::new();
        let room = MockChannel::new("!channel:matrix.org");
        ctx.create_channel("test-channel", "!channel:matrix.org");
        let cmd = make_command("status", vec![]);

        let result = handle_command(
            &room,
            &cmd,
            &ctx.session_store,
            &ctx.scheduler_store,
            None,
            "@user:matrix.org",
            false,
            &ctx.config,
            &ctx.warm_manager,
        )
        .await;

        assert!(result.is_ok());
        assert!(room.has_message_containing("Channel Status"));
        assert!(room.has_message_containing("test-channel"));
        assert!(room.has_message_containing("Webhook URL"));
    }

    #[tokio::test]
    async fn test_status_without_channel() {
        let ctx = TestContext::new();
        let room = MockChannel::new("!channel:matrix.org");
        let cmd = make_command("status", vec![]);

        let result = handle_command(
            &room,
            &cmd,
            &ctx.session_store,
            &ctx.scheduler_store,
            None,
            "@user:matrix.org",
            false,
            &ctx.config,
            &ctx.warm_manager,
        )
        .await;

        assert!(result.is_ok());
        assert!(room.has_message_containing("No channel attached"));
    }

    // =========================================================================
    // List Command Tests
    // =========================================================================

    #[tokio::test]
    async fn test_list_empty_dm() {
        let ctx = TestContext::new();
        let room = MockChannel::dm("!dm:matrix.org");
        let cmd = make_command("list", vec![]);

        let result = handle_command(
            &room,
            &cmd,
            &ctx.session_store,
            &ctx.scheduler_store,
            None,
            "@user:matrix.org",
            true, // is_dm
            &ctx.config,
            &ctx.warm_manager,
        )
        .await;

        assert!(result.is_ok());
        assert!(room.has_message_containing("No channels yet"));
    }

    #[tokio::test]
    async fn test_list_with_channels() {
        let ctx = TestContext::new();
        let room = MockChannel::dm("!dm:matrix.org");
        ctx.create_channel("project-a", "!room1:matrix.org");
        ctx.create_channel("project-b", "!room2:matrix.org");
        let cmd = make_command("list", vec![]);

        let result = handle_command(
            &room,
            &cmd,
            &ctx.session_store,
            &ctx.scheduler_store,
            None,
            "@user:matrix.org",
            true,
            &ctx.config,
            &ctx.warm_manager,
        )
        .await;

        assert!(result.is_ok());
        assert!(room.has_message_containing("Channels"));
        assert!(room.has_message_containing("project-a"));
        assert!(room.has_message_containing("project-b"));
    }

    #[tokio::test]
    async fn test_list_rejected_in_channel() {
        let ctx = TestContext::new();
        let room = MockChannel::new("!channel:matrix.org");
        let cmd = make_command("list", vec![]);

        let result = handle_command(
            &room,
            &cmd,
            &ctx.session_store,
            &ctx.scheduler_store,
            None,
            "@user:matrix.org",
            false, // NOT a dm
            &ctx.config,
            &ctx.warm_manager,
        )
        .await;

        assert!(result.is_ok());
        assert!(room.has_message_containing("only works in DMs"));
    }

    // =========================================================================
    // Debug Command Tests
    // =========================================================================

    #[tokio::test]
    async fn test_debug_rejected_in_dm() {
        let ctx = TestContext::new();
        let room = MockChannel::dm("!dm:matrix.org");
        let cmd = make_command("debug", vec![]);

        let result = handle_command(
            &room,
            &cmd,
            &ctx.session_store,
            &ctx.scheduler_store,
            None,
            "@user:matrix.org",
            true, // is_dm
            &ctx.config,
            &ctx.warm_manager,
        )
        .await;

        assert!(result.is_ok());
        assert!(room.has_message_containing("only works in channel rooms"));
    }

    #[tokio::test]
    async fn test_debug_no_channel() {
        let ctx = TestContext::new();
        let room = MockChannel::new("!channel:matrix.org");
        let cmd = make_command("debug", vec![]);

        let result = handle_command(
            &room,
            &cmd,
            &ctx.session_store,
            &ctx.scheduler_store,
            None,
            "@user:matrix.org",
            false,
            &ctx.config,
            &ctx.warm_manager,
        )
        .await;

        assert!(result.is_ok());
        assert!(room.has_message_containing("No channel attached"));
    }

    #[tokio::test]
    async fn test_debug_status() {
        let ctx = TestContext::new();
        let room = MockChannel::new("!channel:matrix.org");
        ctx.create_channel("test-channel", "!channel:matrix.org");
        let cmd = make_command("debug", vec![]);

        let result = handle_command(
            &room,
            &cmd,
            &ctx.session_store,
            &ctx.scheduler_store,
            None,
            "@user:matrix.org",
            false,
            &ctx.config,
            &ctx.warm_manager,
        )
        .await;

        assert!(result.is_ok());
        assert!(room.has_message_containing("Debug mode"));
    }

    // =========================================================================
    // Backend Command Tests
    // =========================================================================

    #[tokio::test]
    async fn test_backend_rejected_in_dm() {
        let ctx = TestContext::new();
        let room = MockChannel::dm("!dm:matrix.org");
        let cmd = make_command("backend", vec![]);

        let result = handle_command(
            &room,
            &cmd,
            &ctx.session_store,
            &ctx.scheduler_store,
            None,
            "@user:matrix.org",
            true,
            &ctx.config,
            &ctx.warm_manager,
        )
        .await;

        assert!(result.is_ok());
        assert!(room.has_message_containing("only works in channel rooms"));
    }

    #[tokio::test]
    async fn test_backend_status() {
        let ctx = TestContext::new();
        let room = MockChannel::new("!channel:matrix.org");
        ctx.create_channel("test-channel", "!channel:matrix.org");
        let cmd = make_command("backend", vec![]);

        let result = handle_command(
            &room,
            &cmd,
            &ctx.session_store,
            &ctx.scheduler_store,
            None,
            "@user:matrix.org",
            false,
            &ctx.config,
            &ctx.warm_manager,
        )
        .await;

        assert!(result.is_ok());
        assert!(room.has_message_containing("Backend Status"));
    }

    #[tokio::test]
    async fn test_backend_list() {
        let ctx = TestContext::new();
        let room = MockChannel::new("!channel:matrix.org");
        ctx.create_channel("test-channel", "!channel:matrix.org");
        let cmd = make_command("backend", vec!["list"]);

        let result = handle_command(
            &room,
            &cmd,
            &ctx.session_store,
            &ctx.scheduler_store,
            None,
            "@user:matrix.org",
            false,
            &ctx.config,
            &ctx.warm_manager,
        )
        .await;

        assert!(result.is_ok());
        assert!(room.has_message_containing("Available Backends"));
        assert!(room.has_message_containing("acp, mux, direct"));
    }

    #[tokio::test]
    async fn test_backend_set_valid() {
        let ctx = TestContext::new();
        let room = MockChannel::new("!channel:matrix.org");
        ctx.create_channel("test-channel", "!channel:matrix.org");
        let cmd = make_command("backend", vec!["set", "mux"]);

        let result = handle_command(
            &room,
            &cmd,
            &ctx.session_store,
            &ctx.scheduler_store,
            None,
            "@user:matrix.org",
            false,
            &ctx.config,
            &ctx.warm_manager,
        )
        .await;

        assert!(result.is_ok());
        assert!(room.has_message_containing("Backend changed to: mux"));

        // Verify it was actually saved
        let channel = ctx.session_store.get_by_name("test-channel").unwrap().unwrap();
        assert_eq!(channel.backend_type, Some("mux".to_string()));
    }

    #[tokio::test]
    async fn test_backend_set_invalid() {
        let ctx = TestContext::new();
        let room = MockChannel::new("!channel:matrix.org");
        ctx.create_channel("test-channel", "!channel:matrix.org");
        let cmd = make_command("backend", vec!["set", "invalid"]);

        let result = handle_command(
            &room,
            &cmd,
            &ctx.session_store,
            &ctx.scheduler_store,
            None,
            "@user:matrix.org",
            false,
            &ctx.config,
            &ctx.warm_manager,
        )
        .await;

        assert!(result.is_ok());
        assert!(room.has_message_containing("Unknown backend"));
    }

    #[tokio::test]
    async fn test_backend_reset() {
        let ctx = TestContext::new();
        let room = MockChannel::new("!channel:matrix.org");
        ctx.create_channel("test-channel", "!channel:matrix.org");
        // First set a backend
        ctx.session_store.update_backend_type("test-channel", Some("mux")).unwrap();
        let cmd = make_command("backend", vec!["reset"]);

        let result = handle_command(
            &room,
            &cmd,
            &ctx.session_store,
            &ctx.scheduler_store,
            None,
            "@user:matrix.org",
            false,
            &ctx.config,
            &ctx.warm_manager,
        )
        .await;

        assert!(result.is_ok());
        assert!(room.has_message_containing("reset to global default"));

        // Verify it was reset
        let channel = ctx.session_store.get_by_name("test-channel").unwrap().unwrap();
        assert_eq!(channel.backend_type, None);
    }

    // =========================================================================
    // Reset Command Tests
    // =========================================================================

    #[tokio::test]
    async fn test_reset_delegated_in_dm() {
        let ctx = TestContext::new();
        let room = MockChannel::dm("!dm:matrix.org");
        let cmd = make_command("reset", vec!["channel-name"]);

        let result = handle_command(
            &room,
            &cmd,
            &ctx.session_store,
            &ctx.scheduler_store,
            None,
            "@user:matrix.org",
            true, // is_dm
            &ctx.config,
            &ctx.warm_manager,
        )
        .await;

        // Should delegate to Matrix handler
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("DELEGATE_TO_MATRIX:reset"));
    }

    // =========================================================================
    // Delegated Command Tests
    // Note: All reset commands (both local and remote) are delegated to matrix_commands.rs
    // to ensure consistent use of reset_session (which also resets the started flag)
    // =========================================================================

    #[tokio::test]
    async fn test_create_delegated() {
        let ctx = TestContext::new();
        let room = MockChannel::dm("!dm:matrix.org");
        let cmd = make_command("create", vec!["new-channel"]);

        let result = handle_command(
            &room,
            &cmd,
            &ctx.session_store,
            &ctx.scheduler_store,
            None,
            "@user:matrix.org",
            true,
            &ctx.config,
            &ctx.warm_manager,
        )
        .await;

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("DELEGATE_TO_MATRIX:create"));
    }

    #[tokio::test]
    async fn test_join_delegated() {
        let ctx = TestContext::new();
        let room = MockChannel::dm("!dm:matrix.org");
        let cmd = make_command("join", vec!["channel-name"]);

        let result = handle_command(
            &room,
            &cmd,
            &ctx.session_store,
            &ctx.scheduler_store,
            None,
            "@user:matrix.org",
            true,
            &ctx.config,
            &ctx.warm_manager,
        )
        .await;

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("DELEGATE_TO_MATRIX:join"));
    }

    #[tokio::test]
    async fn test_schedule_delegated() {
        let ctx = TestContext::new();
        let room = MockChannel::new("!channel:matrix.org");
        let cmd = make_command("schedule", vec!["in", "1", "hour", "test"]);

        let result = handle_command(
            &room,
            &cmd,
            &ctx.session_store,
            &ctx.scheduler_store,
            None,
            "@user:matrix.org",
            false,
            &ctx.config,
            &ctx.warm_manager,
        )
        .await;

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("DELEGATE_TO_MATRIX:schedule"));
    }

    // =========================================================================
    // Unknown Command Tests
    // =========================================================================

    #[tokio::test]
    async fn test_unknown_command_dm() {
        let ctx = TestContext::new();
        let room = MockChannel::dm("!dm:matrix.org");
        let cmd = make_command("foobar", vec![]);

        let result = handle_command(
            &room,
            &cmd,
            &ctx.session_store,
            &ctx.scheduler_store,
            None,
            "@user:matrix.org",
            true,
            &ctx.config,
            &ctx.warm_manager,
        )
        .await;

        assert!(result.is_ok());
        assert!(room.has_message_containing("Unknown command"));
        assert!(room.has_message_containing("!create"));
    }

    #[tokio::test]
    async fn test_unknown_command_channel() {
        let ctx = TestContext::new();
        let room = MockChannel::new("!channel:matrix.org");
        let cmd = make_command("foobar", vec![]);

        let result = handle_command(
            &room,
            &cmd,
            &ctx.session_store,
            &ctx.scheduler_store,
            None,
            "@user:matrix.org",
            false,
            &ctx.config,
            &ctx.warm_manager,
        )
        .await;

        assert!(result.is_ok());
        assert!(room.has_message_containing("Unknown command"));
        assert!(room.has_message_containing("!status"));
    }
}
