use gorp_agent::backends::mock::MockBackend;
use gorp_agent::AgentEvent;
use serde_json::json;

#[tokio::test]
async fn test_mock_backend_returns_configured_text_response() {
    let mock = MockBackend::new()
        .on_prompt("hello")
        .respond_text("Hi there!");

    let handle = mock.into_handle();
    let session_id = handle.new_session().await.unwrap();

    let mut receiver = handle.prompt(&session_id, "hello").await.unwrap();

    let event = receiver.recv().await.unwrap();
    match event {
        AgentEvent::Result { text, .. } => assert_eq!(text, "Hi there!"),
        _ => panic!("Expected Result event, got {:?}", event),
    }
}

#[tokio::test]
async fn test_mock_backend_tool_response() {
    let mock = MockBackend::new().on_prompt("read file").respond_with(vec![
        AgentEvent::ToolStart {
            id: "t1".to_string(),
            name: "Read".to_string(),
            input: json!({"path": "/tmp/foo"}),
        },
        AgentEvent::ToolEnd {
            id: "t1".to_string(),
            name: "Read".to_string(),
            output: json!({"content": "file contents"}),
            success: true,
            duration_ms: 10,
        },
        AgentEvent::Result {
            text: "Read the file".to_string(),
            usage: None,
            metadata: json!({}),
        },
    ]);

    let handle = mock.into_handle();
    let session_id = handle.new_session().await.unwrap();
    let mut receiver = handle.prompt(&session_id, "read file").await.unwrap();

    let mut events = vec![];
    while let Some(e) = receiver.recv().await {
        events.push(e);
    }

    assert_eq!(events.len(), 3);
    assert!(matches!(&events[0], AgentEvent::ToolStart { name, .. } if name == "Read"));
    assert!(matches!(
        &events[1],
        AgentEvent::ToolEnd { success: true, .. }
    ));
    assert!(matches!(&events[2], AgentEvent::Result { .. }));
}

#[tokio::test]
async fn test_mock_backend_error_response() {
    let mock = MockBackend::new()
        .on_prompt("fail")
        .respond_error(gorp_agent::ErrorCode::BackendError, "Something went wrong");

    let handle = mock.into_handle();
    let session_id = handle.new_session().await.unwrap();
    let mut receiver = handle.prompt(&session_id, "fail").await.unwrap();

    let event = receiver.recv().await.unwrap();
    match event {
        AgentEvent::Error { code, message, .. } => {
            assert_eq!(code, gorp_agent::ErrorCode::BackendError);
            assert_eq!(message, "Something went wrong");
        }
        _ => panic!("Expected Error event, got {:?}", event),
    }
}

#[tokio::test]
async fn test_mock_backend_multiple_expectations() {
    let mock = MockBackend::new()
        .on_prompt("first")
        .respond_text("First response")
        .on_prompt("second")
        .respond_text("Second response");

    let handle = mock.into_handle();
    let session_id = handle.new_session().await.unwrap();

    // First prompt
    let mut receiver = handle.prompt(&session_id, "first").await.unwrap();
    let event = receiver.recv().await.unwrap();
    match event {
        AgentEvent::Result { text, .. } => assert_eq!(text, "First response"),
        _ => panic!("Expected Result event"),
    }

    // Second prompt
    let mut receiver = handle.prompt(&session_id, "second").await.unwrap();
    let event = receiver.recv().await.unwrap();
    match event {
        AgentEvent::Result { text, .. } => assert_eq!(text, "Second response"),
        _ => panic!("Expected Result event"),
    }
}

#[tokio::test]
async fn test_mock_backend_partial_match() {
    let mock = MockBackend::new()
        .on_prompt("read")
        .respond_text("Reading...");

    let handle = mock.into_handle();
    let session_id = handle.new_session().await.unwrap();

    // Should match prompts containing "read"
    let mut receiver = handle
        .prompt(&session_id, "please read the file")
        .await
        .unwrap();
    let event = receiver.recv().await.unwrap();
    match event {
        AgentEvent::Result { text, .. } => assert_eq!(text, "Reading..."),
        _ => panic!("Expected Result event"),
    }
}

#[tokio::test]
async fn test_mock_backend_unmatched_prompt() {
    let mock = MockBackend::new()
        .on_prompt("do something")
        .respond_text("Done");

    let handle = mock.into_handle();
    let session_id = handle.new_session().await.unwrap();

    // Prompt that doesn't match any expectation
    let mut receiver = handle
        .prompt(&session_id, "completely different")
        .await
        .unwrap();
    let event = receiver.recv().await.unwrap();
    match event {
        AgentEvent::Result { text, .. } => {
            assert!(text.contains("no expectation"));
            assert!(text.contains("completely different"));
        }
        _ => panic!("Expected Result event"),
    }
}

#[tokio::test]
async fn test_mock_backend_session_management() {
    let mock = MockBackend::new();
    let handle = mock.into_handle();

    // Create multiple sessions
    let session1 = handle.new_session().await.unwrap();
    let session2 = handle.new_session().await.unwrap();

    // Sessions should have different IDs
    assert_ne!(session1, session2);

    // Should be able to load existing session
    handle.load_session(&session1).await.unwrap();
}

#[tokio::test]
async fn test_mock_backend_streaming_text() {
    let mock = MockBackend::new().on_prompt("stream").respond_with(vec![
        AgentEvent::Text("Hello ".to_string()),
        AgentEvent::Text("world!".to_string()),
        AgentEvent::Result {
            text: "Hello world!".to_string(),
            usage: None,
            metadata: json!({}),
        },
    ]);

    let handle = mock.into_handle();
    let session_id = handle.new_session().await.unwrap();
    let mut receiver = handle.prompt(&session_id, "stream").await.unwrap();

    let mut events = vec![];
    while let Some(e) = receiver.recv().await {
        events.push(e);
    }

    assert_eq!(events.len(), 3);
    assert!(matches!(&events[0], AgentEvent::Text(s) if s == "Hello "));
    assert!(matches!(&events[1], AgentEvent::Text(s) if s == "world!"));
    assert!(matches!(&events[2], AgentEvent::Result { .. }));
}

#[tokio::test]
async fn test_mock_backend_cancel() {
    let mock = MockBackend::new();
    let handle = mock.into_handle();
    let session_id = handle.new_session().await.unwrap();

    // Cancel should succeed (even if nothing to cancel)
    handle.cancel(&session_id).await.unwrap();
}

#[tokio::test]
async fn test_mock_backend_name() {
    let mock = MockBackend::new();
    let handle = mock.into_handle();
    assert_eq!(handle.name(), "mock");
}
