// ABOUTME: Message handler that processes Matrix room messages with authentication and Claude invocation.
// ABOUTME: Checks room ID and user whitelist, manages typing indicators, and handles Claude response delivery.

use anyhow::Result;
use matrix_sdk::{
    room::Room,
    ruma::events::room::message::RoomMessageEventContent,
    Client,
    RoomState,
};

use crate::{claude, config::Config, session::SessionStore};

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

    // Only process messages from configured room
    if room.room_id().as_str() != config.matrix_room_id {
        return Ok(());
    }

    let sender = event.sender.as_str();
    let body = &event.content.body();

    // Ignore bot's own messages
    if sender == client.user_id().unwrap().as_str() {
        return Ok(());
    }

    // Check whitelist
    if !config.allowed_users.contains(sender) {
        tracing::debug!(sender, "Ignoring message from unauthorized user");
        return Ok(());
    }

    tracing::info!(sender, room_id = %room.room_id(), message_preview = &body[..body.len().min(50)], "Processing message");

    // Load session
    let session = session_store.get_or_create(room.room_id().as_str())?;
    let session_args = session.cli_args();

    // Set typing indicator
    room.typing_notice(true).await?;

    // Invoke Claude
    let response = match claude::invoke_claude(
        &config.claude_binary_path,
        config.claude_sdk_url.as_deref(),
        session_args,
        body,
    ).await {
        Ok(resp) => {
            tracing::info!(response_length = resp.len(), "Claude responded");
            resp
        }
        Err(e) => {
            tracing::error!(error = %e, "Claude invocation failed");
            let error_msg = format!("⚠️ Claude error: {}", e);
            room.typing_notice(false).await?;
            room.send(RoomMessageEventContent::text_plain(&error_msg)).await?;
            return Ok(());
        }
    };

    // Clear typing indicator
    room.typing_notice(false).await?;

    // Send response
    room.send(RoomMessageEventContent::text_plain(&response)).await?;

    // Mark session as started
    session_store.mark_started(room.room_id().as_str())?;

    tracing::info!("Response sent successfully");

    Ok(())
}
