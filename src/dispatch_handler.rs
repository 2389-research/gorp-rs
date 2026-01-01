// ABOUTME: DISPATCH control plane message handler for orchestrating workspace rooms.
// ABOUTME: Runs in 1:1 DM, provides cross-room visibility and task dispatch.

use anyhow::Result;
use gorp_agent::AgentEvent;
use matrix_sdk::{
    room::Room, ruma::events::room::message::RoomMessageEventContent, Client, RoomState,
};

use crate::{
    config::Config,
    dispatch_system_prompt::generate_dispatch_prompt,
    session::SessionStore,
    utils::{chunk_message, markdown_to_html, MAX_CHUNK_SIZE},
    warm_session::{SharedWarmSessionManager, WarmSessionManager},
};

/// Handle a message in the DISPATCH control plane room
///
/// DISPATCH is a special agent that:
/// - Runs in the 1:1 DM with the bot
/// - Has no filesystem workspace (pure coordination)
/// - Can query status of all workspace rooms
/// - Can dispatch tasks to worker rooms
/// - Receives events from worker rooms
pub async fn handle_dispatch_message(
    room: Room,
    event: matrix_sdk::ruma::events::room::message::OriginalSyncRoomMessageEvent,
    _client: Client,
    _config: Config,
    session_store: SessionStore,
    warm_manager: SharedWarmSessionManager,
) -> Result<()> {
    // Only work with joined rooms
    if room.state() != RoomState::Joined {
        return Ok(());
    }

    let body = event.content.body();

    tracing::info!(
        room_id = %room.room_id(),
        body_preview = %body.chars().take(50).collect::<String>(),
        "DISPATCH message received"
    );

    // Get or create DISPATCH channel
    let dispatch_channel = session_store.get_or_create_dispatch_channel(room.room_id().as_str())?;

    tracing::debug!(
        session_id = %dispatch_channel.session_id,
        "Using DISPATCH session"
    );

    // Mark as started if not already
    if !dispatch_channel.started {
        session_store.mark_started(room.room_id().as_str())?;
    }

    // Generate dynamic system prompt with current room state
    let system_prompt = generate_dispatch_prompt(&session_store);

    // Start typing indicator
    room.typing_notice(true).await?;

    // Spawn a task to keep the typing indicator refreshed every 25 seconds
    let typing_room = room.clone();
    let typing_room_id = room.room_id().to_string();
    let (typing_tx, mut typing_rx) = tokio::sync::oneshot::channel();
    let typing_handle = tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(25));
        let max_duration = tokio::time::Instant::now() + tokio::time::Duration::from_secs(300); // 5 min max for DISPATCH
        interval.tick().await; // Skip first immediate tick

        loop {
            if tokio::time::Instant::now() > max_duration {
                tracing::warn!(room_id = %typing_room_id, "DISPATCH typing indicator timed out");
                break;
            }
            tokio::select! {
                _ = interval.tick() => {
                    if let Err(e) = typing_room.typing_notice(true).await {
                        tracing::warn!(room_id = %typing_room_id, error = %e, "Failed to refresh typing indicator");
                        break;
                    }
                }
                _ = &mut typing_rx => {
                    break;
                }
            }
        }
    });

    // Create mux backend for DISPATCH using the config from warm_manager
    // DISPATCH uses mux backend (pure API, no CLI) with no workspace directory
    let (warm_config, registry) = {
        let mgr = warm_manager.read().await;
        (mgr.config(), mgr.registry())
    };

    // DISPATCH uses a temporary directory since it doesn't have a workspace
    let dispatch_working_dir = std::env::temp_dir()
        .join("gorp-dispatch")
        .to_string_lossy()
        .to_string();

    // Ensure the dispatch directory exists
    if let Err(e) = std::fs::create_dir_all(&dispatch_working_dir) {
        tracing::warn!(error = %e, "Failed to create DISPATCH working directory");
    }

    // Create agent handle using mux backend explicitly
    let agent_handle = match WarmSessionManager::create_agent_handle_with_config(
        &registry,
        &dispatch_working_dir,
        &warm_config,
        Some("mux"), // Force mux backend for DISPATCH
    ) {
        Ok(handle) => handle,
        Err(e) => {
            let _ = typing_tx.send(());
            typing_handle.abort();
            room.typing_notice(false).await?;

            let error_msg = format!("Failed to create DISPATCH agent: {}", e);
            tracing::error!(error = %e, "DISPATCH agent creation failed");
            room.send(RoomMessageEventContent::text_plain(&error_msg))
                .await?;
            return Ok(());
        }
    };

    // Load or create session
    let session_id = &dispatch_channel.session_id;
    if dispatch_channel.started {
        // Try to resume existing session
        if let Err(e) = agent_handle.load_session(session_id).await {
            tracing::warn!(error = %e, session_id = %session_id, "Failed to load DISPATCH session, creating new");
            // Session doesn't exist or is corrupted - that's fine for mux backend
            // It will create a new one automatically
        }
    }

    // Build prompt with system context
    // For DISPATCH, we prepend the system prompt to the user message as context
    // since mux backend doesn't support per-message system prompts in the same way
    let full_prompt = format!(
        "<system>\n{}\n</system>\n\n<user_message>\n{}\n</user_message>",
        system_prompt, body
    );

    // Send prompt and stream response
    tracing::info!(session_id = %session_id, "Sending prompt to DISPATCH agent");

    let mut event_rx = match agent_handle.prompt(session_id, &full_prompt).await {
        Ok(receiver) => receiver,
        Err(e) => {
            let _ = typing_tx.send(());
            typing_handle.abort();
            room.typing_notice(false).await?;

            let error_msg = format!("Failed to send prompt to DISPATCH: {}", e);
            tracing::error!(error = %e, "DISPATCH prompt failed");
            room.send(RoomMessageEventContent::text_plain(&error_msg))
                .await?;
            return Ok(());
        }
    };

    // Process streaming events
    let mut response_text = String::new();

    while let Some(agent_event) = event_rx.recv().await {
        match agent_event {
            AgentEvent::Text(text) => {
                response_text.push_str(&text);
            }
            AgentEvent::Result { text, .. } => {
                // If we didn't accumulate any text, use the result text
                if response_text.is_empty() {
                    response_text = text;
                }
                break;
            }
            AgentEvent::Error { message, .. } => {
                let _ = typing_tx.send(());
                typing_handle.abort();
                room.typing_notice(false).await?;

                let error_msg = format!("DISPATCH error: {}", message);
                tracing::error!(error = %message, "DISPATCH agent error");
                room.send(RoomMessageEventContent::text_plain(&error_msg))
                    .await?;
                return Ok(());
            }
            AgentEvent::ToolStart { name, .. } => {
                tracing::debug!(tool = %name, "DISPATCH using tool");
            }
            AgentEvent::ToolEnd { name, success, .. } => {
                tracing::debug!(tool = %name, success = success, "DISPATCH tool completed");
            }
            _ => {
                tracing::trace!(event = ?agent_event, "DISPATCH received event");
            }
        }
    }

    // Stop typing indicator
    let _ = typing_tx.send(());
    let _ = typing_handle.await;
    room.typing_notice(false).await?;

    // Send response (chunk if needed)
    if !response_text.is_empty() {
        let chunks = chunk_message(&response_text, MAX_CHUNK_SIZE);
        for chunk in chunks {
            let html = markdown_to_html(&chunk);
            room.send(RoomMessageEventContent::text_html(&chunk, &html))
                .await?;
        }
    } else {
        room.send(RoomMessageEventContent::text_plain(
            "DISPATCH completed without a response.",
        ))
        .await?;
    }

    tracing::info!(
        room_id = %room.room_id(),
        response_len = response_text.len(),
        "DISPATCH response sent"
    );

    Ok(())
}
