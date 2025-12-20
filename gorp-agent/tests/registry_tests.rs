// ABOUTME: Tests for the AgentRegistry and BackendFactory pattern.
// ABOUTME: Validates runtime backend selection and creation.

use gorp_agent::registry::AgentRegistry;
use serde_json::json;

#[tokio::test]
async fn test_registry_creates_mock_backend() {
    let registry = AgentRegistry::default();
    let handle = registry.create("mock", &json!({})).unwrap();
    assert_eq!(handle.name(), "mock");
}

#[tokio::test]
async fn test_registry_creates_direct_backend() {
    let registry = AgentRegistry::default();
    let config = json!({
        "binary": "claude",
        "working_dir": "/tmp"
    });
    let handle = registry.create("direct", &config).unwrap();
    assert_eq!(handle.name(), "direct");
}

#[tokio::test]
async fn test_registry_lists_available_backends() {
    let registry = AgentRegistry::default();
    let available = registry.available();
    assert!(available.contains(&"mock"));
    assert!(available.contains(&"direct"));
}

#[test]
fn test_registry_unknown_backend_errors() {
    let registry = AgentRegistry::default();
    let result = registry.create("nonexistent", &json!({}));
    assert!(result.is_err());
    match result {
        Err(err) => assert!(err.to_string().contains("Unknown backend: nonexistent")),
        Ok(_) => panic!("Expected error for unknown backend"),
    }
}

#[test]
fn test_registry_custom_factory() {
    use gorp_agent::handle::AgentHandle;
    use gorp_agent::registry::BackendFactory;
    use tokio::sync::mpsc;

    let factory: BackendFactory = Box::new(|_config| {
        let (tx, _rx) = mpsc::channel(1);
        Ok(AgentHandle::new(tx, "custom"))
    });

    let registry = AgentRegistry::new().register("custom", factory);
    let handle = registry.create("custom", &json!({})).unwrap();
    assert_eq!(handle.name(), "custom");
}

#[tokio::test]
async fn test_mock_backend_via_registry_works() {
    use gorp_agent::AgentEvent;

    let registry = AgentRegistry::default();
    let handle = registry.create("mock", &json!({})).unwrap();

    let session_id = handle.new_session().await.unwrap();
    assert!(session_id.starts_with("mock-session-"));

    // Send a prompt - mock returns default response for unmatched prompts
    let mut receiver = handle.prompt(&session_id, "test prompt").await.unwrap();

    // Should receive a result event
    let event = receiver.recv().await;
    assert!(event.is_some());
    match event.unwrap() {
        AgentEvent::Result { text, .. } => {
            assert!(text.contains("Mock: no expectation"));
        }
        other => panic!("Expected Result event, got {:?}", other),
    }
}

#[test]
fn test_registry_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<AgentRegistry>();
}

#[tokio::test]
async fn test_direct_backend_config_validation() {
    let registry = AgentRegistry::default();

    // Missing required fields should fail
    let result = registry.create("direct", &json!({}));
    assert!(result.is_err());

    // Valid config should work
    let result = registry.create(
        "direct",
        &json!({
            "binary": "claude",
            "working_dir": "/tmp"
        }),
    );
    assert!(result.is_ok());
}
