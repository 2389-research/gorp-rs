// ABOUTME: MCP tools for DISPATCH control plane - room queries and task dispatch.
// ABOUTME: These tools give DISPATCH cross-room visibility without filesystem access.

use crate::session::{Channel, DispatchTask, DispatchTaskStatus, SessionStore};
use async_trait::async_trait;
use mux::tool::{Tool, ToolResult};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;

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
        last_activity: None,             // TODO: Track last activity timestamp
        agent_status: AgentStatus::Idle, // TODO: Track actual status
    }
}

/// Tool: dispatch_task - Send a task to a worker room
///
/// Creates a task record and sends the prompt to the specified room.
/// Returns the created task for tracking.
///
/// Note: This is a sync function that only creates the database record.
/// The actual message sending happens in the dispatch_handler.
pub fn dispatch_task(
    session_store: &SessionStore,
    room_id: &str,
    prompt: &str,
) -> Result<DispatchTask, String> {
    // Verify the room exists and is not a DISPATCH room
    let channel = session_store
        .get_by_room(room_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Room not found: {}", room_id))?;

    if channel.is_dispatch_room {
        return Err("Cannot dispatch tasks to DISPATCH room".to_string());
    }

    // Create task record
    let task = session_store
        .create_dispatch_task(room_id, prompt)
        .map_err(|e| e.to_string())?;

    tracing::info!(
        task_id = %task.id,
        target_room = %room_id,
        prompt_preview = %prompt.chars().take(50).collect::<String>(),
        "Task dispatched"
    );

    Ok(task)
}

/// Tool: check_task - Check status of a dispatched task
///
/// Returns the current status of a task by its ID.
pub fn check_task(session_store: &SessionStore, task_id: &str) -> Result<DispatchTask, String> {
    session_store
        .get_dispatch_task(task_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Task not found: {}", task_id))
}

/// Tool: list_pending_tasks - List all pending/in-progress tasks
///
/// Returns tasks that are not yet completed or failed, sorted by creation time (newest first).
pub fn list_pending_tasks(session_store: &SessionStore) -> Result<Vec<DispatchTask>, String> {
    let pending = session_store
        .list_dispatch_tasks(Some(DispatchTaskStatus::Pending))
        .map_err(|e| e.to_string())?;
    let in_progress = session_store
        .list_dispatch_tasks(Some(DispatchTaskStatus::InProgress))
        .map_err(|e| e.to_string())?;

    // Combine and sort by creation time (newest first)
    let mut all_tasks: Vec<_> = pending.into_iter().chain(in_progress).collect();
    all_tasks.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(all_tasks)
}

/// Tool: reset_room - Reset a room's agent session
///
/// Generates a new session ID for the room, allowing it to start fresh.
/// This is useful when a session becomes corrupted or orphaned.
pub fn reset_room(session_store: &SessionStore, room_id: &str) -> Result<String, String> {
    // Verify the room exists and is not a DISPATCH room
    let channel = session_store
        .get_by_room(room_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Room not found: {}", room_id))?;

    if channel.is_dispatch_room {
        return Err("Cannot reset DISPATCH room".to_string());
    }

    session_store
        .reset_orphaned_session(room_id)
        .map_err(|e| e.to_string())?;

    // Get the new session ID
    let updated = session_store
        .get_by_room(room_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Room disappeared after reset".to_string())?;

    tracing::info!(
        room_id = %room_id,
        channel = %channel.channel_name,
        new_session_id = %updated.session_id,
        "Room session reset"
    );

    Ok(updated.session_id)
}

/// Tool: get_pending_events - Get unacknowledged events from workers
///
/// Returns events that DISPATCH hasn't processed yet.
pub fn get_pending_events(
    session_store: &SessionStore,
) -> Result<Vec<crate::session::DispatchEvent>, String> {
    session_store
        .get_pending_dispatch_events()
        .map_err(|e| e.to_string())
}

/// Tool: acknowledge_event - Mark an event as processed
///
/// Once DISPATCH has handled an event, this marks it as acknowledged.
pub fn acknowledge_event(session_store: &SessionStore, event_id: &str) -> Result<(), String> {
    session_store
        .acknowledge_dispatch_event(event_id)
        .map_err(|e| e.to_string())
}

/// Tool: list_all_rooms_summary - List all rooms including metadata
///
/// Returns a summary of all workspace rooms with their status.
pub fn list_all_rooms_summary(session_store: &SessionStore) -> Result<String, String> {
    let rooms = list_rooms(session_store)?;

    if rooms.is_empty() {
        return Ok("No workspace rooms configured.".to_string());
    }

    let summary: Vec<String> = rooms
        .iter()
        .map(|r| {
            format!(
                "* {} ({})\n  Path: {}\n  Status: {:?}",
                r.channel_name,
                r.room_id,
                if r.workspace_path.is_empty() {
                    "<no workspace>"
                } else {
                    &r.workspace_path
                },
                r.agent_status
            )
        })
        .collect();

    Ok(summary.join("\n\n"))
}

// ============================================================================
// mux::Tool implementations for DISPATCH agent
// ============================================================================

/// MuxTool: list_rooms - Get status of all workspace rooms
pub struct ListRoomsTool {
    session_store: Arc<SessionStore>,
}

impl ListRoomsTool {
    pub fn new(session_store: Arc<SessionStore>) -> Self {
        Self { session_store }
    }
}

#[async_trait]
impl Tool for ListRoomsTool {
    fn name(&self) -> &str {
        "list_rooms"
    }

    fn description(&self) -> &str {
        "List all active workspace rooms with their status. Returns room name, ID, and workspace path for each room."
    }

    fn schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {},
            "required": []
        })
    }

    async fn execute(&self, _params: serde_json::Value) -> Result<ToolResult, anyhow::Error> {
        let rooms = list_rooms(&self.session_store)
            .map_err(|e| anyhow::anyhow!("Failed to list rooms: {}", e))?;
        Ok(ToolResult::text(serde_json::to_string_pretty(&rooms)?))
    }
}

/// MuxTool: get_room_status - Get detailed info about a specific room
pub struct GetRoomStatusTool {
    session_store: Arc<SessionStore>,
}

impl GetRoomStatusTool {
    pub fn new(session_store: Arc<SessionStore>) -> Self {
        Self { session_store }
    }
}

#[async_trait]
impl Tool for GetRoomStatusTool {
    fn name(&self) -> &str {
        "get_room_status"
    }

    fn description(&self) -> &str {
        "Get detailed status of a specific workspace room by room_id or channel_name."
    }

    fn schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "room_id": {
                    "type": "string",
                    "description": "Matrix room ID (e.g., !abc123:matrix.org)"
                },
                "channel_name": {
                    "type": "string",
                    "description": "Channel name (alternative to room_id)"
                }
            },
            "required": []
        })
    }

    async fn execute(&self, params: serde_json::Value) -> Result<ToolResult, anyhow::Error> {
        #[derive(Deserialize)]
        struct Params {
            room_id: Option<String>,
            channel_name: Option<String>,
        }
        let params: Params = serde_json::from_value(params)?;

        let info = if let Some(room_id) = params.room_id {
            get_room_status(&self.session_store, &room_id)
        } else if let Some(name) = params.channel_name {
            get_room_by_name(&self.session_store, &name)
        } else {
            return Ok(ToolResult::error(
                "Either room_id or channel_name is required",
            ));
        };

        match info {
            Ok(info) => Ok(ToolResult::text(serde_json::to_string_pretty(&info)?)),
            Err(e) => Ok(ToolResult::error(e)),
        }
    }
}

/// MuxTool: dispatch_task - Send a task to a worker room
pub struct DispatchTaskTool {
    session_store: Arc<SessionStore>,
}

impl DispatchTaskTool {
    pub fn new(session_store: Arc<SessionStore>) -> Self {
        Self { session_store }
    }
}

#[async_trait]
impl Tool for DispatchTaskTool {
    fn name(&self) -> &str {
        "dispatch_task"
    }

    fn description(&self) -> &str {
        "Create a task to be sent to a worker room. The task will be queued and executed when the worker is available."
    }

    fn schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "room_id": {
                    "type": "string",
                    "description": "Target room ID to send the task to"
                },
                "prompt": {
                    "type": "string",
                    "description": "The task prompt to send to the worker"
                }
            },
            "required": ["room_id", "prompt"]
        })
    }

    async fn execute(&self, params: serde_json::Value) -> Result<ToolResult, anyhow::Error> {
        #[derive(Deserialize)]
        struct Params {
            room_id: String,
            prompt: String,
        }
        let params: Params = serde_json::from_value(params)?;

        match dispatch_task(&self.session_store, &params.room_id, &params.prompt) {
            Ok(task) => {
                let result = json!({
                    "task_id": task.id,
                    "target_room_id": task.target_room_id,
                    "status": "pending",
                    "message": "Task created and queued for execution"
                });
                Ok(ToolResult::text(serde_json::to_string_pretty(&result)?))
            }
            Err(e) => Ok(ToolResult::error(e)),
        }
    }
}

/// MuxTool: check_task - Check status of a dispatched task
pub struct CheckTaskTool {
    session_store: Arc<SessionStore>,
}

impl CheckTaskTool {
    pub fn new(session_store: Arc<SessionStore>) -> Self {
        Self { session_store }
    }
}

#[async_trait]
impl Tool for CheckTaskTool {
    fn name(&self) -> &str {
        "check_task"
    }

    fn description(&self) -> &str {
        "Check the status of a previously dispatched task by its ID."
    }

    fn schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "task_id": {
                    "type": "string",
                    "description": "The task ID returned from dispatch_task"
                }
            },
            "required": ["task_id"]
        })
    }

    async fn execute(&self, params: serde_json::Value) -> Result<ToolResult, anyhow::Error> {
        #[derive(Deserialize)]
        struct Params {
            task_id: String,
        }
        let params: Params = serde_json::from_value(params)?;

        match check_task(&self.session_store, &params.task_id) {
            Ok(task) => {
                let result = json!({
                    "task_id": task.id,
                    "target_room_id": task.target_room_id,
                    "prompt": task.prompt,
                    "status": format!("{}", task.status),
                    "created_at": task.created_at,
                    "completed_at": task.completed_at,
                    "result_summary": task.result_summary,
                });
                Ok(ToolResult::text(serde_json::to_string_pretty(&result)?))
            }
            Err(e) => Ok(ToolResult::error(e)),
        }
    }
}

/// MuxTool: list_pending_tasks - List all pending and in-progress tasks
pub struct ListPendingTasksTool {
    session_store: Arc<SessionStore>,
}

impl ListPendingTasksTool {
    pub fn new(session_store: Arc<SessionStore>) -> Self {
        Self { session_store }
    }
}

#[async_trait]
impl Tool for ListPendingTasksTool {
    fn name(&self) -> &str {
        "list_pending_tasks"
    }

    fn description(&self) -> &str {
        "List all pending and in-progress tasks across all rooms."
    }

    fn schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {},
            "required": []
        })
    }

    async fn execute(&self, _params: serde_json::Value) -> Result<ToolResult, anyhow::Error> {
        match list_pending_tasks(&self.session_store) {
            Ok(tasks) => {
                let tasks: Vec<_> = tasks
                    .into_iter()
                    .map(|t| {
                        json!({
                            "task_id": t.id,
                            "target_room_id": t.target_room_id,
                            "prompt": t.prompt,
                            "status": format!("{}", t.status),
                            "created_at": t.created_at,
                        })
                    })
                    .collect();
                Ok(ToolResult::text(serde_json::to_string_pretty(&tasks)?))
            }
            Err(e) => Ok(ToolResult::error(e)),
        }
    }
}

/// MuxTool: get_pending_events - Get events from worker rooms
pub struct GetPendingEventsTool {
    session_store: Arc<SessionStore>,
}

impl GetPendingEventsTool {
    pub fn new(session_store: Arc<SessionStore>) -> Self {
        Self { session_store }
    }
}

#[async_trait]
impl Tool for GetPendingEventsTool {
    fn name(&self) -> &str {
        "get_pending_events"
    }

    fn description(&self) -> &str {
        "Get pending events from worker rooms that need attention (completions, errors, questions)."
    }

    fn schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {},
            "required": []
        })
    }

    async fn execute(&self, _params: serde_json::Value) -> Result<ToolResult, anyhow::Error> {
        match get_pending_events(&self.session_store) {
            Ok(events) => {
                let events: Vec<_> = events
                    .into_iter()
                    .map(|e| {
                        json!({
                            "event_id": e.id,
                            "source_room_id": e.source_room_id,
                            "event_type": e.event_type,
                            "payload": e.payload,
                            "created_at": e.created_at,
                        })
                    })
                    .collect();
                Ok(ToolResult::text(serde_json::to_string_pretty(&events)?))
            }
            Err(e) => Ok(ToolResult::error(e)),
        }
    }
}

/// MuxTool: reset_room - Reset a room's agent session
pub struct ResetRoomTool {
    session_store: Arc<SessionStore>,
}

impl ResetRoomTool {
    pub fn new(session_store: Arc<SessionStore>) -> Self {
        Self { session_store }
    }
}

#[async_trait]
impl Tool for ResetRoomTool {
    fn name(&self) -> &str {
        "reset_room"
    }

    fn description(&self) -> &str {
        "Reset a room's agent session, generating a new session ID. Use when a session becomes corrupted or needs a fresh start."
    }

    fn schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "room_id": {
                    "type": "string",
                    "description": "The room ID to reset"
                }
            },
            "required": ["room_id"]
        })
    }

    async fn execute(&self, params: serde_json::Value) -> Result<ToolResult, anyhow::Error> {
        #[derive(Deserialize)]
        struct Params {
            room_id: String,
        }
        let params: Params = serde_json::from_value(params)?;

        match reset_room(&self.session_store, &params.room_id) {
            Ok(new_session_id) => {
                let result = json!({
                    "room_id": params.room_id,
                    "new_session_id": new_session_id,
                    "message": "Session reset successfully"
                });
                Ok(ToolResult::text(serde_json::to_string_pretty(&result)?))
            }
            Err(e) => Ok(ToolResult::error(e)),
        }
    }
}

/// MuxTool: acknowledge_event - Mark an event as acknowledged
pub struct AcknowledgeEventTool {
    session_store: Arc<SessionStore>,
}

impl AcknowledgeEventTool {
    pub fn new(session_store: Arc<SessionStore>) -> Self {
        Self { session_store }
    }
}

#[async_trait]
impl Tool for AcknowledgeEventTool {
    fn name(&self) -> &str {
        "acknowledge_event"
    }

    fn description(&self) -> &str {
        "Mark an event as acknowledged so it no longer appears in pending events."
    }

    fn schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "event_id": {
                    "type": "string",
                    "description": "The event ID to acknowledge"
                }
            },
            "required": ["event_id"]
        })
    }

    async fn execute(&self, params: serde_json::Value) -> Result<ToolResult, anyhow::Error> {
        #[derive(Deserialize)]
        struct Params {
            event_id: String,
        }
        let params: Params = serde_json::from_value(params)?;

        match acknowledge_event(&self.session_store, &params.event_id) {
            Ok(()) => {
                let result = json!({
                    "event_id": params.event_id,
                    "message": "Event acknowledged"
                });
                Ok(ToolResult::text(serde_json::to_string_pretty(&result)?))
            }
            Err(e) => Ok(ToolResult::error(e)),
        }
    }
}

/// Create all DISPATCH tools with the given session store
pub fn create_dispatch_tools(session_store: Arc<SessionStore>) -> Vec<Box<dyn Tool>> {
    vec![
        Box::new(ListRoomsTool::new(Arc::clone(&session_store))),
        Box::new(GetRoomStatusTool::new(Arc::clone(&session_store))),
        Box::new(DispatchTaskTool::new(Arc::clone(&session_store))),
        Box::new(CheckTaskTool::new(Arc::clone(&session_store))),
        Box::new(ListPendingTasksTool::new(Arc::clone(&session_store))),
        Box::new(GetPendingEventsTool::new(Arc::clone(&session_store))),
        Box::new(ResetRoomTool::new(Arc::clone(&session_store))),
        Box::new(AcknowledgeEventTool::new(Arc::clone(&session_store))),
    ]
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

    #[test]
    fn test_dispatch_task() {
        let tmp = TempDir::new().unwrap();
        let store = SessionStore::new(tmp.path()).unwrap();

        // Create a target room
        store
            .create_channel("worker", "!worker:example.com")
            .unwrap();

        // Dispatch a task
        let task = dispatch_task(&store, "!worker:example.com", "Run the tests").unwrap();

        assert_eq!(task.target_room_id, "!worker:example.com");
        assert_eq!(task.prompt, "Run the tests");
        assert_eq!(task.status, DispatchTaskStatus::Pending);
    }

    #[test]
    fn test_dispatch_task_to_dispatch_room_fails() {
        let tmp = TempDir::new().unwrap();
        let store = SessionStore::new(tmp.path()).unwrap();

        // Create a DISPATCH room
        store.create_dispatch_channel("!dm:example.com").unwrap();

        // Try to dispatch to it
        let result = dispatch_task(&store, "!dm:example.com", "Do something");

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("DISPATCH room"));
    }

    #[test]
    fn test_check_task() {
        let tmp = TempDir::new().unwrap();
        let store = SessionStore::new(tmp.path()).unwrap();

        store
            .create_channel("worker", "!worker:example.com")
            .unwrap();
        let task = dispatch_task(&store, "!worker:example.com", "Run the tests").unwrap();

        let checked = check_task(&store, &task.id).unwrap();

        assert_eq!(checked.id, task.id);
        assert_eq!(checked.status, DispatchTaskStatus::Pending);
    }

    #[test]
    fn test_list_pending_tasks() {
        let tmp = TempDir::new().unwrap();
        let store = SessionStore::new(tmp.path()).unwrap();

        store
            .create_channel("worker", "!worker:example.com")
            .unwrap();

        dispatch_task(&store, "!worker:example.com", "Task 1").unwrap();
        dispatch_task(&store, "!worker:example.com", "Task 2").unwrap();

        let pending = list_pending_tasks(&store).unwrap();

        assert_eq!(pending.len(), 2);
    }

    #[test]
    fn test_reset_room() {
        let tmp = TempDir::new().unwrap();
        let store = SessionStore::new(tmp.path()).unwrap();

        // Create a channel
        let channel = store.create_channel("test", "!room:example.com").unwrap();
        let original_session = channel.session_id.clone();

        // Reset the room
        let new_session = reset_room(&store, "!room:example.com").unwrap();

        assert_ne!(new_session, original_session);
    }

    #[test]
    fn test_reset_dispatch_room_fails() {
        let tmp = TempDir::new().unwrap();
        let store = SessionStore::new(tmp.path()).unwrap();

        store.create_dispatch_channel("!dm:example.com").unwrap();

        let result = reset_room(&store, "!dm:example.com");

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("DISPATCH room"));
    }

    #[test]
    fn test_list_all_rooms_summary() {
        let tmp = TempDir::new().unwrap();
        let store = SessionStore::new(tmp.path()).unwrap();

        store
            .create_channel("project-a", "!room1:example.com")
            .unwrap();
        store
            .create_channel("project-b", "!room2:example.com")
            .unwrap();

        let summary = list_all_rooms_summary(&store).unwrap();

        assert!(summary.contains("project-a"));
        assert!(summary.contains("project-b"));
    }

    #[test]
    fn test_list_all_rooms_summary_empty() {
        let tmp = TempDir::new().unwrap();
        let store = SessionStore::new(tmp.path()).unwrap();

        let summary = list_all_rooms_summary(&store).unwrap();

        assert!(summary.contains("No workspace rooms"));
    }
}
