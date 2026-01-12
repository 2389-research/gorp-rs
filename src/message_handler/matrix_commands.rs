// ABOUTME: Matrix-dependent command handler for commands requiring Matrix client operations.
// ABOUTME: Handles setup, create, join, delete, schedule, cleanup, etc. that need room/client access.

use anyhow::Result;
use matrix_sdk::{room::Room, ruma::events::room::message::RoomMessageEventContent, Client};

use crate::{
    config::Config,
    matrix_client, metrics, onboarding,
    scheduler::{
        parse_time_expression, ParsedSchedule, ScheduleStatus, ScheduledPrompt, SchedulerStore,
    },
    session::SessionStore,
    warm_session::SharedWarmSessionManager,
};

use super::helpers::{looks_like_cron, truncate_str};
use super::schedule_import::parse_schedule_input;

use chrono::Utc;

/// Handle Matrix-dependent commands that were delegated from the testable command handler.
///
/// These commands require access to the Matrix client for room operations,
/// invitations, and other Matrix-specific functionality.
#[allow(clippy::too_many_arguments)]
pub async fn handle_matrix_command(
    room: &Room,
    command: &str,
    command_parts: &[&str],
    session_store: &SessionStore,
    scheduler_store: &SchedulerStore,
    client: &Client,
    sender: &str,
    is_dm: bool,
    config: &Config,
    warm_manager: &SharedWarmSessionManager,
) -> Result<()> {
    match command {
        "setup" => {
            // Onboarding wizard - only works in DMs
            if !is_dm {
                room.send(RoomMessageEventContent::text_plain(
                    "The !setup command only works in DMs. DM me to run the setup wizard.",
                ))
                .await?;
                return Ok(());
            }

            // Reset and start onboarding
            onboarding::reset_and_start(room, session_store, sender).await?;
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
                room.send(RoomMessageEventContent::text_plain(format!(
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
                room.send(RoomMessageEventContent::text_plain(format!(
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
                    room.send(RoomMessageEventContent::text_plain(format!(
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
                        room.send(RoomMessageEventContent::text_plain(format!(
                            "ℹ️ You're already in channel '{}'!",
                            channel_name
                        )))
                        .await?;
                    } else {
                        room.send(RoomMessageEventContent::text_plain(format!(
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
                room.send(RoomMessageEventContent::text_plain(format!(
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
        "reset" if is_dm => {
            // DM command: !reset <channel_name>
            if command_parts.len() < 2 {
                room.send(RoomMessageEventContent::text_plain(
                    "Usage: !reset <channel-name>\n\n\
                    Resets the session for the specified channel.\n\
                    Use !list to see available channels.",
                ))
                .await?;
                return Ok(());
            }

            let channel_name = command_parts[1].to_lowercase();

            // Look up channel by name
            let Some(channel) = session_store.get_by_name(&channel_name)? else {
                room.send(RoomMessageEventContent::text_plain(format!(
                    "❌ Channel '{}' not found.\n\nUse !list to see all channels.",
                    channel_name
                )))
                .await?;
                return Ok(());
            };

            // Generate new session ID and reset started flag
            let new_session_id = uuid::Uuid::new_v4().to_string();
            session_store.reset_session(&channel.channel_name, &new_session_id)?;

            // Evict from warm session cache
            let evicted = {
                let mut mgr = warm_manager.write().await;
                mgr.evict(&channel.channel_name)
            };

            room.send(RoomMessageEventContent::text_plain(format!(
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
                evicted = evicted,
                "Session reset by user from DM"
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
                    room.send(RoomMessageEventContent::text_plain(format!(
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
                if dir_name == "template" || dir_name.starts_with('.') || dir_name == "attachments"
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
                        let invite_failed =
                            match matrix_client::invite_user(client, &new_room_id, sender).await {
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
                    msg.push_str(&format!("✅ Restored {} channel(s):\n", restored.len()));
                    for name in &restored {
                        msg.push_str(&format!("  • {}\n", name));
                    }
                    msg.push_str("\nCheck your room invites!\n");
                }
                if !skipped.is_empty() {
                    msg.push_str(&format!("\n⏭️ Skipped {} item(s):\n", skipped.len()));
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
                                    room.send(RoomMessageEventContent::text_plain(format!(
                                        "No schedule found matching ID '{}'",
                                        id
                                    )))
                                    .await?;
                                }
                                1 => {
                                    scheduler_store.delete_schedule(&matching[0].id)?;
                                    room.send(RoomMessageEventContent::text_plain(format!(
                                        "🗑️ Deleted schedule: {}",
                                        truncate_str(&matching[0].prompt, 50)
                                    )))
                                    .await?;
                                }
                                _ => {
                                    room.send(RoomMessageEventContent::text_plain(format!(
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
                                    room.send(RoomMessageEventContent::text_plain(format!(
                                        "No active schedule found matching ID '{}'",
                                        id
                                    )))
                                    .await?;
                                }
                                1 => {
                                    scheduler_store.pause_schedule(&matching[0].id)?;
                                    room.send(RoomMessageEventContent::text_plain(format!(
                                        "⏸️ Paused schedule: {}",
                                        truncate_str(&matching[0].prompt, 50)
                                    )))
                                    .await?;
                                }
                                _ => {
                                    room.send(RoomMessageEventContent::text_plain(format!(
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
                                    room.send(RoomMessageEventContent::text_plain(format!(
                                        "No paused schedule found matching ID '{}'",
                                        id
                                    )))
                                    .await?;
                                }
                                1 => {
                                    scheduler_store.resume_schedule(&matching[0].id)?;
                                    room.send(RoomMessageEventContent::text_plain(format!(
                                        "▶️ Resumed schedule: {}",
                                        truncate_str(&matching[0].prompt, 50)
                                    )))
                                    .await?;
                                }
                                _ => {
                                    room.send(RoomMessageEventContent::text_plain(format!(
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
                        .filter(|s| {
                            matches!(s.status, ScheduleStatus::Active | ScheduleStatus::Paused)
                        })
                        .collect();

                    if active_schedules.is_empty() {
                        room.send(RoomMessageEventContent::text_plain(
                            "📅 No schedules to export.",
                        ))
                        .await?;
                        return Ok(());
                    }

                    // Build YAML content
                    let mut yaml_content = String::from(
                        "# Gorp Schedule Export\n# Import with: !schedule import\n\nschedules:\n",
                    );
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
                        let needs_literal = sched.prompt.contains(':')
                            || sched.prompt.contains('#')
                            || sched.prompt.contains('\n')
                            || sched.prompt.contains('"')
                            || sched.prompt.contains('\'')
                            || sched.prompt.contains('[')
                            || sched.prompt.contains(']')
                            || sched.prompt.contains('{')
                            || sched.prompt.contains('}');
                        if needs_literal {
                            // Use literal block style with proper indentation
                            let indented_prompt = sched
                                .prompt
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
                        room.send(RoomMessageEventContent::text_plain(format!(
                            "⚠️ Failed to create .gorp directory: {}",
                            e
                        )))
                        .await?;
                        return Ok(());
                    }
                    let schedule_path = gorp_dir.join("schedule.yaml");
                    if let Err(e) = std::fs::write(&schedule_path, &yaml_content) {
                        room.send(RoomMessageEventContent::text_plain(format!(
                            "⚠️ Failed to write schedule.yaml: {}",
                            e
                        )))
                        .await?;
                        return Ok(());
                    }

                    room.send(RoomMessageEventContent::text_plain(format!(
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
                            room.send(RoomMessageEventContent::text_plain(format!(
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
                                current_prompt =
                                    Some(literal_lines.join("\n").trim_end().to_string());
                                literal_lines.clear();
                                literal_indent = 0;
                            }
                        }

                        if trimmed.starts_with("- time:") {
                            // Save previous schedule if complete
                            if let (Some(time), Some(prompt)) =
                                (current_time.take(), current_prompt.take())
                            {
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
                                    Err(e) => errors.push(format!(
                                        "'{}': {}",
                                        truncate_str(&prompt, 20),
                                        e
                                    )),
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
                                current_prompt =
                                    Some(prompt_val.trim_matches('"').replace("\\\"", "\""));
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
                    if let (Some(time), Some(prompt)) = (current_time.take(), current_prompt.take())
                    {
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
                            Err(e) => {
                                errors.push(format!("'{}': {}", truncate_str(&prompt, 20), e))
                            }
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

                    room.send(RoomMessageEventContent::text_plain(format!(
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
            // Note: DM resets with channel name argument are handled by "reset" if is_dm above
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

            // Evict from warm session cache
            let evicted = {
                let mut mgr = warm_manager.write().await;
                mgr.evict(&channel.channel_name)
            };

            room.send(RoomMessageEventContent::text_plain(format!(
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
                evicted = evicted,
                "Session reset by user"
            );
        }
        _ => {
            // This should not be reached - unknown commands are handled by commands module
            // and only specific delegated commands should reach here
            tracing::warn!(command = %command, "Unexpected command reached Matrix handler");
            return Err(anyhow::anyhow!(
                "Internal error: command '{}' was delegated but has no handler",
                command
            ));
        }
    }

    Ok(())
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
