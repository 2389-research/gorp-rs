// ABOUTME: Message handler that processes Matrix room messages with authentication and Claude invocation.
// ABOUTME: Checks room ID and user whitelist, manages typing indicators, streams tool usage to Matrix.

use anyhow::Result;
use matrix_sdk::{
    media::{MediaFormat, MediaRequestParameters},
    room::Room,
    ruma::events::room::message::{
        MessageType, RoomMessageEventContent,
    },
    Client, RoomState,
};

use crate::{
    claude::{self, ClaudeEvent},
    config::Config,
    matrix_client,
    metrics,
    scheduler::{
        parse_time_expression, ParsedSchedule, ScheduleStatus, ScheduledPrompt, SchedulerStore,
    },
    session::SessionStore,
    utils::{chunk_message, log_matrix_message, markdown_to_html, MAX_CHUNK_SIZE},
};
use chrono::Utc;
use std::path::Path;

/// Help documentation loaded at compile time
const HELP_MD: &str = include_str!("../docs/HELP.md");
/// Message of the day shown on boot
const MOTD_MD: &str = include_str!("../docs/MOTD.md");
/// Changelog documentation
const CHANGELOG_MD: &str = include_str!("../docs/CHANGELOG.md");

/// Check if debug mode is enabled for a channel directory
/// Debug mode is enabled by creating an empty file: .gorp/enable-debug
fn is_debug_enabled(channel_dir: &str) -> bool {
    let debug_path = Path::new(channel_dir).join(".gorp").join("enable-debug");
    debug_path.exists()
}

/// Write context file for MCP tools to read
/// This tells tools like gorp_schedule_prompt which channel/room they're operating in
async fn write_context_file(
    channel_dir: &str,
    room_id: &str,
    channel_name: &str,
    session_id: &str,
) -> Result<()> {
    let gorp_dir = Path::new(channel_dir).join(".gorp");
    tokio::fs::create_dir_all(&gorp_dir).await?;

    let context = serde_json::json!({
        "room_id": room_id,
        "channel_name": channel_name,
        "session_id": session_id,
        "updated_at": chrono::Utc::now().to_rfc3339()
    });

    let context_path = gorp_dir.join("context.json");
    tokio::fs::write(&context_path, serde_json::to_string_pretty(&context)?).await?;

    tracing::debug!(path = %context_path.display(), "Wrote MCP context file");
    Ok(())
}

/// Download an attachment from Matrix and save it to the workspace
/// Returns the relative path to the saved file
async fn download_attachment(
    client: &Client,
    source: &matrix_sdk::ruma::events::room::MediaSource,
    filename: &str,
    workspace_dir: &str,
) -> Result<String> {
    use tokio::io::AsyncWriteExt;

    // Create attachments directory
    let attachments_dir = Path::new(workspace_dir).join("attachments");
    tokio::fs::create_dir_all(&attachments_dir).await?;

    // Generate unique filename to avoid collisions
    let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");
    let safe_filename = filename
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '.' || *c == '-' || *c == '_')
        .collect::<String>();
    let unique_filename = format!("{}_{}", timestamp, safe_filename);
    let file_path = attachments_dir.join(&unique_filename);

    // Download the media
    let request = MediaRequestParameters {
        source: source.clone(),
        format: MediaFormat::File,
    };

    let data = client
        .media()
        .get_media_content(&request, true) // use_cache=true
        .await
        .map_err(|e| anyhow::anyhow!("Failed to download media: {}", e))?;

    // Write to file
    let mut file = tokio::fs::File::create(&file_path).await?;
    file.write_all(&data).await?;

    tracing::info!(
        filename = %unique_filename,
        size = data.len(),
        "Downloaded attachment"
    );

    Ok(format!("attachments/{}", unique_filename))
}

pub async fn handle_message(
    room: Room,
    event: matrix_sdk::ruma::events::room::message::OriginalSyncRoomMessageEvent,
    client: Client,
    config: Config,
    session_store: SessionStore,
    scheduler_store: SchedulerStore,
) -> Result<()> {
    let start_time = std::time::Instant::now();

    // Only work with joined rooms
    if room.state() != RoomState::Joined {
        return Ok(());
    }

    // Check if this is a DM (direct message)
    let is_dm = room.is_direct().await.unwrap_or(false);

    let sender = event.sender.as_str();
    let body = &event.content.body();

    // Ignore bot's own messages
    let Some(bot_user_id) = client.user_id() else {
        tracing::warn!("Bot user_id not available, skipping message");
        return Ok(());
    };
    if sender == bot_user_id.as_str() {
        return Ok(());
    }

    // Check whitelist
    let allowed_users = config.allowed_users_set();
    if !allowed_users.contains(sender) {
        tracing::debug!(sender, "Ignoring message from unauthorized user");
        return Ok(());
    }

    // Safe preview generation (respects UTF-8 boundaries)
    let message_preview: String = body.chars().take(50).collect();
    tracing::info!(sender, room_id = %room.room_id(), message_preview, "Processing message");

    // Handle commands - be specific about what constitutes a command
    // Accept "!claude <command>" or "!<word>" (but not just "!" or "!!")
    let is_command = body.starts_with("!claude ")
        || (body.starts_with("!")
            && body.len() > 1
            && body.chars().nth(1).map_or(false, |c| c.is_alphabetic()));

    if is_command {
        metrics::record_message_received("command");
        let result = handle_command(
            room,
            body,
            &session_store,
            &scheduler_store,
            &client,
            sender,
            is_dm,
            &config,
        )
        .await;
        let duration = start_time.elapsed().as_secs_f64();
        metrics::record_message_processing_duration(duration);
        return result;
    }

    // Regular chat message
    metrics::record_message_received("chat");

    // Check if channel is attached
    let Some(channel) = session_store.get_by_room(room.room_id().as_str())? else {
        let help_msg = if is_dm {
            // Check if this is a new user (no channels at all)
            let all_channels = session_store.list_all().unwrap_or_default();
            if all_channels.is_empty() {
                // New user - show welcome with suggested channels
                "👋 **Welcome to gorp!**\n\n\
                I'm your AI assistant with persistent sessions and workspace directories.\n\n\
                **Get started with these recommended channels:**\n\n\
                ```\n\
                !create pa        # Personal assistant for email, calendar, tasks\n\
                !create news      # News aggregation and curation\n\
                !create research  # Research projects with auditable citations\n\
                !create weather   # Weather updates and forecasts\n\
                ```\n\n\
                Each channel gets its own workspace with pre-configured settings.\n\n\
                Type `!help` for all commands or `!list` to see your channels."
                    .to_string()
            } else {
                "👋 To get started, create a channel:\n\n\
                !create <channel-name>\n\n\
                Example: !create PA\n\
                This creates a dedicated Claude session in a workspace directory.\n\n\
                Type !list to see your existing channels."
                    .to_string()
            }
        } else {
            "No Claude channel attached to this room.\n\n\
            💡 DM me to create a channel with: !create <name>\n\n\
            Need help? Send: !help"
                .to_string()
        };
        let help_html = markdown_to_html(&help_msg);
        room.send(RoomMessageEventContent::text_html(&help_msg, &help_html))
            .await?;
        return Ok(());
    };

    // Check for attachments (images, files) and build the prompt
    let prompt = match &event.content.msgtype {
        MessageType::Image(image_content) => {
            // Download the image
            let filename = image_content.body.clone();
            match download_attachment(&client, &image_content.source, &filename, &channel.directory)
                .await
            {
                Ok(rel_path) => {
                    let abs_path = format!("{}/{}", channel.directory, rel_path);
                    tracing::info!(path = %abs_path, "Image downloaded");
                    // Include image path in prompt for Claude to read
                    format!(
                        "[Attached image: {}]\n\n{}",
                        abs_path,
                        image_content.body
                    )
                }
                Err(e) => {
                    tracing::error!(error = %e, "Failed to download image");
                    room.send(RoomMessageEventContent::text_plain(&format!(
                        "⚠️ Failed to download image: {}",
                        e
                    )))
                    .await?;
                    return Ok(());
                }
            }
        }
        MessageType::File(file_content) => {
            // Download the file
            let filename = file_content.body.clone();
            match download_attachment(&client, &file_content.source, &filename, &channel.directory)
                .await
            {
                Ok(rel_path) => {
                    let abs_path = format!("{}/{}", channel.directory, rel_path);
                    tracing::info!(path = %abs_path, "File downloaded");
                    format!(
                        "[Attached file: {}]\n\n{}",
                        abs_path,
                        file_content.body
                    )
                }
                Err(e) => {
                    tracing::error!(error = %e, "Failed to download file");
                    room.send(RoomMessageEventContent::text_plain(&format!(
                        "⚠️ Failed to download file: {}",
                        e
                    )))
                    .await?;
                    return Ok(());
                }
            }
        }
        _ => {
            // Text message or other type - use body as-is
            body.to_string()
        }
    };

    let channel_args = channel.cli_args();

    // Write context file for MCP tools (before Claude invocation)
    if let Err(e) = write_context_file(
        &channel.directory,
        room.room_id().as_str(),
        &channel.channel_name,
        &channel.session_id,
    )
    .await
    {
        tracing::warn!(error = %e, "Failed to write MCP context file");
        // Non-fatal - continue without context file
    }

    // Start typing indicator and keep it alive
    room.typing_notice(true).await?;

    // Spawn a task to keep the typing indicator refreshed every 25 seconds
    let typing_room = room.clone();
    let (typing_tx, mut typing_rx) = tokio::sync::oneshot::channel();
    let typing_handle = tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(25));
        interval.tick().await; // Skip first immediate tick

        loop {
            tokio::select! {
                _ = interval.tick() => {
                    if let Err(e) = typing_room.typing_notice(true).await {
                        tracing::warn!(error = %e, "Failed to refresh typing indicator");
                        break;
                    }
                }
                _ = &mut typing_rx => {
                    // Stop signal received
                    break;
                }
            }
        }
    });

    // Invoke Claude with streaming to show tool usage
    let claude_start = std::time::Instant::now();
    metrics::record_claude_invocation("matrix");
    let mut event_rx = match claude::invoke_claude_streaming(
        &config.claude.binary_path,
        config.claude.sdk_url.as_deref(),
        channel_args,
        &prompt,
        Some(&channel.directory),
    )
    .await
    {
        Ok(rx) => rx,
        Err(e) => {
            tracing::error!(error = %e, "Claude invocation failed");
            metrics::record_error("claude_invocation");
            let error_msg = format!("⚠️ Claude error: {}", e);

            let _ = typing_tx.send(());
            typing_handle.abort();
            room.typing_notice(false).await?;

            room.send(RoomMessageEventContent::text_plain(&error_msg))
                .await?;
            return Ok(());
        }
    };

    // Check if debug mode is enabled for this channel
    // Debug mode shows tool usage in Matrix (create .gorp/enable-debug to enable)
    let debug_enabled = is_debug_enabled(&channel.directory);
    if debug_enabled {
        tracing::debug!(channel = %channel.channel_name, "Debug mode enabled - will show tool usage");
    }

    // Process streaming events
    let mut final_response: Option<String> = None;
    let mut tools_used: Vec<String> = Vec::new();

    while let Some(event) = event_rx.recv().await {
        match event {
            ClaudeEvent::ToolUse {
                name,
                input_preview,
            } => {
                tools_used.push(name.clone());
                metrics::record_tool_used(&name);

                // Only send tool notifications if debug mode is enabled
                if debug_enabled {
                    // Build tool message with plain and HTML versions
                    let (plain, html) = if input_preview.is_empty() {
                        (format!("🔧 {}", name), format!("🔧 <code>{}</code>", name))
                    } else {
                        (
                            format!("🔧 {} · {}", name, input_preview),
                            format!("🔧 <code>{}</code> · <code>{}</code>", name, input_preview),
                        )
                    };

                    // Send tool notification to room
                    if let Err(e) = room
                        .send(RoomMessageEventContent::text_html(&plain, &html))
                        .await
                    {
                        tracing::warn!(error = %e, "Failed to send tool notification");
                    } else {
                        log_matrix_message(
                            &channel.directory,
                            room.room_id().as_str(),
                            "tool_notification",
                            &plain,
                            Some(&html),
                            None,
                            None,
                        )
                        .await;
                    }
                }
            }
            ClaudeEvent::Result { text, usage } => {
                // Record token usage metrics
                metrics::record_claude_tokens(
                    usage.input_tokens,
                    usage.output_tokens,
                    usage.cache_read_tokens,
                    usage.cache_creation_tokens,
                );
                // Convert dollars to cents and record
                let cost_cents = (usage.total_cost_usd * 100.0).round() as u64;
                metrics::record_claude_cost_cents(cost_cents);

                tracing::info!(
                    input_tokens = usage.input_tokens,
                    output_tokens = usage.output_tokens,
                    cost_usd = usage.total_cost_usd,
                    "Claude usage recorded"
                );

                final_response = Some(text);
            }
            ClaudeEvent::Error(error) => {
                let _ = typing_tx.send(());
                typing_handle.abort();
                room.typing_notice(false).await?;

                metrics::record_error("claude_streaming");
                let error_msg = format!("⚠️ Claude error: {}", error);
                room.send(RoomMessageEventContent::text_plain(&error_msg))
                    .await?;
                return Ok(());
            }
        }
    }

    let response = match final_response {
        Some(r) => {
            let claude_duration = claude_start.elapsed().as_secs_f64();
            metrics::record_claude_duration(claude_duration);
            metrics::record_claude_response_length(r.len());
            tracing::info!(
                response_length = r.len(),
                tools_count = tools_used.len(),
                "Claude responded"
            );
            r
        }
        None => {
            let _ = typing_tx.send(());
            typing_handle.abort();
            room.typing_notice(false).await?;

            metrics::record_error("claude_no_response");
            room.send(RoomMessageEventContent::text_plain(
                "⚠️ Claude finished without a response",
            ))
            .await?;
            return Ok(());
        }
    };

    // Mark session as started BEFORE sending response (to ensure consistency)
    session_store.mark_started(room.room_id().as_str())?;

    // Stop typing indicator refresh
    let _ = typing_tx.send(());
    let _ = typing_handle.await; // Wait for graceful shutdown
    room.typing_notice(false).await?;

    // Send response with markdown formatting, chunked if too long
    // Matrix limit is ~65KB but we chunk for better display
    let chunks = chunk_message(&response, MAX_CHUNK_SIZE);
    let chunk_count = chunks.len();

    for (i, chunk) in chunks.into_iter().enumerate() {
        let html = markdown_to_html(&chunk);
        room.send(RoomMessageEventContent::text_html(&chunk, &html))
            .await?;
        metrics::record_message_sent();

        // Log the Matrix message
        log_matrix_message(
            &channel.directory,
            room.room_id().as_str(),
            "response",
            &chunk,
            Some(&html),
            if chunk_count > 1 { Some(i) } else { None },
            if chunk_count > 1 {
                Some(chunk_count)
            } else {
                None
            },
        )
        .await;

        // Small delay between chunks to maintain order
        if i < chunk_count - 1 {
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }
    }

    // Record total message processing time
    let total_duration = start_time.elapsed().as_secs_f64();
    metrics::record_message_processing_duration(total_duration);

    tracing::info!(chunk_count, "Response sent successfully");

    Ok(())
}

async fn handle_command(
    room: Room,
    body: &str,
    session_store: &SessionStore,
    scheduler_store: &SchedulerStore,
    client: &Client,
    sender: &str,
    is_dm: bool,
    config: &Config,
) -> Result<()> {
    let parts: Vec<&str> = body.split_whitespace().collect();

    // Strip !claude prefix if present, otherwise treat whole thing as command
    let command_parts: Vec<&str> = if body.starts_with("!claude ") {
        parts[1..].to_vec()
    } else if body.starts_with("!") {
        let mut p = parts.clone();
        if let Some(first) = p.first_mut() {
            *first = &first[1..]; // Remove the ! prefix
        }
        p
    } else {
        parts.clone()
    };

    if command_parts.is_empty() || command_parts[0].is_empty() {
        let help_msg = if is_dm {
            "💬 Orchestrator Commands:\n\
            !create <name> - Create new channel\n\
            !join <name> - Get invited to a channel\n\
            !delete <name> - Remove channel (keeps workspace)\n\
            !cleanup - Leave orphaned rooms\n\
            !restore-rooms - Restore channels from workspace directories\n\
            !list - Show all channels\n\
            !help - Show detailed help"
        } else {
            "Available commands:\n\
            !help - Show detailed help\n\
            !status - Show current channel info\n\
            !debug - Toggle tool usage display\n\
            !leave - Bot leaves this room"
        };
        room.send(RoomMessageEventContent::text_plain(help_msg))
            .await?;
        return Ok(());
    }

    let command = command_parts[0];
    metrics::record_command(command);

    match command {
        "help" => {
            // Send help as HTML (converted from markdown)
            let help_html = markdown_to_html(HELP_MD);
            room.send(RoomMessageEventContent::text_html(HELP_MD, &help_html))
                .await?;
        }
        "changelog" => {
            // Send changelog as HTML (converted from markdown)
            let changelog_html = markdown_to_html(CHANGELOG_MD);
            room.send(RoomMessageEventContent::text_html(CHANGELOG_MD, &changelog_html))
                .await?;
        }
        "motd" => {
            // Send message of the day as HTML
            let motd_html = markdown_to_html(MOTD_MD);
            room.send(RoomMessageEventContent::text_html(MOTD_MD, &motd_html))
                .await?;
        }
        "debug" => {
            if is_dm {
                room.send(RoomMessageEventContent::text_plain(
                    "❌ The !debug command only works in channel rooms.",
                ))
                .await?;
                return Ok(());
            }

            // Get channel directory
            let Some(channel) = session_store.get_by_room(room.room_id().as_str())? else {
                room.send(RoomMessageEventContent::text_plain(
                    "No channel attached to this room.",
                ))
                .await?;
                return Ok(());
            };

            // Note: Channel directory is validated at database read time via Channel::validate_directory()
            let channel_path = std::path::Path::new(&channel.directory);
            let debug_dir = channel_path.join(".gorp");
            let debug_file = debug_dir.join("enable-debug");

            let subcommand = command_parts.get(1).map(|s| s.to_lowercase());
            match subcommand.as_deref() {
                Some("on") | Some("enable") => {
                    // Create .gorp directory if needed
                    if let Err(e) = std::fs::create_dir_all(&debug_dir) {
                        room.send(RoomMessageEventContent::text_plain(&format!(
                            "⚠️ Failed to create debug directory: {}",
                            e
                        )))
                        .await?;
                        return Ok(());
                    }
                    // Create enable-debug file
                    if let Err(e) = std::fs::write(&debug_file, "") {
                        room.send(RoomMessageEventContent::text_plain(&format!(
                            "⚠️ Failed to enable debug: {}",
                            e
                        )))
                        .await?;
                        return Ok(());
                    }
                    room.send(RoomMessageEventContent::text_plain(
                        "🔧 Debug mode ENABLED\n\nTool usage will now be shown in this channel.",
                    ))
                    .await?;
                    tracing::info!(channel = %channel.channel_name, "Debug mode enabled");
                }
                Some("off") | Some("disable") => {
                    // Remove enable-debug file if it exists
                    if debug_file.exists() {
                        if let Err(e) = std::fs::remove_file(&debug_file) {
                            room.send(RoomMessageEventContent::text_plain(&format!(
                                "⚠️ Failed to disable debug: {}",
                                e
                            )))
                            .await?;
                            return Ok(());
                        }
                    }
                    room.send(RoomMessageEventContent::text_plain(
                        "🔇 Debug mode DISABLED\n\nTool usage will be hidden in this channel.",
                    ))
                    .await?;
                    tracing::info!(channel = %channel.channel_name, "Debug mode disabled");
                }
                _ => {
                    // Show current status
                    let status = if debug_file.exists() {
                        "🔧 Debug mode is ENABLED\n\nTool usage is shown in this channel."
                    } else {
                        "🔇 Debug mode is DISABLED\n\nTool usage is hidden in this channel."
                    };
                    room.send(RoomMessageEventContent::text_plain(&format!(
                        "{}\n\nCommands:\n  !debug on - Show tool usage\n  !debug off - Hide tool usage",
                        status
                    )))
                    .await?;
                }
            }
        }
        "status" => {
            if let Some(channel) = session_store.get_by_room(room.room_id().as_str())? {
                let debug_status = if is_debug_enabled(&channel.directory) {
                    "🔧 Enabled (tool usage shown)"
                } else {
                    "🔇 Disabled (tool usage hidden)"
                };
                let status = format!(
                    "📊 Channel Status\n\n\
                    Channel: {}\n\
                    Session ID: {}\n\
                    Directory: {}\n\
                    Started: {}\n\
                    Debug Mode: {}\n\n\
                    Webhook URL:\n\
                    POST http://{}:{}/webhook/session/{}\n\n\
                    This room is backed by a persistent Claude session.",
                    channel.channel_name,
                    channel.session_id,
                    channel.directory,
                    if channel.started {
                        "Yes"
                    } else {
                        "No (first message will start it)"
                    },
                    debug_status,
                    config.webhook.host,
                    config.webhook.port,
                    channel.session_id
                );
                room.send(RoomMessageEventContent::text_plain(&status))
                    .await?;
            } else {
                room.send(RoomMessageEventContent::text_plain(
                    "📊 Channel Status\n\n\
                    No channel attached.\n\n\
                    DM me to create one: !create <name>",
                ))
                .await?;
            }
        }
        "create" => {
            if !is_dm {
                room.send(RoomMessageEventContent::text_plain(
                    "❌ The !create command only works in DMs.\n\nDM me to create a new channel!",
                ))
                .await?;
                return Ok(());
            }

            if command_parts.len() < 2 {
                room.send(RoomMessageEventContent::text_plain(
                    "Usage: !create <channel-name>\n\n\
                    Example: !create PA\n\
                    Example: !create dev-help\n\n\
                    This will create a workspace directory and Matrix room for the channel.",
                ))
                .await?;
                return Ok(());
            }

            // Normalize channel name to lowercase for consistency
            let channel_name = command_parts[1].to_lowercase();

            // Validate channel name (alphanumeric, dashes, underscores only)
            if !channel_name
                .chars()
                .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
            {
                room.send(RoomMessageEventContent::text_plain(
                    "❌ Channel name can only contain letters, numbers, dashes, and underscores.\n\n\
                    Example: PA, dev-help, my_channel",
                ))
                .await?;
                return Ok(());
            }

            // Check if channel already exists (case-insensitive)
            if session_store.get_by_name(&channel_name)?.is_some() {
                room.send(RoomMessageEventContent::text_plain(&format!(
                    "❌ Channel '{}' already exists.\n\nUse !list to see all channels.",
                    channel_name
                )))
                .await?;
                return Ok(());
            }

            // Create Matrix room
            let room_name = format!("{}: {}", config.matrix.room_prefix, channel_name);
            let new_room_id = matrix_client::create_room(client, &room_name).await?;
            metrics::record_room_created();

            // Invite user
            matrix_client::invite_user(client, &new_room_id, sender).await?;

            // Create channel in database (this also creates the directory)
            let channel = session_store.create_channel(&channel_name, new_room_id.as_str())?;
            metrics::increment_active_channels();

            let response = format!(
                "✅ Created Channel: {}\n\n\
                Room: {}\n\
                Session ID: {}\n\
                Directory: {}\n\n\
                Check your room list - I've invited you!\n\
                Once you join, just send messages to start working.\n\n\
                Webhook: POST http://{}:{}/webhook/session/{}",
                channel_name,
                room_name,
                &channel.session_id[..8],
                channel.directory,
                config.webhook.host,
                config.webhook.port,
                channel.session_id
            );
            room.send(RoomMessageEventContent::text_plain(&response))
                .await?;

            tracing::info!(
                room_id = %new_room_id,
                channel_name = %channel_name,
                session_id = %channel.session_id,
                directory = %channel.directory,
                user = %sender,
                "Created channel for user"
            );
        }
        "join" => {
            if !is_dm {
                room.send(RoomMessageEventContent::text_plain(
                    "❌ The !join command only works in DMs.",
                ))
                .await?;
                return Ok(());
            }

            if command_parts.len() < 2 {
                room.send(RoomMessageEventContent::text_plain(
                    "Usage: !join <channel-name>\n\n\
                    Sends you an invite to the channel's room.\n\
                    Use !list to see available channels.",
                ))
                .await?;
                return Ok(());
            }

            let channel_name = command_parts[1].to_lowercase();

            // Find the channel
            let Some(channel) = session_store.get_by_name(&channel_name)? else {
                room.send(RoomMessageEventContent::text_plain(&format!(
                    "❌ Channel '{}' not found.\n\nUse !list to see all channels.",
                    channel_name
                )))
                .await?;
                return Ok(());
            };

            // Invite user to the room
            let room_id: matrix_sdk::ruma::OwnedRoomId = channel
                .room_id
                .parse()
                .map_err(|e| anyhow::anyhow!("Invalid room ID: {}", e))?;
            match matrix_client::invite_user(client, &room_id, sender).await {
                Ok(_) => {
                    room.send(RoomMessageEventContent::text_plain(&format!(
                        "✅ Invited you to channel '{}'!\n\nCheck your room invites.",
                        channel_name
                    )))
                    .await?;
                    tracing::info!(
                        channel_name = %channel_name,
                        user = %sender,
                        "User invited to channel"
                    );
                }
                Err(e) => {
                    // Check if already in room
                    let err_str = e.to_string();
                    if err_str.contains("already in the room")
                        || err_str.contains("is already joined")
                    {
                        room.send(RoomMessageEventContent::text_plain(&format!(
                            "ℹ️ You're already in channel '{}'!",
                            channel_name
                        )))
                        .await?;
                    } else {
                        room.send(RoomMessageEventContent::text_plain(&format!(
                            "⚠️ Failed to invite: {}",
                            e
                        )))
                        .await?;
                    }
                }
            }
        }
        "delete" => {
            if !is_dm {
                room.send(RoomMessageEventContent::text_plain(
                    "❌ The !delete command only works in DMs.\n\nDM me to manage channels!",
                ))
                .await?;
                return Ok(());
            }

            if command_parts.len() < 2 {
                room.send(RoomMessageEventContent::text_plain(
                    "Usage: !delete <channel-name>\n\n\
                    This removes the channel from the database and the bot leaves the room.\n\
                    The workspace directory is preserved.",
                ))
                .await?;
                return Ok(());
            }

            let channel_name = command_parts[1].to_lowercase();

            // Find the channel
            let Some(channel) = session_store.get_by_name(&channel_name)? else {
                room.send(RoomMessageEventContent::text_plain(&format!(
                    "❌ Channel '{}' not found.\n\nUse !list to see all channels.",
                    channel_name
                )))
                .await?;
                return Ok(());
            };

            // Leave the room
            let room_id = channel.room_id.clone();
            if let Some(target_room) = client.get_room(
                <&matrix_sdk::ruma::RoomId>::try_from(room_id.as_str())
                    .map_err(|e| anyhow::anyhow!("Invalid room ID: {}", e))?,
            ) {
                if let Err(e) = target_room.leave().await {
                    tracing::warn!(error = %e, room_id = %room_id, "Failed to leave room");
                }
            }

            // Remove from database (keeps directory)
            session_store.delete_channel(&channel_name)?;
            metrics::decrement_active_channels();

            let response = format!(
                "✅ Deleted channel: {}\n\n\
                - Bot left the room\n\
                - Removed from database\n\
                - Workspace preserved: {}",
                channel_name, channel.directory
            );
            room.send(RoomMessageEventContent::text_plain(&response))
                .await?;

            tracing::info!(
                channel_name = %channel_name,
                room_id = %room_id,
                "Channel deleted by user"
            );
        }
        "leave" => {
            // Bot leaves current room, removes from database if tracked
            let room_id = room.room_id().to_string();

            // Check if this room is tracked
            let channel_name = session_store.delete_by_room(&room_id)?;
            if channel_name.is_some() {
                metrics::decrement_active_channels();
            }

            let goodbye = if let Some(name) = &channel_name {
                format!(
                    "👋 Leaving channel '{}'. Workspace preserved. Goodbye!",
                    name
                )
            } else {
                "👋 Goodbye!".to_string()
            };

            room.send(RoomMessageEventContent::text_plain(&goodbye))
                .await?;

            // Leave the room
            if let Err(e) = room.leave().await {
                tracing::error!(error = %e, room_id = %room_id, "Failed to leave room");
            } else {
                tracing::info!(
                    room_id = %room_id,
                    channel_name = ?channel_name,
                    "Bot left room"
                );
            }
        }
        "cleanup" => {
            if !is_dm {
                room.send(RoomMessageEventContent::text_plain(
                    "❌ The !cleanup command only works in DMs.",
                ))
                .await?;
                return Ok(());
            }

            room.send(RoomMessageEventContent::text_plain(
                "🧹 Scanning for orphaned rooms and stale database entries...",
            ))
            .await?;

            let mut cleaned_rooms = Vec::new();
            let mut cleaned_db = Vec::new();
            let mut errors = Vec::new();

            // Get all joined room IDs for checking stale DB entries
            let joined_room_ids: std::collections::HashSet<String> = client
                .joined_rooms()
                .iter()
                .map(|r| r.room_id().to_string())
                .collect();

            // Phase 1: Clean orphaned rooms (where bot is the only member)
            for joined_room in client.joined_rooms() {
                let room_id = joined_room.room_id().to_string();

                // Get member count (joined members only)
                let members = match joined_room.members(matrix_sdk::RoomMemberships::JOIN).await {
                    Ok(m) => m,
                    Err(e) => {
                        errors.push(format!("{}: {}", room_id, e));
                        continue;
                    }
                };

                // If bot is the only member, clean up
                if members.len() <= 1 {
                    // Get channel name if tracked
                    let channel_name = session_store.delete_by_room(&room_id).ok().flatten();
                    if channel_name.is_some() {
                        metrics::decrement_active_channels();
                    }

                    // Leave the room
                    if let Err(e) = joined_room.leave().await {
                        errors.push(format!("{}: {}", room_id, e));
                        continue;
                    }

                    let name = channel_name.unwrap_or_else(|| room_id.clone());
                    cleaned_rooms.push(name);
                }
            }

            // Phase 2: Clean stale database entries (rooms bot is no longer in)
            if let Ok(channels) = session_store.list_all() {
                for channel in channels {
                    if !joined_room_ids.contains(&channel.room_id) {
                        // Bot is not in this room anymore, clean up DB entry
                        if session_store.delete_by_room(&channel.room_id).is_ok() {
                            metrics::decrement_active_channels();
                            cleaned_db.push(channel.channel_name);
                        }
                    }
                }
            }

            let response = if cleaned_rooms.is_empty() && cleaned_db.is_empty() && errors.is_empty()
            {
                "✅ No orphaned rooms or stale entries found. Everything is clean!".to_string()
            } else {
                let mut msg = String::new();
                if !cleaned_rooms.is_empty() {
                    msg.push_str(&format!(
                        "🧹 Left {} orphaned room(s):\n",
                        cleaned_rooms.len()
                    ));
                    for name in &cleaned_rooms {
                        msg.push_str(&format!("  - {}\n", name));
                    }
                }
                if !cleaned_db.is_empty() {
                    msg.push_str(&format!(
                        "\n🗑️ Removed {} stale database entry(ies):\n",
                        cleaned_db.len()
                    ));
                    for name in &cleaned_db {
                        msg.push_str(&format!("  - {}\n", name));
                    }
                }
                if !errors.is_empty() {
                    msg.push_str(&format!("\n⚠️ {} error(s):\n", errors.len()));
                    for err in &errors {
                        msg.push_str(&format!("  - {}\n", err));
                    }
                }
                msg.push_str("\nWorkspace directories preserved.");
                msg
            };

            room.send(RoomMessageEventContent::text_plain(&response))
                .await?;

            tracing::info!(
                cleaned_rooms = cleaned_rooms.len(),
                cleaned_db = cleaned_db.len(),
                error_count = errors.len(),
                "Cleanup completed"
            );
        }
        "restore-rooms" => {
            if !is_dm {
                room.send(RoomMessageEventContent::text_plain(
                    "❌ The !restore-rooms command only works in DMs.",
                ))
                .await?;
                return Ok(());
            }

            room.send(RoomMessageEventContent::text_plain(
                "🔄 Scanning workspace for directories to restore...",
            ))
            .await?;

            let workspace_path = &config.workspace.path;
            let mut restored = Vec::new();
            let mut skipped = Vec::new();
            let mut errors = Vec::new();

            // Read workspace directory
            let entries = match std::fs::read_dir(workspace_path) {
                Ok(e) => e,
                Err(e) => {
                    room.send(RoomMessageEventContent::text_plain(&format!(
                        "⚠️ Failed to read workspace directory: {}",
                        e
                    )))
                    .await?;
                    return Ok(());
                }
            };

            for entry in entries.flatten() {
                let path = entry.path();

                // Skip non-directories and special items
                if !path.is_dir() {
                    continue;
                }

                let dir_name = match entry.file_name().to_str() {
                    Some(name) => name.to_string(),
                    None => continue,
                };

                // Skip special directories
                if dir_name == "template"
                    || dir_name.starts_with('.')
                    || dir_name == "attachments"
                {
                    continue;
                }

                // Validate channel name format
                if !dir_name
                    .chars()
                    .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
                {
                    skipped.push(format!("{} (invalid name)", dir_name));
                    continue;
                }

                // Verify this looks like a valid workspace (has .claude/ or CLAUDE.md)
                let claude_dir = path.join(".claude");
                let claude_md = path.join("CLAUDE.md");
                if !claude_dir.exists() && !claude_md.exists() {
                    skipped.push(format!("{} (not a workspace)", dir_name));
                    continue;
                }

                // Check if channel already exists in database
                let channel_name = dir_name.to_lowercase();
                if session_store.get_by_name(&channel_name)?.is_some() {
                    skipped.push(format!("{} (already exists)", channel_name));
                    continue;
                }

                // Create Matrix room for this workspace
                let room_name = format!("{}: {}", config.matrix.room_prefix, channel_name);
                match matrix_client::create_room(client, &room_name).await {
                    Ok(new_room_id) => {
                        // Invite user to the room
                        let invite_failed = match matrix_client::invite_user(client, &new_room_id, sender).await {
                            Ok(_) => false,
                            Err(e) => {
                                tracing::warn!(
                                    channel = %channel_name,
                                    error = %e,
                                    "Failed to invite user to restored room"
                                );
                                true
                            }
                        };

                        // Create channel in database (inherits existing directory)
                        match session_store.create_channel(&channel_name, new_room_id.as_str()) {
                            Ok(_channel) => {
                                metrics::increment_active_channels();
                                if invite_failed {
                                    restored.push(format!("{} (invite failed)", channel_name));
                                } else {
                                    restored.push(channel_name.clone());
                                }
                                tracing::info!(
                                    channel = %channel_name,
                                    room_id = %new_room_id,
                                    "Restored channel from workspace"
                                );
                            }
                            Err(e) => {
                                errors.push(format!("{}: {}", channel_name, e));
                            }
                        }
                    }
                    Err(e) => {
                        errors.push(format!("{}: failed to create room - {}", channel_name, e));
                    }
                }
            }

            // Build response
            let response = if restored.is_empty() && skipped.is_empty() && errors.is_empty() {
                "📂 No workspace directories found to restore.".to_string()
            } else {
                let mut msg = String::new();
                if !restored.is_empty() {
                    msg.push_str(&format!(
                        "✅ Restored {} channel(s):\n",
                        restored.len()
                    ));
                    for name in &restored {
                        msg.push_str(&format!("  • {}\n", name));
                    }
                    msg.push_str("\nCheck your room invites!\n");
                }
                if !skipped.is_empty() {
                    msg.push_str(&format!(
                        "\n⏭️ Skipped {} item(s):\n",
                        skipped.len()
                    ));
                    for name in &skipped {
                        msg.push_str(&format!("  • {}\n", name));
                    }
                }
                if !errors.is_empty() {
                    msg.push_str(&format!("\n⚠️ {} error(s):\n", errors.len()));
                    for err in &errors {
                        msg.push_str(&format!("  • {}\n", err));
                    }
                }
                msg
            };

            room.send(RoomMessageEventContent::text_plain(&response))
                .await?;

            tracing::info!(
                restored = restored.len(),
                skipped = skipped.len(),
                errors = errors.len(),
                "Restore-rooms completed"
            );
        }
        "list" => {
            let channels = session_store.list_all()?;

            if channels.is_empty() {
                let msg = if is_dm {
                    "📋 **No Channels Yet**\n\n\
                    Get started with these recommended channels:\n\n\
                    ```\n\
                    !create pa        # Personal assistant\n\
                    !create news      # News curation\n\
                    !create research  # Research projects\n\
                    !create weather   # Weather updates\n\
                    ```\n\n\
                    Each channel has pre-configured settings ready to go."
                } else {
                    "📋 No Channels Found\n\nDM me to create a channel!"
                };
                let msg_html = markdown_to_html(msg);
                room.send(RoomMessageEventContent::text_html(msg, &msg_html)).await?;
                return Ok(());
            }

            let mut list_text = String::from("📋 Your Claude Channels\n\n");

            for (idx, channel) in channels.iter().enumerate() {
                let current_marker = if channel.room_id == room.room_id().as_str() {
                    " ← (current)"
                } else {
                    ""
                };

                list_text.push_str(&format!(
                    "{}. {}{}\n   📁 {}\n   🔑 Session: {}\n   Webhook: /webhook/session/{}\n\n",
                    idx + 1,
                    channel.channel_name,
                    current_marker,
                    channel.directory,
                    &channel.session_id[..8],
                    channel.session_id
                ));
            }

            room.send(RoomMessageEventContent::text_plain(&list_text))
                .await?;
        }
        "schedule" => {
            // Only allow in channels (not DMs)
            if is_dm {
                room.send(RoomMessageEventContent::text_plain(
                    "Scheduling is only available in channels. Create a channel first with !create <name>",
                ))
                .await?;
                return Ok(());
            }

            // Get the channel for this room
            let channel = match session_store.get_by_room(room.room_id().as_str())? {
                Some(c) => c,
                None => {
                    room.send(RoomMessageEventContent::text_plain(
                        "This room is not associated with a channel. Please set up a channel first.",
                    ))
                    .await?;
                    return Ok(());
                }
            };

            // Parse subcommand (args are command_parts[1..])
            let args = &command_parts[1..];
            let subcommand = args.first().map(|s| s.to_lowercase());
            match subcommand.as_deref() {
                Some("list") => {
                    // List schedules for this room
                    let schedules = scheduler_store.list_by_room(room.room_id().as_str())?;
                    if schedules.is_empty() {
                        room.send(RoomMessageEventContent::text_plain(
                            "📅 No scheduled prompts.\n\nCreate one with: !schedule <time> <prompt>\nExamples:\n  !schedule in 2 hours check my inbox\n  !schedule tomorrow 9am summarize my calendar\n  !schedule every monday 8am weekly standup",
                        ))
                        .await?;
                    } else {
                        let mut msg = String::from("📅 Scheduled Prompts\n\n");
                        for (i, sched) in schedules.iter().enumerate() {
                            let status_icon = match sched.status {
                                ScheduleStatus::Active => "🟢",
                                ScheduleStatus::Paused => "⏸️",
                                ScheduleStatus::Completed => "✅",
                                ScheduleStatus::Failed => "❌",
                                ScheduleStatus::Executing => "⏳",
                                ScheduleStatus::Cancelled => "🚫",
                            };
                            let schedule_type = if sched.cron_expression.is_some() {
                                "🔄 recurring"
                            } else {
                                "⏰ one-time"
                            };
                            msg.push_str(&format!(
                                "{}. {} {} [{}]\n   📝 {}\n   ⏱️ Next: {}\n   🆔 {}\n\n",
                                i + 1,
                                status_icon,
                                schedule_type,
                                sched.status,
                                truncate_str(&sched.prompt, 50),
                                &sched.next_execution_at[..16],
                                &sched.id[..8]
                            ));
                        }
                        msg.push_str("Commands: !schedule delete <id>, !schedule pause <id>, !schedule resume <id>");
                        room.send(RoomMessageEventContent::text_plain(&msg)).await?;
                    }
                }
                Some("delete") => {
                    let schedule_id = args.get(1);
                    match schedule_id {
                        Some(id) => {
                            // Find schedule by partial ID match
                            let schedules =
                                scheduler_store.list_by_room(room.room_id().as_str())?;
                            let matching: Vec<_> =
                                schedules.iter().filter(|s| s.id.starts_with(*id)).collect();
                            match matching.len() {
                                0 => {
                                    room.send(RoomMessageEventContent::text_plain(&format!(
                                        "No schedule found matching ID '{}'",
                                        id
                                    )))
                                    .await?;
                                }
                                1 => {
                                    scheduler_store.delete_schedule(&matching[0].id)?;
                                    room.send(RoomMessageEventContent::text_plain(&format!(
                                        "🗑️ Deleted schedule: {}",
                                        truncate_str(&matching[0].prompt, 50)
                                    )))
                                    .await?;
                                }
                                _ => {
                                    room.send(RoomMessageEventContent::text_plain(&format!(
                                        "Multiple schedules match '{}'. Be more specific.",
                                        id
                                    )))
                                    .await?;
                                }
                            }
                        }
                        None => {
                            room.send(RoomMessageEventContent::text_plain(
                                "Usage: !schedule delete <id>\nUse !schedule list to see IDs",
                            ))
                            .await?;
                        }
                    }
                }
                Some("pause") => {
                    let schedule_id = args.get(1);
                    match schedule_id {
                        Some(id) => {
                            let schedules =
                                scheduler_store.list_by_room(room.room_id().as_str())?;
                            let matching: Vec<_> = schedules
                                .iter()
                                .filter(|s| {
                                    s.id.starts_with(*id) && s.status == ScheduleStatus::Active
                                })
                                .collect();
                            match matching.len() {
                                0 => {
                                    room.send(RoomMessageEventContent::text_plain(&format!(
                                        "No active schedule found matching ID '{}'",
                                        id
                                    )))
                                    .await?;
                                }
                                1 => {
                                    scheduler_store.pause_schedule(&matching[0].id)?;
                                    room.send(RoomMessageEventContent::text_plain(&format!(
                                        "⏸️ Paused schedule: {}",
                                        truncate_str(&matching[0].prompt, 50)
                                    )))
                                    .await?;
                                }
                                _ => {
                                    room.send(RoomMessageEventContent::text_plain(&format!(
                                        "Multiple schedules match '{}'. Be more specific.",
                                        id
                                    )))
                                    .await?;
                                }
                            }
                        }
                        None => {
                            room.send(RoomMessageEventContent::text_plain(
                                "Usage: !schedule pause <id>\nUse !schedule list to see IDs",
                            ))
                            .await?;
                        }
                    }
                }
                Some("resume") => {
                    let schedule_id = args.get(1);
                    match schedule_id {
                        Some(id) => {
                            let schedules =
                                scheduler_store.list_by_room(room.room_id().as_str())?;
                            let matching: Vec<_> = schedules
                                .iter()
                                .filter(|s| {
                                    s.id.starts_with(*id) && s.status == ScheduleStatus::Paused
                                })
                                .collect();
                            match matching.len() {
                                0 => {
                                    room.send(RoomMessageEventContent::text_plain(&format!(
                                        "No paused schedule found matching ID '{}'",
                                        id
                                    )))
                                    .await?;
                                }
                                1 => {
                                    scheduler_store.resume_schedule(&matching[0].id)?;
                                    room.send(RoomMessageEventContent::text_plain(&format!(
                                        "▶️ Resumed schedule: {}",
                                        truncate_str(&matching[0].prompt, 50)
                                    )))
                                    .await?;
                                }
                                _ => {
                                    room.send(RoomMessageEventContent::text_plain(&format!(
                                        "Multiple schedules match '{}'. Be more specific.",
                                        id
                                    )))
                                    .await?;
                                }
                            }
                        }
                        None => {
                            room.send(RoomMessageEventContent::text_plain(
                                "Usage: !schedule resume <id>\nUse !schedule list to see IDs",
                            ))
                            .await?;
                        }
                    }
                }
                Some("export") => {
                    // Export schedules to .gorp/schedule.yaml
                    let schedules = scheduler_store.list_by_room(room.room_id().as_str())?;
                    let active_schedules: Vec<_> = schedules
                        .iter()
                        .filter(|s| matches!(s.status, ScheduleStatus::Active | ScheduleStatus::Paused))
                        .collect();

                    if active_schedules.is_empty() {
                        room.send(RoomMessageEventContent::text_plain(
                            "📅 No schedules to export.",
                        ))
                        .await?;
                        return Ok(());
                    }

                    // Build YAML content
                    let mut yaml_content = String::from("# Gorp Schedule Export\n# Import with: !schedule import\n\nschedules:\n");
                    for sched in &active_schedules {
                        let time_str = if let Some(ref cron) = sched.cron_expression {
                            cron.clone()
                        } else if let Some(ref exec_at) = sched.execute_at {
                            exec_at.clone()
                        } else {
                            sched.next_execution_at.clone()
                        };
                        let status = if matches!(sched.status, ScheduleStatus::Paused) {
                            "paused"
                        } else {
                            "active"
                        };
                        // Use YAML literal block style (|) for prompts to handle special chars safely
                        let needs_literal =
                            sched.prompt.contains(':') ||
                            sched.prompt.contains('#') ||
                            sched.prompt.contains('\n') ||
                            sched.prompt.contains('"') ||
                            sched.prompt.contains('\'') ||
                            sched.prompt.contains('[') ||
                            sched.prompt.contains(']') ||
                            sched.prompt.contains('{') ||
                            sched.prompt.contains('}');
                        if needs_literal {
                            // Use literal block style with proper indentation
                            let indented_prompt = sched.prompt
                                .lines()
                                .map(|line| format!("      {}", line))
                                .collect::<Vec<_>>()
                                .join("\n");
                            yaml_content.push_str(&format!(
                                "  - time: \"{}\"\n    prompt: |\n{}\n    status: {}\n",
                                time_str, indented_prompt, status
                            ));
                        } else {
                            yaml_content.push_str(&format!(
                                "  - time: \"{}\"\n    prompt: \"{}\"\n    status: {}\n",
                                time_str, sched.prompt, status
                            ));
                        }
                    }

                    // Write to .gorp/schedule.yaml
                    let gorp_dir = std::path::Path::new(&channel.directory).join(".gorp");
                    if let Err(e) = std::fs::create_dir_all(&gorp_dir) {
                        room.send(RoomMessageEventContent::text_plain(&format!(
                            "⚠️ Failed to create .gorp directory: {}",
                            e
                        )))
                        .await?;
                        return Ok(());
                    }
                    let schedule_path = gorp_dir.join("schedule.yaml");
                    if let Err(e) = std::fs::write(&schedule_path, &yaml_content) {
                        room.send(RoomMessageEventContent::text_plain(&format!(
                            "⚠️ Failed to write schedule.yaml: {}",
                            e
                        )))
                        .await?;
                        return Ok(());
                    }

                    room.send(RoomMessageEventContent::text_plain(&format!(
                        "📤 Exported {} schedule(s) to .gorp/schedule.yaml",
                        active_schedules.len()
                    )))
                    .await?;
                }
                Some("import") => {
                    // Import schedules from .gorp/schedule.yaml
                    let schedule_path = std::path::Path::new(&channel.directory)
                        .join(".gorp")
                        .join("schedule.yaml");

                    if !schedule_path.exists() {
                        room.send(RoomMessageEventContent::text_plain(
                            "📥 No schedule.yaml found in .gorp/ directory.\nCreate one manually or use !schedule export first.",
                        ))
                        .await?;
                        return Ok(());
                    }

                    let yaml_content = match std::fs::read_to_string(&schedule_path) {
                        Ok(content) => content,
                        Err(e) => {
                            room.send(RoomMessageEventContent::text_plain(&format!(
                                "⚠️ Failed to read schedule.yaml: {}",
                                e
                            )))
                            .await?;
                            return Ok(());
                        }
                    };

                    // Parse YAML (handles both inline and literal block style prompts)
                    let mut imported_count = 0;
                    let mut errors: Vec<String> = Vec::new();
                    let mut current_time: Option<String> = None;
                    let mut current_prompt: Option<String> = None;
                    let mut current_status = "active";
                    let mut in_literal_block = false;
                    let mut literal_indent: usize = 0; // Minimum indent of literal block
                    let mut literal_lines: Vec<String> = Vec::new();

                    for line in yaml_content.lines() {
                        let trimmed = line.trim();

                        // Handle literal block continuation
                        if in_literal_block {
                            // Calculate current line's leading spaces
                            let leading_spaces = line.len() - line.trim_start().len();

                            // Empty lines are preserved in literal blocks
                            if trimmed.is_empty() {
                                literal_lines.push(String::new());
                                continue;
                            }

                            // On first content line, detect the indent level
                            if literal_indent == 0 && !trimmed.is_empty() {
                                literal_indent = leading_spaces;
                            }

                            // Continue if line is indented at least as much as the block
                            if leading_spaces >= literal_indent && literal_indent > 0 {
                                // Strip the block's base indentation
                                literal_lines.push(line[literal_indent..].to_string());
                                continue;
                            } else if !trimmed.is_empty() {
                                // Non-empty line with less indent = end of block
                                in_literal_block = false;
                                current_prompt = Some(literal_lines.join("\n").trim_end().to_string());
                                literal_lines.clear();
                                literal_indent = 0;
                            }
                        }

                        if trimmed.starts_with("- time:") {
                            // Save previous schedule if complete
                            if let (Some(time), Some(prompt)) = (current_time.take(), current_prompt.take()) {
                                match import_schedule(
                                    &time,
                                    &prompt,
                                    current_status == "paused",
                                    &channel,
                                    sender,
                                    &config.scheduler.timezone,
                                    scheduler_store,
                                ) {
                                    Ok(_) => imported_count += 1,
                                    Err(e) => errors.push(format!("'{}': {}", truncate_str(&prompt, 20), e)),
                                }
                            }
                            current_status = "active";
                            // Parse time value
                            let time_val = trimmed.strip_prefix("- time:").unwrap().trim();
                            current_time = Some(time_val.trim_matches('"').to_string());
                        } else if trimmed.starts_with("time:") {
                            let time_val = trimmed.strip_prefix("time:").unwrap().trim();
                            current_time = Some(time_val.trim_matches('"').to_string());
                        } else if trimmed.starts_with("prompt:") {
                            let prompt_val = trimmed.strip_prefix("prompt:").unwrap().trim();
                            if prompt_val == "|" {
                                // Start of literal block style
                                in_literal_block = true;
                                literal_lines.clear();
                                literal_indent = 0;
                            } else {
                                // Inline prompt value
                                current_prompt = Some(prompt_val.trim_matches('"').replace("\\\"", "\""));
                            }
                        } else if trimmed.starts_with("status:") {
                            let status_val = trimmed.strip_prefix("status:").unwrap().trim();
                            current_status = status_val.trim();
                        }
                    }

                    // Handle any remaining literal block
                    if in_literal_block && !literal_lines.is_empty() {
                        current_prompt = Some(literal_lines.join("\n").trim_end().to_string());
                    }

                    // Don't forget the last one
                    if let (Some(time), Some(prompt)) = (current_time.take(), current_prompt.take()) {
                        match import_schedule(
                            &time,
                            &prompt,
                            current_status == "paused",
                            &channel,
                            sender,
                            &config.scheduler.timezone,
                            scheduler_store,
                        ) {
                            Ok(_) => imported_count += 1,
                            Err(e) => errors.push(format!("'{}': {}", truncate_str(&prompt, 20), e)),
                        }
                    }

                    let mut msg = format!("📥 Imported {} schedule(s)", imported_count);
                    if !errors.is_empty() {
                        msg.push_str(&format!("\n\n⚠️ {} error(s):\n", errors.len()));
                        for err in errors.iter().take(5) {
                            msg.push_str(&format!("  • {}\n", err));
                        }
                        if errors.len() > 5 {
                            msg.push_str(&format!("  ... and {} more\n", errors.len() - 5));
                        }
                    }
                    room.send(RoomMessageEventContent::text_plain(&msg)).await?;
                }
                _ => {
                    // Default: create a schedule
                    // Parse time expression from the beginning of args
                    if args.is_empty() {
                        room.send(RoomMessageEventContent::text_plain(
                            "Usage: !schedule <time> <prompt>\n\nExamples:\n  !schedule in 2 hours check my inbox\n  !schedule tomorrow 9am summarize my calendar\n  !schedule every monday 8am weekly standup\n\nOther commands:\n  !schedule list\n  !schedule delete <id>\n  !schedule pause <id>\n  !schedule resume <id>\n  !schedule export\n  !schedule import",
                        ))
                        .await?;
                        return Ok(());
                    }

                    // Try to parse time expression greedily from start
                    let full_args = args.join(" ");
                    let (parsed_schedule, prompt) =
                        parse_schedule_input(&full_args, &config.scheduler.timezone)?;

                    if prompt.is_empty() {
                        room.send(RoomMessageEventContent::text_plain(
                            "Missing prompt. Usage: !schedule <time> <prompt>",
                        ))
                        .await?;
                        return Ok(());
                    }

                    // Create the schedule
                    let schedule_id = uuid::Uuid::new_v4().to_string();
                    let now = Utc::now().to_rfc3339();

                    let (execute_at, cron_expr, next_exec) = match &parsed_schedule {
                        ParsedSchedule::OneTime(dt) => {
                            (Some(dt.to_rfc3339()), None, dt.to_rfc3339())
                        }
                        ParsedSchedule::Recurring { cron, next } => {
                            (None, Some(cron.clone()), next.to_rfc3339())
                        }
                    };

                    let scheduled_prompt = ScheduledPrompt {
                        id: schedule_id.clone(),
                        channel_name: channel.channel_name.clone(),
                        room_id: room.room_id().to_string(),
                        prompt: prompt.clone(),
                        created_by: sender.to_string(),
                        created_at: now,
                        execute_at,
                        cron_expression: cron_expr.clone(),
                        last_executed_at: None,
                        next_execution_at: next_exec.clone(),
                        status: ScheduleStatus::Active,
                        error_message: None,
                        execution_count: 0,
                    };

                    scheduler_store.create_schedule(&scheduled_prompt)?;

                    let schedule_type = if cron_expr.is_some() {
                        "🔄 Recurring schedule"
                    } else {
                        "⏰ One-time schedule"
                    };

                    room.send(RoomMessageEventContent::text_plain(&format!(
                        "{} created!\n\n📝 Prompt: {}\n⏱️ Next execution: {} ({})\n🆔 ID: {}",
                        schedule_type,
                        truncate_str(&prompt, 100),
                        &next_exec[..16],
                        &config.scheduler.timezone,
                        &schedule_id[..8]
                    )))
                    .await?;

                    tracing::info!(
                        schedule_id = %schedule_id,
                        channel = %channel.channel_name,
                        next_exec = %next_exec,
                        "Schedule created"
                    );
                }
            }
        }
        "reset" => {
            // Reset Claude session for this channel (generates new session ID)
            if is_dm {
                room.send(RoomMessageEventContent::text_plain(
                    "❌ The !reset command only works in channel rooms.",
                ))
                .await?;
                return Ok(());
            }

            let Some(channel) = session_store.get_by_room(room.room_id().as_str())? else {
                room.send(RoomMessageEventContent::text_plain(
                    "No channel attached to this room.",
                ))
                .await?;
                return Ok(());
            };

            // Generate new session ID and reset started flag
            let new_session_id = uuid::Uuid::new_v4().to_string();
            session_store.reset_session(&channel.channel_name, &new_session_id)?;

            room.send(RoomMessageEventContent::text_plain(&format!(
                "🔄 Session Reset\n\n\
                Channel: {}\n\
                New Session ID: {}\n\n\
                Claude will start fresh on next message.\n\
                MCP tools and settings will be reloaded.",
                channel.channel_name,
                &new_session_id[..8]
            )))
            .await?;

            tracing::info!(
                channel = %channel.channel_name,
                old_session = %channel.session_id,
                new_session = %new_session_id,
                "Session reset by user"
            );
        }
        _ => {
            let help_msg = if is_dm {
                "Unknown command. Available commands:\n\
                !create <name> - Create new channel\n\
                !join <name> - Get invited to channel\n\
                !delete <name> - Remove channel\n\
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
            room.send(RoomMessageEventContent::text_plain(help_msg))
                .await?;
        }
    }

    Ok(())
}

/// Truncate a string to max_len characters, adding "..." if truncated
/// Uses character-based slicing to avoid UTF-8 boundary panics
fn truncate_str(s: &str, max_len: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max_len {
        s.to_string()
    } else {
        let truncated: String = chars[..max_len.saturating_sub(3)].iter().collect();
        format!("{}...", truncated)
    }
}

/// Check if a string looks like a cron expression (5 fields: minute hour day month weekday)
/// This is a heuristic, not strict validation - invalid cron expressions will be caught
/// by the cron parser later with a proper error message.
fn looks_like_cron(s: &str) -> bool {
    let parts: Vec<&str> = s.split_whitespace().collect();
    // Cron has 5 fields, each containing digits, *, -, /, or ,
    parts.len() == 5
        && parts.iter().all(|p| {
            p.chars()
                .all(|c| c.is_ascii_digit() || c == '*' || c == '-' || c == '/' || c == ',')
        })
}

/// Import a single schedule from YAML data
fn import_schedule(
    time: &str,
    prompt: &str,
    paused: bool,
    channel: &crate::session::Channel,
    sender: &str,
    timezone: &str,
    scheduler_store: &SchedulerStore,
) -> anyhow::Result<()> {
    // Check if time is a raw cron expression (exported from recurring schedule)
    let parsed = if looks_like_cron(time) {
        // Parse as raw cron expression
        use crate::scheduler::compute_next_cron_execution_in_tz;
        let next = compute_next_cron_execution_in_tz(time, timezone)?;
        ParsedSchedule::Recurring {
            cron: time.to_string(),
            next,
        }
    } else {
        // Try parsing as natural language time expression
        parse_time_expression(time, timezone)?
    };

    let schedule_id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();

    let (execute_at, cron_expr, next_exec) = match &parsed {
        ParsedSchedule::OneTime(dt) => (Some(dt.to_rfc3339()), None, dt.to_rfc3339()),
        ParsedSchedule::Recurring { cron, next } => (None, Some(cron.clone()), next.to_rfc3339()),
    };

    let status = if paused {
        ScheduleStatus::Paused
    } else {
        ScheduleStatus::Active
    };

    let scheduled_prompt = ScheduledPrompt {
        id: schedule_id,
        channel_name: channel.channel_name.clone(),
        room_id: channel.room_id.clone(),
        prompt: prompt.to_string(),
        created_by: sender.to_string(),
        created_at: now,
        execute_at,
        cron_expression: cron_expr,
        last_executed_at: None,
        next_execution_at: next_exec,
        status,
        error_message: None,
        execution_count: 0,
    };

    scheduler_store.create_schedule(&scheduled_prompt)?;
    Ok(())
}

/// Parse schedule input to extract time expression and prompt
/// Uses greedy matching with a max lookahead to avoid consuming the entire prompt
fn parse_schedule_input(input: &str, timezone: &str) -> anyhow::Result<(ParsedSchedule, String)> {
    let words: Vec<&str> = input.split_whitespace().collect();

    // Require at least 1 word for prompt, limit time expression to 10 words max
    let max_time_words = std::cmp::min(words.len().saturating_sub(1), 10);

    // Try progressively longer prefixes until parsing fails
    let mut last_valid: Option<(ParsedSchedule, usize)> = None;

    for end_idx in 1..=max_time_words {
        let time_expr = words[..end_idx].join(" ");
        if let Ok(schedule) = parse_time_expression(&time_expr, timezone) {
            last_valid = Some((schedule, end_idx));
        }
    }

    match last_valid {
        Some((schedule, word_count)) => {
            let prompt = words[word_count..].join(" ");
            Ok((schedule, prompt))
        }
        None => anyhow::bail!(
            "Could not parse time expression. Try: 'in 2 hours', 'tomorrow 9am', 'every monday 8am'"
        ),
    }
}
