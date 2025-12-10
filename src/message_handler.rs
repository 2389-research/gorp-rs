// ABOUTME: Message handler that processes Matrix room messages with authentication and Claude invocation.
// ABOUTME: Checks room ID and user whitelist, manages typing indicators, and handles Claude response delivery.

use anyhow::Result;
use matrix_sdk::{
    room::Room, ruma::events::room::message::RoomMessageEventContent, Client, RoomState,
};

use crate::{claude, config::Config, matrix_client, session::SessionStore};

pub async fn handle_message(
    room: Room,
    event: matrix_sdk::ruma::events::room::message::OriginalSyncRoomMessageEvent,
    client: Client,
    config: Config,
    session_store: SessionStore,
) -> Result<()> {
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
        return handle_command(room, body, &session_store, &client, sender, is_dm, &config).await;
    }

    // Check if channel is attached
    let Some(channel) = session_store.get_by_room(room.room_id().as_str())? else {
        let help_msg = if is_dm {
            "👋 Welcome! To get started, create a channel:\n\n\
            !create <channel-name>\n\n\
            Example: !create PA\n\
            This creates a dedicated Claude session in a workspace directory."
        } else {
            "No Claude channel attached to this room.\n\n\
            💡 DM me to create a channel with: !create <name>\n\n\
            Need help? Send: !help"
        };
        room.send(RoomMessageEventContent::text_plain(help_msg))
            .await?;
        return Ok(());
    };

    let channel_args = channel.cli_args();

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

    // Invoke Claude
    let response = match claude::invoke_claude(
        &config.claude.binary_path,
        config.claude.sdk_url.as_deref(),
        channel_args,
        body,
        Some(&channel.directory),
    )
    .await
    {
        Ok(resp) => {
            tracing::info!(response_length = resp.len(), "Claude responded");
            resp
        }
        Err(e) => {
            tracing::error!(error = %e, "Claude invocation failed");
            let error_msg = format!("⚠️ Claude error: {}", e);

            // Stop typing indicator refresh
            let _ = typing_tx.send(());
            typing_handle.abort();
            room.typing_notice(false).await?;

            room.send(RoomMessageEventContent::text_plain(&error_msg))
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

    // Send response
    room.send(RoomMessageEventContent::text_plain(&response))
        .await?;

    tracing::info!("Response sent successfully");

    Ok(())
}

async fn handle_command(
    room: Room,
    body: &str,
    session_store: &SessionStore,
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
            !create <name> - Create new channel (e.g., !create PA)\n\
            !list - Show all your channels\n\
            !help - Show detailed help"
        } else {
            "Available commands:\n\
            !help - Show detailed help\n\
            !status - Show current channel info"
        };
        room.send(RoomMessageEventContent::text_plain(help_msg))
            .await?;
        return Ok(());
    }

    let command = command_parts[0];

    match command {
        "help" => {
            let help_text = format!(
                "📚 Claude Channel System Help\n\n\
                ## What are Channels?\n\
                Channels are persistent Claude conversations backed by workspace directories.\n\
                Each channel has its own Matrix room, Claude session, and working directory.\n\n\
                ## DM Commands (Orchestrator)\n\
                !create <name> - Create a new channel\n\
                  Example: !create PA\n\
                  Creates: workspace/PA/ directory + Matrix room + Claude session\n\n\
                !list - Show all your channels\n\
                !help - Show this help message\n\n\
                ## Room Commands\n\
                !status - Show current channel info\n\
                !help - Show this help message\n\n\
                ## How It Works\n\
                1. DM the bot: !create PA\n\
                2. Bot creates:\n\
                   - workspace/PA/ directory\n\
                   - Matrix room named \"{}: PA\"\n\
                   - Persistent Claude session\n\
                3. Join the room and start chatting!\n\
                4. All conversation history is preserved\n\n\
                ## Webhook Support\n\
                Each channel has a webhook URL for external triggers:\n\
                  POST http://{}:{}/webhook/session/<session-id>\n\
                  {{\"prompt\": \"your message here\"}}\n\n\
                Use this for scheduled tasks, cron jobs, etc.\n\n\
                ## Features\n\
                ✅ Persistent conversation history\n\
                ✅ Dedicated workspace per channel\n\
                ✅ Smart session reuse\n\
                ✅ Webhook integration for automation\n\
                ✅ One channel = one ongoing conversation\n\n\
                Need more help? Just ask!",
                config.matrix.room_prefix, config.webhook.host, config.webhook.port
            );

            room.send(RoomMessageEventContent::text_plain(&help_text))
                .await?;
        }
        "status" => {
            if let Some(channel) = session_store.get_by_room(room.room_id().as_str())? {
                let status = format!(
                    "📊 Channel Status\n\n\
                    Channel: {}\n\
                    Session ID: {}\n\
                    Directory: {}\n\
                    Started: {}\n\n\
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

            let channel_name = command_parts[1].to_string();

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

            // Check if channel already exists
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

            // Invite user
            matrix_client::invite_user(client, &new_room_id, sender).await?;

            // Create channel in database (this also creates the directory)
            let channel = session_store.create_channel(&channel_name, new_room_id.as_str())?;

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
        "list" => {
            let channels = session_store.list_all()?;

            if channels.is_empty() {
                let msg = if is_dm {
                    "📋 No Channels Yet\n\nCreate one with: !create <name>\n\nExample: !create PA"
                } else {
                    "📋 No Channels Found\n\nDM me to create a channel!"
                };
                room.send(RoomMessageEventContent::text_plain(msg)).await?;
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
        _ => {
            let help_msg = if is_dm {
                "Unknown command. Available commands:\n\
                !create <name> - Create new channel\n\
                !list - Show all channels\n\
                !help - Show detailed help"
            } else {
                "Unknown command. Available commands:\n\
                !status - Show channel info\n\
                !help - Show detailed help"
            };
            room.send(RoomMessageEventContent::text_plain(help_msg))
                .await?;
        }
    }

    Ok(())
}
