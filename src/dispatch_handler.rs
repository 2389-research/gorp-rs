// ABOUTME: DISPATCH control plane message handler for orchestrating workspace rooms.
// ABOUTME: Runs in 1:1 DM, provides cross-room visibility and task dispatch.

use anyhow::Result;
use matrix_sdk::{room::Room, ruma::events::room::message::RoomMessageEventContent, Client, RoomState};

use crate::{config::Config, session::SessionStore, warm_session::SharedWarmSessionManager};

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
    _warm_manager: SharedWarmSessionManager,
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
    let dispatch_channel =
        session_store.get_or_create_dispatch_channel(room.room_id().as_str())?;

    tracing::debug!(
        session_id = %dispatch_channel.session_id,
        "Using DISPATCH session"
    );

    // TODO: Implement full DISPATCH agent invocation in Task 5.2
    // For now, just acknowledge the message
    let acknowledgement = format!(
        "DISPATCH received your message. (Session: {})\n\n\
        Full agent integration coming soon!",
        &dispatch_channel.session_id[..8]
    );

    room.send(RoomMessageEventContent::text_plain(&acknowledgement))
        .await?;

    Ok(())
}
