// ABOUTME: Integration tests for DISPATCH control plane functionality.
// ABOUTME: Tests room detection, event routing, and task dispatch.

use gorp::session::{DispatchEvent, DispatchTaskStatus, SessionStore};
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
    assert!(store.get_dispatch_channel("!dm1:matrix.org").unwrap().is_some());
    assert!(store.get_dispatch_channel("!dm2:matrix.org").unwrap().is_some());
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
