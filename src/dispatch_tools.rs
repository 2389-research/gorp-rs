// ABOUTME: MCP tools for DISPATCH control plane - room queries and task dispatch.
// ABOUTME: These tools give DISPATCH cross-room visibility without filesystem access.

use crate::session::{Channel, SessionStore};
use serde::{Deserialize, Serialize};

/// Room information for DISPATCH
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoomInfo {
    pub room_id: String,
    pub channel_name: String,
    pub workspace_path: String,
    pub last_activity: Option<String>,
    pub agent_status: AgentStatus,
}

/// Status of an agent in a room
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentStatus {
    Idle,
    Working,
    WaitingInput,
    Error,
}

impl Default for AgentStatus {
    fn default() -> Self {
        Self::Idle
    }
}

/// Tool: list_rooms - List all active workspace rooms
///
/// Returns information about all non-DISPATCH rooms.
pub fn list_rooms(session_store: &SessionStore) -> Result<Vec<RoomInfo>, String> {
    let channels = session_store.list_all().map_err(|e| e.to_string())?;

    Ok(channels
        .into_iter()
        .filter(|c| !c.is_dispatch_room)
        .map(channel_to_room_info)
        .collect())
}

/// Tool: get_room_status - Get detailed status of a specific room
///
/// Returns detailed information about a single room by its Matrix room ID.
pub fn get_room_status(session_store: &SessionStore, room_id: &str) -> Result<RoomInfo, String> {
    let channel = session_store
        .get_by_room(room_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Room not found: {}", room_id))?;

    if channel.is_dispatch_room {
        return Err("Cannot get status of DISPATCH room".to_string());
    }

    Ok(channel_to_room_info(channel))
}

/// Tool: get_room_by_name - Get room info by channel name
///
/// Returns information about a room looked up by its channel name.
pub fn get_room_by_name(session_store: &SessionStore, name: &str) -> Result<RoomInfo, String> {
    let channel = session_store
        .get_by_name(name)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Channel not found: {}", name))?;

    if channel.is_dispatch_room {
        return Err("Cannot get status of DISPATCH room".to_string());
    }

    Ok(channel_to_room_info(channel))
}

/// Convert a Channel to RoomInfo
fn channel_to_room_info(channel: Channel) -> RoomInfo {
    RoomInfo {
        room_id: channel.room_id,
        channel_name: channel.channel_name,
        workspace_path: channel.directory,
        last_activity: None, // TODO: Track last activity timestamp
        agent_status: AgentStatus::Idle, // TODO: Track actual status
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_list_rooms_excludes_dispatch() {
        let tmp = TempDir::new().unwrap();
        let store = SessionStore::new(tmp.path()).unwrap();

        // Create a regular channel
        store
            .create_channel("test-channel", "!room1:example.com")
            .unwrap();

        // Create a DISPATCH channel
        store.create_dispatch_channel("!dm:example.com").unwrap();

        let rooms = list_rooms(&store).unwrap();

        assert_eq!(rooms.len(), 1);
        assert_eq!(rooms[0].channel_name, "test-channel");
    }

    #[test]
    fn test_get_room_status() {
        let tmp = TempDir::new().unwrap();
        let store = SessionStore::new(tmp.path()).unwrap();

        store
            .create_channel("my-project", "!room1:example.com")
            .unwrap();

        let info = get_room_status(&store, "!room1:example.com").unwrap();

        assert_eq!(info.channel_name, "my-project");
        assert_eq!(info.room_id, "!room1:example.com");
    }

    #[test]
    fn test_get_room_status_not_found() {
        let tmp = TempDir::new().unwrap();
        let store = SessionStore::new(tmp.path()).unwrap();

        let result = get_room_status(&store, "!nonexistent:example.com");

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not found"));
    }

    #[test]
    fn test_get_room_by_name() {
        let tmp = TempDir::new().unwrap();
        let store = SessionStore::new(tmp.path()).unwrap();

        store
            .create_channel("my-project", "!room1:example.com")
            .unwrap();

        let info = get_room_by_name(&store, "my-project").unwrap();

        assert_eq!(info.channel_name, "my-project");
    }
}
