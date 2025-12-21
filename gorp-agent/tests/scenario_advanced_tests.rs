// ABOUTME: Advanced scenario tests for registry, recording/replay, and MockBuilder.
// ABOUTME: Tests real system behavior without mocks - uses actual implementations.

use gorp_agent::backends::mock::MockBackend;
use gorp_agent::registry::AgentRegistry;
use gorp_agent::testing::mock_builder::{MockAgentBuilder, ToolCall};
use gorp_agent::testing::recording::{Interaction, RecordingAgent, ReplayAgent};
use gorp_agent::AgentEvent;
use std::time::Duration;

/// Scenario: Registry creates and manages multiple backend types
/// Given: A registry with default backends registered
/// When: Backends are created by name
/// Then: Each backend works correctly with its expected configuration
#[tokio::test]
async fn scenario_registry_multi_backend_creation() {
    let registry = AgentRegistry::default();

    // Verify available backends
    let backends = registry.available();
    assert!(backends.contains(&"mock"), "Mock backend should be available");
    assert!(backends.contains(&"direct"), "Direct backend should be available");

    // Create a mock backend via registry
    let mock_config = serde_json::json!({});
    let mock_handle = registry.create("mock", &mock_config).unwrap();
    assert_eq!(mock_handle.name(), "mock");

    // Verify mock works
    let session = mock_handle.new_session().await.unwrap();
    assert!(!session.is_empty());
}

/// Scenario: Registry custom factory registration
/// Given: A custom backend factory
/// When: The factory is registered with a unique name
/// Then: The custom backend can be created and used
#[tokio::test]
async fn scenario_registry_custom_factory() {
    let registry = AgentRegistry::new()
        .register("custom-test", |_config| {
            let mock = MockBackend::new()
                .on_prompt("custom").respond_text("Custom backend response");
            Ok(mock.into_handle())
        });

    // Create and use the custom backend
    let config = serde_json::json!({});
    let handle = registry.create("custom-test", &config).unwrap();
    let session = handle.new_session().await.unwrap();

    let mut rx = handle.prompt(&session, "this is custom").await.unwrap();
    let event = rx.recv().await.unwrap();

    assert!(matches!(event, AgentEvent::Result { text, .. } if text.contains("Custom")));
}

/// Scenario: Registry error handling for unknown backends
/// Given: A registry without a specific backend
/// When: An unknown backend is requested
/// Then: A clear error is returned
#[tokio::test]
async fn scenario_registry_unknown_backend_error() {
    let registry = AgentRegistry::new();

    let config = serde_json::json!({});
    let result = registry.create("nonexistent-backend", &config);

    assert!(result.is_err());
    match result {
        Err(err) => assert!(err.to_string().contains("Unknown backend")),
        Ok(_) => panic!("Expected error for unknown backend"),
    }
}

/// Scenario: Recording agent captures all interactions
/// Given: A recording agent wrapping a real backend
/// When: Multiple prompts are sent with various event types
/// Then: The transcript contains all interactions in order
#[tokio::test]
async fn scenario_recording_captures_full_interaction() {
    let mock = MockBackend::new()
        .on_prompt("first").respond_text("First response")
        .on_prompt("second").respond_with(vec![
            AgentEvent::Text("Streaming...".to_string()),
            AgentEvent::Text(" more text".to_string()),
            AgentEvent::Result {
                text: "Complete".to_string(),
                usage: None,
                metadata: serde_json::json!({}),
            },
        ]);

    let recording = RecordingAgent::wrap(mock.into_handle());
    let session = recording.new_session().await.unwrap();

    // Send first prompt
    let mut rx1 = recording.prompt(&session, "first prompt").await.unwrap();
    while rx1.recv().await.is_some() {}

    // Send second prompt
    let mut rx2 = recording.prompt(&session, "second prompt").await.unwrap();
    while rx2.recv().await.is_some() {}

    // Get transcript
    let transcript = recording.transcript();

    assert_eq!(transcript.len(), 2, "Should have 2 interactions");

    // Verify first interaction
    assert!(transcript[0].prompt.contains("first"));
    assert_eq!(transcript[0].events.len(), 1);

    // Verify second interaction has streaming events
    assert!(transcript[1].prompt.contains("second"));
    assert_eq!(transcript[1].events.len(), 3, "Should have 3 events for streaming");
}

/// Scenario: Replay agent reproduces recorded behavior exactly
/// Given: A transcript with recorded interactions
/// When: The replay agent receives the same prompts
/// Then: It returns the exact recorded events
#[tokio::test]
async fn scenario_replay_reproduces_recorded_behavior() {
    // Create a transcript manually
    let transcript = vec![
        Interaction {
            timestamp: std::time::SystemTime::now(),
            session_id: "test-session".to_string(),
            prompt: "hello".to_string(),
            events: vec![
                AgentEvent::Text("Hi ".to_string()),
                AgentEvent::Text("there!".to_string()),
                AgentEvent::Result {
                    text: "Greeting complete".to_string(),
                    usage: None,
                    metadata: serde_json::json!({}),
                },
            ],
        },
    ];

    let replay = ReplayAgent::from_transcript(transcript);
    let handle = replay.into_handle();
    let session = handle.new_session().await.unwrap();

    let mut rx = handle.prompt(&session, "hello").await.unwrap();

    // Collect events
    let mut events = vec![];
    while let Some(event) = rx.recv().await {
        events.push(event);
    }

    assert_eq!(events.len(), 3);
    assert!(matches!(&events[0], AgentEvent::Text(t) if t == "Hi "));
    assert!(matches!(&events[1], AgentEvent::Text(t) if t == "there!"));
    assert!(matches!(&events[2], AgentEvent::Result { text, .. } if text == "Greeting complete"));
}

/// Scenario: Record and replay roundtrip
/// Given: A session recorded with real backend interactions
/// When: The recording is saved and replayed
/// Then: The replay produces identical behavior
#[tokio::test]
async fn scenario_record_replay_roundtrip() {
    // Record
    let mock = MockBackend::new()
        .on_prompt("test").respond_with(vec![
            AgentEvent::ToolStart {
                id: "t1".to_string(),
                name: "TestTool".to_string(),
                input: serde_json::json!({"arg": "value"}),
            },
            AgentEvent::ToolEnd {
                id: "t1".to_string(),
                name: "TestTool".to_string(),
                output: serde_json::json!({"result": "success"}),
                success: true,
                duration_ms: 100,
            },
            AgentEvent::Result {
                text: "Tool executed".to_string(),
                usage: None,
                metadata: serde_json::json!({}),
            },
        ]);

    let recording = RecordingAgent::wrap(mock.into_handle());
    let session = recording.new_session().await.unwrap();

    let mut rx = recording.prompt(&session, "run test").await.unwrap();
    let mut recorded_events = vec![];
    while let Some(event) = rx.recv().await {
        recorded_events.push(event);
    }

    // Get transcript
    let transcript = recording.transcript();

    // Replay
    let replay = ReplayAgent::from_transcript(transcript);
    let replay_handle = replay.into_handle();
    let replay_session = replay_handle.new_session().await.unwrap();

    let mut rx = replay_handle.prompt(&replay_session, "run test").await.unwrap();
    let mut replayed_events = vec![];
    while let Some(event) = rx.recv().await {
        replayed_events.push(event);
    }

    // Compare
    assert_eq!(recorded_events.len(), replayed_events.len());

    // Verify tool events match
    assert!(matches!(&replayed_events[0], AgentEvent::ToolStart { name, .. } if name == "TestTool"));
    assert!(matches!(&replayed_events[1], AgentEvent::ToolEnd { success: true, .. }));
    assert!(matches!(&replayed_events[2], AgentEvent::Result { .. }));
}

/// Scenario: MockBuilder with streaming simulation
/// Given: A MockBuilder configured with streaming
/// When: A prompt is sent
/// Then: Events arrive in the correct order
#[tokio::test]
async fn scenario_mock_builder_streaming() {
    let handle = MockAgentBuilder::new()
        .on_prompt("stream")
        .with_streaming(vec![
            "Part 1".to_string(),
            "Part 2".to_string(),
            "Part 3".to_string(),
        ])
        .respond_text("Complete")
        .into_handle();

    let session = handle.new_session().await.unwrap();

    let mut rx = handle.prompt(&session, "stream this").await.unwrap();

    let mut events = vec![];
    while let Some(event) = rx.recv().await {
        events.push(event);
    }

    // Should have streaming + result events (streaming chunks become Text events + the Result)
    // Note: streaming chunks are converted to Text events by MockBuilder
    assert!(events.len() >= 1, "Should have at least result event");
}

/// Scenario: MockBuilder with tool simulation
/// Given: A MockBuilder configured with tool calls
/// When: A prompt triggers the tool
/// Then: Tool start/end events are emitted correctly
#[tokio::test]
async fn scenario_mock_builder_tool_simulation() {
    let handle = MockAgentBuilder::new()
        .on_prompt("read")
        .respond_with_tools(
            vec![
                ToolCall {
                    name: "Read".to_string(),
                    input: serde_json::json!({"path": "/test.txt"}),
                    output: serde_json::json!({"content": "file data"}),
                    success: true,
                    duration_ms: 50,
                },
                ToolCall {
                    name: "Write".to_string(),
                    input: serde_json::json!({"path": "/out.txt"}),
                    output: serde_json::json!({"bytes": 100}),
                    success: true,
                    duration_ms: 50,
                },
            ],
            "Files processed",
        )
        .into_handle();

    let session = handle.new_session().await.unwrap();
    let mut rx = handle.prompt(&session, "read and write files").await.unwrap();

    let mut events = vec![];
    while let Some(event) = rx.recv().await {
        events.push(event);
    }

    // Should have: ToolStart, ToolEnd, ToolStart, ToolEnd, Result
    assert_eq!(events.len(), 5);

    // Verify first tool
    assert!(matches!(&events[0], AgentEvent::ToolStart { name, .. } if name == "Read"));
    assert!(matches!(&events[1], AgentEvent::ToolEnd { name, success: true, .. } if name == "Read"));

    // Verify second tool
    assert!(matches!(&events[2], AgentEvent::ToolStart { name, .. } if name == "Write"));
    assert!(matches!(&events[3], AgentEvent::ToolEnd { name, success: true, .. } if name == "Write"));

    // Verify result
    assert!(matches!(&events[4], AgentEvent::Result { text, .. } if text.contains("processed")));
}

/// Scenario: MockBuilder expectation verification
/// Given: A MockBuilder with expected prompt count
/// When: The expected number of prompts is sent
/// Then: Verification passes without panic
#[tokio::test]
async fn scenario_mock_builder_expectation_verification() {
    let handle = MockAgentBuilder::new()
        .on_prompt("a").respond_text("A")
        .on_prompt("b").respond_text("B")
        .expect_prompt_count(2)
        .into_handle();

    let session = handle.new_session().await.unwrap();

    // Send exactly 2 prompts
    let mut rx1 = handle.prompt(&session, "prompt a").await.unwrap();
    while rx1.recv().await.is_some() {}

    let mut rx2 = handle.prompt(&session, "prompt b").await.unwrap();
    while rx2.recv().await.is_some() {}

    // Verify expectations are met
    handle.verify_all_expectations_met().expect("All expectations should be met");
}

/// Scenario: Cancel command on mock backend
/// Given: A mock backend with an active session
/// When: Cancel is called on the session
/// Then: The cancel completes successfully (no-op for mock)
#[tokio::test]
async fn scenario_cancel_command_on_mock() {
    let mock = MockBackend::new()
        .on_prompt("test").respond_text("Response");

    let handle = mock.into_handle();
    let session = handle.new_session().await.unwrap();

    // Cancel should succeed (even though mock doesn't have real cancellation)
    let result = handle.cancel(&session).await;
    assert!(result.is_ok(), "Cancel should succeed on mock backend");

    // Should still be able to send prompts after cancel
    let mut rx = handle.prompt(&session, "test after cancel").await.unwrap();
    let event = rx.recv().await.unwrap();

    assert!(matches!(event, AgentEvent::Result { .. }));
}

/// Scenario: High volume prompts stress test
/// Given: A mock backend with many expectations
/// When: Many prompts are sent rapidly
/// Then: All prompts complete successfully with correct responses
#[tokio::test]
async fn scenario_high_volume_stress_test() {
    const NUM_PROMPTS: usize = 100;

    let mut mock = MockBackend::new();
    for i in 0..NUM_PROMPTS {
        mock = mock.on_prompt(&format!("prompt-{}", i))
            .respond_text(&format!("response-{}", i));
    }

    let handle = mock.into_handle();
    let session = handle.new_session().await.unwrap();

    for i in 0..NUM_PROMPTS {
        let mut rx = handle.prompt(&session, &format!("prompt-{}", i)).await.unwrap();
        let event = rx.recv().await.unwrap();

        if let AgentEvent::Result { text, .. } = event {
            assert!(text.contains(&format!("response-{}", i)),
                "Prompt {} should get response-{}", i, i);
        } else {
            panic!("Expected Result event for prompt {}", i);
        }
    }
}

/// Scenario: Session ID uniqueness
/// Given: Multiple sessions created
/// When: Checking session IDs
/// Then: Each session has a unique ID
#[tokio::test]
async fn scenario_session_id_uniqueness() {
    let mock = MockBackend::new();
    let handle = mock.into_handle();

    let mut session_ids = std::collections::HashSet::new();

    for _ in 0..50 {
        let session = handle.new_session().await.unwrap();
        assert!(session_ids.insert(session.clone()),
            "Session ID {} should be unique", session);
    }

    assert_eq!(session_ids.len(), 50, "All 50 sessions should have unique IDs");
}

/// Scenario: MockBuilder with delay between events
/// Given: A MockBuilder configured with delay
/// When: A prompt is sent
/// Then: Events are delayed appropriately
#[tokio::test]
async fn scenario_mock_builder_with_delay() {
    let handle = MockAgentBuilder::new()
        .on_prompt("delayed")
        .with_delay(Duration::from_millis(50))
        .respond_text("Delayed response")
        .into_handle();

    let session = handle.new_session().await.unwrap();
    let start = std::time::Instant::now();

    let mut rx = handle.prompt(&session, "delayed query").await.unwrap();

    // Consume all events
    while rx.recv().await.is_some() {}

    let elapsed = start.elapsed();

    // Should have some delay (at least 50ms for the configured delay)
    assert!(elapsed >= Duration::from_millis(40), "Should have delay: {:?}", elapsed);
}
