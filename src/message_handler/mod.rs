// ABOUTME: Message handler that processes Matrix room messages with authentication and Claude invocation.
// ABOUTME: Checks room ID and user whitelist, manages typing indicators, streams tool usage to Matrix.

// Submodules
pub mod attachments;
pub mod chat;
pub mod commands;
pub mod context;
pub mod helpers;
pub mod matrix_commands;
pub mod schedule_import;
pub mod traits;

// Re-exports from submodules for backward compatibility
pub use attachments::download_attachment;
pub use context::{route_to_dispatch, write_context_file};
pub use helpers::{is_debug_enabled, looks_like_cron, truncate_str, validate_channel_name};
pub use schedule_import::parse_schedule_input;
pub use traits::{ChannelAdapter, MatrixRoom, MessageSender, MockRoom};

use anyhow::Result;
use matrix_sdk::{
    room::Room,
    ruma::events::room::message::RoomMessageEventContent,
    Client, RoomState,
};

use crate::{
    commands::{parse_message, Command, ParseResult},
    config::Config,
    matrix_client, metrics, onboarding,
    scheduler::SchedulerStore,
    session::SessionStore,
    utils::markdown_to_html,
    warm_session::SharedWarmSessionManager,
};

pub async fn handle_message(
    room: Room,
    event: matrix_sdk::ruma::events::room::message::OriginalSyncRoomMessageEvent,
    client: Client,
    config: Config,
    session_store: SessionStore,
    scheduler_store: SchedulerStore,
    warm_manager: SharedWarmSessionManager,
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

    // Parse message using gorp-core command parsing
    let parse_result = parse_message(body, "!claude");

    if let ParseResult::Command(cmd) = parse_result {
        metrics::record_message_received("command");
        let result = handle_command(
            room,
            &cmd,
            &session_store,
            &scheduler_store,
            &client,
            sender,
            is_dm,
            &config,
            &warm_manager,
        )
        .await;
        let duration = start_time.elapsed().as_secs_f64();
        metrics::record_message_processing_duration(duration);
        return result;
    }

    // Check for escape sequence (!! prefix) - treat as regular message
    if let ParseResult::Ignore = parse_result {
        return Ok(());
    }

    // Check if this is the DISPATCH control plane room (only in DMs)
    if is_dm {
        // Check for existing DISPATCH channel
        if session_store
            .get_dispatch_channel(room.room_id().as_str())?
            .is_some()
        {
            metrics::record_message_received("dispatch");
            return crate::dispatch_handler::handle_dispatch_message(
                room,
                event,
                client,
                config,
                session_store,
                warm_manager,
            )
            .await;
        }

        // Check for DISPATCH activation command
        let body_lower = body.to_lowercase();
        if body_lower.starts_with("!dispatch") || body_lower == "dispatch" {
            // Create DISPATCH channel and route to handler
            tracing::info!(room_id = %room.room_id(), "DISPATCH channel activated via command");
            metrics::record_message_received("dispatch");
            session_store.create_dispatch_channel(room.room_id().as_str())?;
            return crate::dispatch_handler::handle_dispatch_message(
                room,
                event,
                client,
                config,
                session_store,
                warm_manager,
            )
            .await;
        }
    }

    // Regular chat message
    metrics::record_message_received("chat");

    // Check if channel is attached
    let Some(channel) = session_store.get_by_room(room.room_id().as_str())? else {
        // In DM, check for onboarding flow
        if is_dm {
            let user_id = sender;

            // Check if we're in the middle of onboarding and waiting for a channel name
            if onboarding::is_waiting_for_channel_name(&session_store, user_id)? {
                // User is providing a channel name - validate and create
                let channel_name = body.trim().to_lowercase();

                // Handle special responses
                let msg_lower = channel_name.to_lowercase();
                if msg_lower == "done" || msg_lower == "skip" {
                    // Complete onboarding without creating a channel
                    let mut state =
                        onboarding::get_state(&session_store, user_id)?.unwrap_or_default();
                    state.step = onboarding::OnboardingStep::Completed;
                    onboarding::save_state(&session_store, user_id, &state)?;

                    let msg =
                        "Alright! You can create a channel anytime with `!create <name>`.\n\n\
                        Type `!help` for all commands.";
                    let html = markdown_to_html(msg);
                    room.send(RoomMessageEventContent::text_html(msg, &html))
                        .await?;
                    return Ok(());
                }

                // Validate channel name
                if channel_name.is_empty()
                    || !channel_name
                        .chars()
                        .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
                    || channel_name.len() > 50
                {
                    let msg = "Channel names can only contain letters, numbers, dashes, and underscores.\n\
                        Try something like `pa` or `my-project`.";
                    room.send(RoomMessageEventContent::text_plain(msg)).await?;
                    return Ok(());
                }

                // Check if channel already exists
                if session_store.get_by_name(&channel_name)?.is_some() {
                    let msg = format!(
                        "A channel named `{}` already exists! Try a different name.",
                        channel_name
                    );
                    room.send(RoomMessageEventContent::text_plain(&msg)).await?;
                    return Ok(());
                }

                // Create Matrix room
                let room_name = format!("{}: {}", config.matrix.room_prefix, channel_name);
                let new_room_id = match matrix_client::create_room(&client, &room_name).await {
                    Ok(id) => id,
                    Err(e) => {
                        let msg = format!("Failed to create Matrix room: {}", e);
                        room.send(RoomMessageEventContent::text_plain(&msg)).await?;
                        return Ok(());
                    }
                };
                metrics::record_room_created();

                // Invite user
                if let Err(e) = matrix_client::invite_user(&client, &new_room_id, sender).await {
                    tracing::warn!(error = %e, "Failed to invite user to channel");
                }

                // Create channel in database (this also creates the directory)
                let channel =
                    match session_store.create_channel(&channel_name, new_room_id.as_str()) {
                        Ok(c) => c,
                        Err(e) => {
                            let msg = format!("Failed to create channel: {}", e);
                            room.send(RoomMessageEventContent::text_plain(&msg)).await?;
                            return Ok(());
                        }
                    };
                metrics::increment_active_channels();

                tracing::info!(
                    channel = %channel_name,
                    room_id = %new_room_id,
                    directory = %channel.directory,
                    "Channel created during onboarding"
                );

                // Complete onboarding
                onboarding::complete(
                    &room,
                    &session_store,
                    user_id,
                    &channel_name,
                    &channel.directory,
                )
                .await?;
                return Ok(());
            }

            // Check if user should go through onboarding
            if onboarding::should_onboard(&session_store, user_id)? {
                // Try to handle as onboarding response, or start fresh
                if onboarding::handle_message(&room, &session_store, user_id, body).await? {
                    return Ok(()); // Message was handled by onboarding
                }
                // No active onboarding state - start fresh
                onboarding::start(&room, &session_store, user_id).await?;
                return Ok(());
            }

            // Not in onboarding - auto-create DISPATCH channel and route there
            // This gives users a natural chat experience in DMs
            tracing::info!(room_id = %room.room_id(), user = %user_id, "Auto-creating DISPATCH channel for DM");
            session_store.create_dispatch_channel(room.room_id().as_str())?;
            metrics::record_message_received("dispatch");
            return crate::dispatch_handler::handle_dispatch_message(
                room,
                event,
                client,
                config,
                session_store,
                warm_manager,
            )
            .await;
        } else {
            let help_msg = "No Claude channel attached to this room.\n\n\
                💡 DM me to create a channel with: !create <name>\n\n\
                Need help? Send: !help";
            let help_html = markdown_to_html(help_msg);
            room.send(RoomMessageEventContent::text_html(help_msg, &help_html))
                .await?;
            return Ok(());
        }
    };

    // Delegate to chat module for actual Claude invocation and response streaming
    chat::process_chat_message(
        room,
        event,
        client,
        channel,
        session_store,
        warm_manager,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn handle_command(
    room: Room,
    cmd: &Command,
    session_store: &SessionStore,
    scheduler_store: &SchedulerStore,
    client: &Client,
    sender: &str,
    is_dm: bool,
    config: &Config,
    warm_manager: &SharedWarmSessionManager,
) -> Result<()> {
    // Wrap Room in MatrixRoom for testable command handler
    let matrix_room = MatrixRoom::new(room.clone());

    // Try the testable command handler first
    match commands::handle_command(
        &matrix_room,
        cmd,
        session_store,
        scheduler_store,
        Some(client),
        sender,
        is_dm,
        config,
        warm_manager,
    )
    .await
    {
        Ok(()) => return Ok(()),
        Err(e) => {
            let err_msg = e.to_string();
            // Check if this is a delegation request
            if !err_msg.starts_with("DELEGATE_TO_MATRIX:") {
                // Real error, propagate it
                return Err(e);
            }
            // Fall through to handle Matrix-dependent commands below
        }
    }

    // Command name and args are already parsed by gorp-core
    let command = cmd.name.as_str();
    let command_parts: Vec<&str> = std::iter::once(command)
        .chain(cmd.args.iter().map(|s| s.as_str()))
        .collect();

    // Delegate to matrix_commands module for Matrix-dependent command handling
    matrix_commands::handle_matrix_command(
        &room,
        command,
        &command_parts,
        session_store,
        scheduler_store,
        client,
        sender,
        is_dm,
        config,
        warm_manager,
    )
    .await
}
