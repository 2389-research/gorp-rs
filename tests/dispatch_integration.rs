// ABOUTME: Integration tests for DISPATCH control plane functionality.
// ABOUTME: Tests room detection, event routing, task dispatch, and mux Tool execution.

use gorp::dispatch_tools::{
    create_dispatch_tools, AcknowledgeEventTool, CheckTaskTool, DispatchTaskTool,
    GetPendingEventsTool, GetRoomStatusTool, ListPendingTasksTool, ListRoomsTool, ResetRoomTool,
};
use gorp::session::{DispatchEvent, DispatchTaskStatus, SessionStore};
use mux::tool::Tool;
use std::sync::Arc;
use tempfile::TempDir;

#[test]
fn test_dispatch_channel_creation() {
    let tmp = TempDir::new().unwrap();
    // Create template directory (required by SessionStore)
    let template_dir = tmp.path().join("template");
    std::fs::create_dir_all(&template_dir).unwrap();

    let store = SessionStore::new(tmp.path()).unwrap();

    // Create DISPATCH channel
    let room_id = "!test:matrix.org";
    let channel = store.create_dispatch_channel(room_id).unwrap();

    assert!(channel.is_dispatch_room);
    // channel_name is now "dispatch:{room_id}" for uniqueness
    assert_eq!(channel.channel_name, format!("dispatch:{}", room_id));
    assert!(channel.directory.is_empty());
}

#[test]
fn test_dispatch_multiple_users() {
    let tmp = TempDir::new().unwrap();
    let template_dir = tmp.path().join("template");
    std::fs::create_dir_all(&template_dir).unwrap();

    let store = SessionStore::new(tmp.path()).unwrap();

    // Create DISPATCH channels for two different users/DMs
    let channel1 = store.create_dispatch_channel("!dm1:matrix.org").unwrap();
    let channel2 = store.create_dispatch_channel("!dm2:matrix.org").unwrap();

    // Both should exist with different channel names
    assert!(channel1.is_dispatch_room);
    assert!(channel2.is_dispatch_room);
    assert_ne!(channel1.channel_name, channel2.channel_name);
    assert_ne!(channel1.session_id, channel2.session_id);

    // Both should be retrievable
    assert!(store
        .get_dispatch_channel("!dm1:matrix.org")
        .unwrap()
        .is_some());
    assert!(store
        .get_dispatch_channel("!dm2:matrix.org")
        .unwrap()
        .is_some());
}

#[test]
fn test_dispatch_channel_get_or_create() {
    let tmp = TempDir::new().unwrap();
    let template_dir = tmp.path().join("template");
    std::fs::create_dir_all(&template_dir).unwrap();

    let store = SessionStore::new(tmp.path()).unwrap();
    let room_id = "!dispatch-test:matrix.org";

    // First call creates
    let channel1 = store.get_or_create_dispatch_channel(room_id).unwrap();
    assert!(channel1.is_dispatch_room);

    // Second call returns existing
    let channel2 = store.get_or_create_dispatch_channel(room_id).unwrap();
    assert_eq!(channel1.session_id, channel2.session_id);
}

#[test]
fn test_dispatch_event_crud() {
    let tmp = TempDir::new().unwrap();
    let template_dir = tmp.path().join("template");
    std::fs::create_dir_all(&template_dir).unwrap();

    let store = SessionStore::new(tmp.path()).unwrap();

    // Insert event
    let event = DispatchEvent {
        id: "test-event-1".to_string(),
        source_room_id: "!room:matrix.org".to_string(),
        event_type: "dispatch:task_completed".to_string(),
        payload: serde_json::json!({"summary": "Done!"}),
        created_at: chrono::Utc::now().to_rfc3339(),
        acknowledged_at: None,
    };
    store.insert_dispatch_event(&event).unwrap();

    // Get pending events
    let pending = store.get_pending_dispatch_events().unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].id, "test-event-1");

    // Acknowledge event
    store.acknowledge_dispatch_event("test-event-1").unwrap();

    // Should no longer be pending
    let pending = store.get_pending_dispatch_events().unwrap();
    assert_eq!(pending.len(), 0);
}

#[test]
fn test_dispatch_task_lifecycle() {
    let tmp = TempDir::new().unwrap();
    let template_dir = tmp.path().join("template");
    std::fs::create_dir_all(&template_dir).unwrap();

    let store = SessionStore::new(tmp.path()).unwrap();

    // Create task
    let task = store
        .create_dispatch_task("!worker:matrix.org", "Run the tests")
        .unwrap();
    assert_eq!(task.status, DispatchTaskStatus::Pending);
    assert_eq!(task.prompt, "Run the tests");

    // Update to in_progress
    store
        .update_dispatch_task_status(&task.id, DispatchTaskStatus::InProgress, None)
        .unwrap();
    let updated = store.get_dispatch_task(&task.id).unwrap().unwrap();
    assert_eq!(updated.status, DispatchTaskStatus::InProgress);

    // Complete with result
    store
        .update_dispatch_task_status(
            &task.id,
            DispatchTaskStatus::Completed,
            Some("All tests passed"),
        )
        .unwrap();
    let completed = store.get_dispatch_task(&task.id).unwrap().unwrap();
    assert_eq!(completed.status, DispatchTaskStatus::Completed);
    assert_eq!(
        completed.result_summary,
        Some("All tests passed".to_string())
    );
}

#[test]
fn test_list_dispatch_tasks_by_status() {
    let tmp = TempDir::new().unwrap();
    let template_dir = tmp.path().join("template");
    std::fs::create_dir_all(&template_dir).unwrap();

    let store = SessionStore::new(tmp.path()).unwrap();

    // Create multiple tasks
    let task1 = store
        .create_dispatch_task("!room1:matrix.org", "Task 1")
        .unwrap();
    let task2 = store
        .create_dispatch_task("!room2:matrix.org", "Task 2")
        .unwrap();

    // Update one to in_progress
    store
        .update_dispatch_task_status(&task1.id, DispatchTaskStatus::InProgress, None)
        .unwrap();

    // List pending only
    let pending = store
        .list_dispatch_tasks(Some(DispatchTaskStatus::Pending))
        .unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].id, task2.id);

    // List in_progress only
    let in_progress = store
        .list_dispatch_tasks(Some(DispatchTaskStatus::InProgress))
        .unwrap();
    assert_eq!(in_progress.len(), 1);
    assert_eq!(in_progress[0].id, task1.id);

    // List all
    let all = store.list_dispatch_tasks(None).unwrap();
    assert_eq!(all.len(), 2);
}

// ============================================================================
// Scenario tests for mux Tool implementations
// ============================================================================

fn setup_store_with_rooms() -> (TempDir, Arc<SessionStore>) {
    let tmp = TempDir::new().unwrap();
    let template_dir = tmp.path().join("template");
    std::fs::create_dir_all(&template_dir).unwrap();

    let store = Arc::new(SessionStore::new(tmp.path()).unwrap());

    // Create some workspace rooms
    store
        .create_channel("project-alpha", "!alpha:matrix.org")
        .unwrap();
    store
        .create_channel("project-beta", "!beta:matrix.org")
        .unwrap();

    (tmp, store)
}

#[tokio::test]
async fn scenario_list_rooms_returns_workspace_rooms_only() {
    let (_tmp, store) = setup_store_with_rooms();

    // Create a DISPATCH room (should be excluded from list)
    store.create_dispatch_channel("!dm:matrix.org").unwrap();

    let tool = ListRoomsTool::new(Arc::clone(&store));
    let result = tool.execute(serde_json::json!({})).await.unwrap();

    assert!(!result.is_error);
    let rooms: Vec<serde_json::Value> = serde_json::from_str(&result.content).unwrap();

    // Should have 2 workspace rooms, not the DISPATCH room
    assert_eq!(rooms.len(), 2);
    let names: Vec<&str> = rooms
        .iter()
        .map(|r| r["channel_name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"project-alpha"));
    assert!(names.contains(&"project-beta"));
}

#[tokio::test]
async fn scenario_get_room_status_by_room_id() {
    let (_tmp, store) = setup_store_with_rooms();

    let tool = GetRoomStatusTool::new(Arc::clone(&store));
    let result = tool
        .execute(serde_json::json!({"room_id": "!alpha:matrix.org"}))
        .await
        .unwrap();

    assert!(!result.is_error);
    let info: serde_json::Value = serde_json::from_str(&result.content).unwrap();
    assert_eq!(info["channel_name"], "project-alpha");
}

#[tokio::test]
async fn scenario_get_room_status_by_channel_name() {
    let (_tmp, store) = setup_store_with_rooms();

    let tool = GetRoomStatusTool::new(Arc::clone(&store));
    let result = tool
        .execute(serde_json::json!({"channel_name": "project-beta"}))
        .await
        .unwrap();

    assert!(!result.is_error);
    let info: serde_json::Value = serde_json::from_str(&result.content).unwrap();
    assert_eq!(info["room_id"], "!beta:matrix.org");
}

#[tokio::test]
async fn scenario_get_room_status_requires_identifier() {
    let (_tmp, store) = setup_store_with_rooms();

    let tool = GetRoomStatusTool::new(Arc::clone(&store));
    let result = tool.execute(serde_json::json!({})).await.unwrap();

    assert!(result.is_error);
    assert!(result.content.contains("required"));
}

#[tokio::test]
async fn scenario_dispatch_task_creates_pending_task() {
    let (_tmp, store) = setup_store_with_rooms();

    let tool = DispatchTaskTool::new(Arc::clone(&store));
    let result = tool
        .execute(serde_json::json!({
            "room_id": "!alpha:matrix.org",
            "prompt": "Run cargo test"
        }))
        .await
        .unwrap();

    assert!(!result.is_error);
    let response: serde_json::Value = serde_json::from_str(&result.content).unwrap();
    assert_eq!(response["status"], "pending");
    assert!(response["task_id"].as_str().is_some());
}

#[tokio::test]
async fn scenario_dispatch_task_to_nonexistent_room_fails() {
    let (_tmp, store) = setup_store_with_rooms();

    let tool = DispatchTaskTool::new(Arc::clone(&store));
    let result = tool
        .execute(serde_json::json!({
            "room_id": "!nonexistent:matrix.org",
            "prompt": "Do something"
        }))
        .await
        .unwrap();

    assert!(result.is_error);
    assert!(result.content.contains("not found"));
}

#[tokio::test]
async fn scenario_dispatch_task_to_dispatch_room_fails() {
    let (_tmp, store) = setup_store_with_rooms();
    store.create_dispatch_channel("!dm:matrix.org").unwrap();

    let tool = DispatchTaskTool::new(Arc::clone(&store));
    let result = tool
        .execute(serde_json::json!({
            "room_id": "!dm:matrix.org",
            "prompt": "Self-dispatch"
        }))
        .await
        .unwrap();

    assert!(result.is_error);
    assert!(result.content.contains("DISPATCH"));
}

#[tokio::test]
async fn scenario_check_task_returns_task_details() {
    let (_tmp, store) = setup_store_with_rooms();

    // Create a task
    let task = store
        .create_dispatch_task("!alpha:matrix.org", "Build it")
        .unwrap();

    let tool = CheckTaskTool::new(Arc::clone(&store));
    let result = tool
        .execute(serde_json::json!({"task_id": task.id}))
        .await
        .unwrap();

    assert!(!result.is_error);
    let info: serde_json::Value = serde_json::from_str(&result.content).unwrap();
    assert_eq!(info["prompt"], "Build it");
    // Status is lowercase "pending" from Display trait
    assert!(info["status"]
        .as_str()
        .unwrap()
        .to_lowercase()
        .contains("pending"));
}

#[tokio::test]
async fn scenario_check_nonexistent_task_fails() {
    let (_tmp, store) = setup_store_with_rooms();

    let tool = CheckTaskTool::new(Arc::clone(&store));
    let result = tool
        .execute(serde_json::json!({"task_id": "fake-task-id"}))
        .await
        .unwrap();

    assert!(result.is_error);
    assert!(result.content.contains("not found"));
}

#[tokio::test]
async fn scenario_list_pending_tasks_shows_active_tasks() {
    let (_tmp, store) = setup_store_with_rooms();

    // Create tasks in different states
    let task1 = store
        .create_dispatch_task("!alpha:matrix.org", "Task 1")
        .unwrap();
    store
        .create_dispatch_task("!beta:matrix.org", "Task 2")
        .unwrap();
    store
        .update_dispatch_task_status(&task1.id, DispatchTaskStatus::InProgress, None)
        .unwrap();

    let tool = ListPendingTasksTool::new(Arc::clone(&store));
    let result = tool.execute(serde_json::json!({})).await.unwrap();

    assert!(!result.is_error);
    let tasks: Vec<serde_json::Value> = serde_json::from_str(&result.content).unwrap();

    // Both pending and in_progress should be listed
    assert_eq!(tasks.len(), 2);
}

#[tokio::test]
async fn scenario_get_pending_events_returns_unacknowledged() {
    let (_tmp, store) = setup_store_with_rooms();

    // Insert an event
    let event = DispatchEvent {
        id: "evt-1".to_string(),
        source_room_id: "!alpha:matrix.org".to_string(),
        event_type: "dispatch:task_completed".to_string(),
        payload: serde_json::json!({"result": "success"}),
        created_at: chrono::Utc::now().to_rfc3339(),
        acknowledged_at: None,
    };
    store.insert_dispatch_event(&event).unwrap();

    let tool = GetPendingEventsTool::new(Arc::clone(&store));
    let result = tool.execute(serde_json::json!({})).await.unwrap();

    assert!(!result.is_error);
    let events: Vec<serde_json::Value> = serde_json::from_str(&result.content).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["event_id"], "evt-1");
}

#[tokio::test]
async fn scenario_acknowledge_event_removes_from_pending() {
    let (_tmp, store) = setup_store_with_rooms();

    // Insert and acknowledge an event
    let event = DispatchEvent {
        id: "evt-2".to_string(),
        source_room_id: "!alpha:matrix.org".to_string(),
        event_type: "dispatch:question".to_string(),
        payload: serde_json::json!({"question": "Which branch?"}),
        created_at: chrono::Utc::now().to_rfc3339(),
        acknowledged_at: None,
    };
    store.insert_dispatch_event(&event).unwrap();

    let ack_tool = AcknowledgeEventTool::new(Arc::clone(&store));
    let result = ack_tool
        .execute(serde_json::json!({"event_id": "evt-2"}))
        .await
        .unwrap();

    assert!(!result.is_error);

    // Verify it's gone from pending
    let pending_tool = GetPendingEventsTool::new(Arc::clone(&store));
    let pending_result = pending_tool.execute(serde_json::json!({})).await.unwrap();
    let events: Vec<serde_json::Value> = serde_json::from_str(&pending_result.content).unwrap();
    assert_eq!(events.len(), 0);
}

#[tokio::test]
async fn scenario_reset_room_generates_new_session() {
    let (_tmp, store) = setup_store_with_rooms();

    let original = store.get_by_room("!alpha:matrix.org").unwrap().unwrap();

    let tool = ResetRoomTool::new(Arc::clone(&store));
    let result = tool
        .execute(serde_json::json!({"room_id": "!alpha:matrix.org"}))
        .await
        .unwrap();

    assert!(!result.is_error);
    let response: serde_json::Value = serde_json::from_str(&result.content).unwrap();
    let new_session_id = response["new_session_id"].as_str().unwrap();

    assert_ne!(new_session_id, original.session_id);
}

#[tokio::test]
async fn scenario_reset_dispatch_room_fails() {
    let (_tmp, store) = setup_store_with_rooms();
    store.create_dispatch_channel("!dm:matrix.org").unwrap();

    let tool = ResetRoomTool::new(Arc::clone(&store));
    let result = tool
        .execute(serde_json::json!({"room_id": "!dm:matrix.org"}))
        .await
        .unwrap();

    assert!(result.is_error);
    assert!(result.content.contains("DISPATCH"));
}

#[tokio::test]
async fn scenario_create_dispatch_tools_returns_all_eight() {
    let (_tmp, store) = setup_store_with_rooms();

    let tools = create_dispatch_tools(store);

    assert_eq!(tools.len(), 8);

    let names: Vec<&str> = tools.iter().map(|t| t.name()).collect();
    assert!(names.contains(&"list_rooms"));
    assert!(names.contains(&"get_room_status"));
    assert!(names.contains(&"dispatch_task"));
    assert!(names.contains(&"check_task"));
    assert!(names.contains(&"list_pending_tasks"));
    assert!(names.contains(&"get_pending_events"));
    assert!(names.contains(&"reset_room"));
    assert!(names.contains(&"acknowledge_event"));
}
