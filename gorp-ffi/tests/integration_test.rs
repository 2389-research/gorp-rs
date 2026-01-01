// ABOUTME: Integration tests for gorp-ffi.
// ABOUTME: Tests the FFI layer without generating actual bindings.

use gorp_ffi::{FfiAgentRegistry, FfiSessionStore};
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
