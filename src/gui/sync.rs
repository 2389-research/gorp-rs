// ABOUTME: Matrix sync integration for GUI - streams events to iced app
// ABOUTME: Runs background sync and sends room/message updates via channel

use matrix_sdk::ruma::events::room::message::MessageType;
use matrix_sdk::ruma::OwnedRoomId;
use matrix_sdk::{Client, Room};
use tokio::sync::mpsc;

/// Events the GUI cares about from Matrix sync
#[derive(Debug, Clone)]
pub enum MatrixEvent {
    /// New message received in a room
    Message {
        room_id: String,
        sender: String,
        content: String,
        timestamp: String,
        is_own: bool,
    },
    /// Room list changed (joined/left room)
    RoomListChanged,
    /// Typing indicator update
    Typing {
        room_id: String,
        users: Vec<String>,
    },
    /// Sync error occurred
    SyncError(String),
    /// Connection state changed
    ConnectionState(ConnectionStatus),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionStatus {
    Connected,
    Syncing,
    Disconnected,
}

/// Start the Matrix sync loop and return a receiver for GUI events
pub fn start_sync(
    client: Client,
    sync_token: String,
) -> mpsc::UnboundedReceiver<MatrixEvent> {
    let (tx, rx) = mpsc::unbounded_channel();

    // Clone for event handlers
    let tx_messages = tx.clone();
    let tx_typing = tx.clone();
    let tx_rooms = tx.clone();
    let own_user_id = client.user_id().map(|u| u.to_string()).unwrap_or_default();

    // Register message handler
    client.add_event_handler({
        let own_user = own_user_id.clone();
        move |event: matrix_sdk::ruma::events::room::message::SyncRoomMessageEvent,
              room: Room| {
            let tx = tx_messages.clone();
            let own_user = own_user.clone();
            async move {
                let Some(original) = event.as_original() else {
                    return;
                };

                let content = match &original.content.msgtype {
                    MessageType::Text(text) => text.body.clone(),
                    MessageType::Notice(notice) => notice.body.clone(),
                    MessageType::Emote(emote) => format!("* {}", emote.body),
                    _ => return, // Skip non-text messages
                };

                let sender_id = original.sender.to_string();
                let is_own = sender_id == own_user;

                // Get display name
                let sender = room
                    .get_member_no_sync(&original.sender)
                    .await
                    .ok()
                    .flatten()
                    .and_then(|m| m.display_name().map(String::from))
                    .unwrap_or_else(|| sender_id.clone());

                let timestamp = {
                    let ts = original.origin_server_ts;
                    let secs = ts.as_secs();
                    chrono::DateTime::from_timestamp(secs.into(), 0)
                        .map(|dt| dt.format("%H:%M").to_string())
                        .unwrap_or_else(|| "??:??".to_string())
                };

                let _ = tx.send(MatrixEvent::Message {
                    room_id: room.room_id().to_string(),
                    sender,
                    content,
                    timestamp,
                    is_own,
                });
            }
        }
    });

    // Register typing handler
    client.add_event_handler(
        move |event: matrix_sdk::ruma::events::typing::SyncTypingEvent, room: Room| {
            let tx = tx_typing.clone();
            async move {
                let users: Vec<String> = event
                    .content
                    .user_ids
                    .iter()
                    .map(|u| u.to_string())
                    .collect();

                let _ = tx.send(MatrixEvent::Typing {
                    room_id: room.room_id().to_string(),
                    users,
                });
            }
        },
    );

    // Register room membership handler for room list changes
    client.add_event_handler(
        move |_event: matrix_sdk::ruma::events::room::member::SyncRoomMemberEvent, _room: Room| {
            let tx = tx_rooms.clone();
            async move {
                let _ = tx.send(MatrixEvent::RoomListChanged);
            }
        },
    );

    // Spawn the sync loop
    let tx_sync = tx.clone();
    tokio::spawn(async move {
        use matrix_sdk::config::SyncSettings;

        let settings = SyncSettings::default().token(sync_token);

        // Notify connected
        let _ = tx_sync.send(MatrixEvent::ConnectionState(ConnectionStatus::Connected));

        tracing::info!("GUI sync loop starting");

        // Run continuous sync
        match client.sync(settings).await {
            Ok(_) => {
                tracing::warn!("Matrix sync returned unexpectedly");
            }
            Err(e) => {
                tracing::error!(error = %e, "Matrix sync failed");
                let _ = tx_sync.send(MatrixEvent::SyncError(e.to_string()));
                let _ = tx_sync.send(MatrixEvent::ConnectionState(ConnectionStatus::Disconnected));
            }
        }
    });

    rx
}

/// Load recent messages for a room (for initial chat view population)
pub async fn load_room_messages(
    client: &Client,
    room_id: &str,
    limit: usize,
) -> Vec<(String, String, String, bool)> {
    let own_user_id = client.user_id().map(|u| u.to_string()).unwrap_or_default();

    let Ok(room_id) = room_id.parse::<OwnedRoomId>() else {
        return Vec::new();
    };

    let Some(room) = client.get_room(&room_id) else {
        return Vec::new();
    };

    let mut messages = Vec::new();

    // Use room timeline if available
    let options = matrix_sdk::room::MessagesOptions::backward();

    match room.messages(options).await {
        Ok(response) => {
            for event in response.chunk.iter().take(limit) {
                // Try to deserialize as a room message event
                if let Ok(any_event) = event.raw().deserialize() {
                    use matrix_sdk::ruma::events::AnySyncTimelineEvent;
                    if let AnySyncTimelineEvent::MessageLike(
                        matrix_sdk::ruma::events::AnySyncMessageLikeEvent::RoomMessage(msg),
                    ) = any_event
                    {
                        let Some(original) = msg.as_original() else {
                            continue;
                        };

                        let content = match &original.content.msgtype {
                            MessageType::Text(text) => text.body.clone(),
                            MessageType::Notice(notice) => notice.body.clone(),
                            MessageType::Emote(emote) => format!("* {}", emote.body),
                            _ => continue,
                        };

                        let sender_id = original.sender.to_string();
                        let is_own = sender_id == own_user_id;

                        let sender = sender_id
                            .split(':')
                            .next()
                            .unwrap_or(&sender_id)
                            .trim_start_matches('@')
                            .to_string();

                        let timestamp = {
                            let secs = original.origin_server_ts.as_secs();
                            chrono::DateTime::from_timestamp(secs.into(), 0)
                                .map(|dt| dt.format("%H:%M").to_string())
                                .unwrap_or_else(|| "??:??".to_string())
                        };

                        messages.push((sender, content, timestamp, is_own));
                    }
                }
            }
        }
        Err(e) => {
            tracing::error!(error = %e, "Failed to load room messages");
        }
    }

    // Reverse to get chronological order
    messages.reverse();
    messages
}
