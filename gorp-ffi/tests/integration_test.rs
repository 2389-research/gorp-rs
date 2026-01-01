// ABOUTME: Integration tests for gorp-ffi.
// ABOUTME: Tests the FFI layer without generating actual bindings.

use gorp_ffi::{
    AgentEventCallback, FfiAgentRegistry, FfiErrorCode, FfiSchedulerStore, FfiSessionStore,
    FfiUsage,
};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use tempfile::TempDir;

#[test]
fn test_registry_lists_backends() {
    let registry = FfiAgentRegistry::new();
    let backends = registry.available_backends();

    assert!(backends.contains(&"mock".to_string()));
    assert!(backends.contains(&"direct".to_string()));
    // acp and mux depend on feature flags
}

#[tokio::test]
async fn test_create_mock_backend() {
    let registry = FfiAgentRegistry::new();
    let config = r#"{"responses": ["Hello!"]}"#;

    let handle = registry.create("mock".to_string(), config.to_string());
    assert!(handle.is_ok());

    let handle = handle.unwrap();
    assert_eq!(handle.name(), "mock");
}

#[test]
fn test_session_store_crud() {
    let temp_dir = TempDir::new().unwrap();
    let workspace_path = temp_dir.path().to_string_lossy().to_string();

    let store = FfiSessionStore::new(workspace_path).unwrap();

    // Create channel
    let channel = store
        .create_channel("test-channel".to_string(), "!room:example.com".to_string())
        .unwrap();
    assert_eq!(channel.channel_name, "test-channel");
    assert!(!channel.started);

    // Get by name
    let found = store.get_by_name("test-channel".to_string()).unwrap();
    assert!(found.is_some());
    assert_eq!(found.unwrap().room_id, "!room:example.com");

    // List all
    let all = store.list_all().unwrap();
    assert_eq!(all.len(), 1);

    // Mark started
    store.mark_started("!room:example.com".to_string()).unwrap();
    let updated = store.get_by_name("test-channel".to_string()).unwrap();
    assert!(updated.unwrap().started);

    // Delete
    store.delete_channel("test-channel".to_string()).unwrap();
    let deleted = store.get_by_name("test-channel".to_string()).unwrap();
    assert!(deleted.is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn test_mock_backend_new_session() {
    let registry = FfiAgentRegistry::new();
    let config = r#"{"responses": ["Hello!"]}"#;
    let handle = registry
        .create("mock".to_string(), config.to_string())
        .unwrap();

    // new_session uses block_on internally, so run it in spawn_blocking
    // to avoid nested runtime conflict
    let session_id = tokio::task::spawn_blocking(move || handle.new_session())
        .await
        .unwrap()
        .unwrap();
    assert!(!session_id.is_empty());
}

#[test]
fn test_scheduler_store_crud() {
    let temp_dir = TempDir::new().unwrap();
    let workspace_path = temp_dir.path().to_string_lossy().to_string();

    // Create session store first (scheduler shares its DB)
    let session_store = FfiSessionStore::new(workspace_path).unwrap();
    let scheduler_store = FfiSchedulerStore::new(&session_store).unwrap();

    // Create a channel first (required by foreign key constraint)
    session_store
        .create_channel("test-channel".to_string(), "!room:example.com".to_string())
        .unwrap();

    // Initially empty
    let all = scheduler_store.list_all().unwrap();
    assert!(all.is_empty());

    // Create a schedule with relative time expression
    let schedule = scheduler_store
        .create_schedule(
            "test-channel".to_string(),
            "!room:example.com".to_string(),
            "remind me to check logs".to_string(),
            "@user:example.com".to_string(),
            "in 1 hour".to_string(),
            "UTC".to_string(),
        )
        .unwrap();

    assert_eq!(schedule.channel_name, "test-channel");
    assert_eq!(schedule.room_id, "!room:example.com");
    assert_eq!(schedule.prompt, "remind me to check logs");
    assert!(!schedule.id.is_empty());

    // Get by ID
    let found = scheduler_store.get_by_id(schedule.id.clone()).unwrap();
    assert!(found.is_some());
    assert_eq!(found.unwrap().prompt, "remind me to check logs");

    // List by room
    let by_room = scheduler_store
        .list_by_room("!room:example.com".to_string())
        .unwrap();
    assert_eq!(by_room.len(), 1);

    // List by channel
    let by_channel = scheduler_store
        .list_by_channel("test-channel".to_string())
        .unwrap();
    assert_eq!(by_channel.len(), 1);

    // Pause
    scheduler_store.pause_schedule(schedule.id.clone()).unwrap();
    let paused = scheduler_store.get_by_id(schedule.id.clone()).unwrap();
    assert!(matches!(
        paused.unwrap().status,
        gorp_ffi::FfiScheduleStatus::Paused
    ));

    // Resume
    scheduler_store
        .resume_schedule(schedule.id.clone())
        .unwrap();
    let resumed = scheduler_store.get_by_id(schedule.id.clone()).unwrap();
    assert!(matches!(
        resumed.unwrap().status,
        gorp_ffi::FfiScheduleStatus::Active
    ));

    // Delete
    let deleted = scheduler_store
        .delete_schedule(schedule.id.clone())
        .unwrap();
    assert!(deleted);
    let gone = scheduler_store.get_by_id(schedule.id).unwrap();
    assert!(gone.is_none());
}

#[test]
fn test_scheduler_create_with_recurring() {
    let temp_dir = TempDir::new().unwrap();
    let workspace_path = temp_dir.path().to_string_lossy().to_string();

    let session_store = FfiSessionStore::new(workspace_path).unwrap();
    let scheduler_store = FfiSchedulerStore::new(&session_store).unwrap();

    // Create channel first (required by foreign key constraint)
    session_store
        .create_channel(
            "daily-channel".to_string(),
            "!daily:example.com".to_string(),
        )
        .unwrap();

    // Create a recurring schedule with natural language
    let schedule = scheduler_store
        .create_schedule(
            "daily-channel".to_string(),
            "!daily:example.com".to_string(),
            "daily standup reminder".to_string(),
            "@bot:example.com".to_string(),
            "every day 9am".to_string(),
            "America/New_York".to_string(),
        )
        .unwrap();

    assert_eq!(schedule.channel_name, "daily-channel");
    assert!(schedule.cron_expression.is_some());
    assert_eq!(schedule.cron_expression.unwrap(), "0 9 * * *");
    assert!(schedule.execute_at.is_none()); // Recurring schedules don't have one-time execute_at

    // Clean up
    scheduler_store.delete_schedule(schedule.id).unwrap();
}

#[test]
fn test_scheduler_invalid_time_expression() {
    let temp_dir = TempDir::new().unwrap();
    let workspace_path = temp_dir.path().to_string_lossy().to_string();

    let session_store = FfiSessionStore::new(workspace_path).unwrap();
    let scheduler_store = FfiSchedulerStore::new(&session_store).unwrap();

    // Invalid time expression should return error
    let result = scheduler_store.create_schedule(
        "test-channel".to_string(),
        "!room:example.com".to_string(),
        "some prompt".to_string(),
        "@user:example.com".to_string(),
        "not a valid time".to_string(),
        "UTC".to_string(),
    );

    assert!(result.is_err());
}

/// Test callback for tracking events received from the agent
struct TestCallback {
    text_received: AtomicBool,
    result_received: AtomicBool,
    error_received: AtomicBool,
    event_count: AtomicUsize,
}

impl TestCallback {
    fn new() -> Self {
        Self {
            text_received: AtomicBool::new(false),
            result_received: AtomicBool::new(false),
            error_received: AtomicBool::new(false),
            event_count: AtomicUsize::new(0),
        }
    }
}

impl AgentEventCallback for TestCallback {
    fn on_text(&self, _text: String) {
        self.text_received.store(true, Ordering::SeqCst);
        self.event_count.fetch_add(1, Ordering::SeqCst);
    }

    fn on_tool_start(&self, _id: String, _name: String, _input_json: String) {
        self.event_count.fetch_add(1, Ordering::SeqCst);
    }

    fn on_tool_progress(&self, _id: String, _update_json: String) {
        self.event_count.fetch_add(1, Ordering::SeqCst);
    }

    fn on_tool_end(
        &self,
        _id: String,
        _name: String,
        _output_json: String,
        _success: bool,
        _duration_ms: u64,
    ) {
        self.event_count.fetch_add(1, Ordering::SeqCst);
    }

    fn on_result(&self, _text: String, _usage: Option<FfiUsage>, _metadata_json: String) {
        self.result_received.store(true, Ordering::SeqCst);
        self.event_count.fetch_add(1, Ordering::SeqCst);
    }

    fn on_error(&self, _code: FfiErrorCode, _message: String, _recoverable: bool) {
        self.error_received.store(true, Ordering::SeqCst);
        self.event_count.fetch_add(1, Ordering::SeqCst);
    }

    fn on_session_invalid(&self, _reason: String) {
        self.event_count.fetch_add(1, Ordering::SeqCst);
    }

    fn on_session_changed(&self, _new_session_id: String) {
        self.event_count.fetch_add(1, Ordering::SeqCst);
    }

    fn on_custom(&self, _kind: String, _payload_json: String) {
        self.event_count.fetch_add(1, Ordering::SeqCst);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn test_mock_backend_prompt_callback() {
    let registry = FfiAgentRegistry::new();
    let config = r#"{"responses": ["Hello from mock!"]}"#;
    let handle = registry
        .create("mock".to_string(), config.to_string())
        .unwrap();

    // Create session
    let session_id = tokio::task::spawn_blocking({
        let handle = Arc::clone(&handle);
        move || handle.new_session()
    })
    .await
    .unwrap()
    .unwrap();

    // Create callback to track events
    let callback = Arc::new(TestCallback::new());

    // Send prompt - this spawns a background task
    let callback_box = Box::new(CallbackWrapper(Arc::clone(&callback)));
    let prompt_result = handle.prompt(session_id, "Hello!".to_string(), callback_box);
    assert!(prompt_result.is_ok());

    // Give the background task time to complete
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Verify we received events
    assert!(
        callback.event_count.load(Ordering::SeqCst) > 0,
        "Expected at least one event from mock backend"
    );
}

/// Wrapper to make Arc<TestCallback> implement AgentEventCallback
struct CallbackWrapper(Arc<TestCallback>);

impl AgentEventCallback for CallbackWrapper {
    fn on_text(&self, text: String) {
        self.0.on_text(text);
    }

    fn on_tool_start(&self, id: String, name: String, input_json: String) {
        self.0.on_tool_start(id, name, input_json);
    }

    fn on_tool_progress(&self, id: String, update_json: String) {
        self.0.on_tool_progress(id, update_json);
    }

    fn on_tool_end(
        &self,
        id: String,
        name: String,
        output_json: String,
        success: bool,
        duration_ms: u64,
    ) {
        self.0
            .on_tool_end(id, name, output_json, success, duration_ms);
    }

    fn on_result(&self, text: String, usage: Option<FfiUsage>, metadata_json: String) {
        self.0.on_result(text, usage, metadata_json);
    }

    fn on_error(&self, code: FfiErrorCode, message: String, recoverable: bool) {
        self.0.on_error(code, message, recoverable);
    }

    fn on_session_invalid(&self, reason: String) {
        self.0.on_session_invalid(reason);
    }

    fn on_session_changed(&self, new_session_id: String) {
        self.0.on_session_changed(new_session_id);
    }

    fn on_custom(&self, kind: String, payload_json: String) {
        self.0.on_custom(kind, payload_json);
    }
}
