// ABOUTME: Scenario tests for backend behavior under real conditions.
// ABOUTME: Tests concurrent access, cancellation, session lifecycle, and error recovery.

use gorp_agent::backends::mock::MockBackend;
use gorp_agent::{AgentEvent, ErrorCode};

/// Scenario: Concurrent prompts to the same new session
/// Given: A newly created session
/// When: Two prompts are sent concurrently before the first completes
/// Then: Only the first prompt should trigger backend initialization (is_new_session=true)
#[tokio::test]
async fn scenario_concurrent_prompts_to_new_session() {
    let mock = MockBackend::new()
        .on_prompt("first").respond_text("Response to first")
        .on_prompt("second").respond_text("Response to second");

    let handle = mock.into_handle();
    let session_id = handle.new_session().await.unwrap();

    // Verify session is being tracked
    assert_eq!(handle.tracked_session_count(), 1, "New session should be tracked");

    // Launch two concurrent prompts
    let handle1 = handle.clone();
    let handle2 = handle.clone();
    let sid1 = session_id.clone();
    let sid2 = session_id.clone();

    let (result1, result2) = tokio::join!(
        async move {
            let mut rx = handle1.prompt(&sid1, "first prompt").await.unwrap();
            let mut events = vec![];
            while let Some(event) = rx.recv().await {
                events.push(event);
            }
            events
        },
        async move {
            let mut rx = handle2.prompt(&sid2, "second prompt").await.unwrap();
            let mut events = vec![];
            while let Some(event) = rx.recv().await {
                events.push(event);
            }
            events
        }
    );

    // Both prompts should complete
    assert!(!result1.is_empty(), "First prompt should get response");
    assert!(!result2.is_empty(), "Second prompt should get response");

    // Session should no longer be tracked (cleaned up after first prompt)
    assert_eq!(handle.tracked_session_count(), 0, "Session should be cleaned up after prompts");
}

/// Scenario: Session abandonment prevents memory leaks
/// Given: Multiple sessions are created
/// When: Some sessions are abandoned without sending prompts
/// Then: Memory should be reclaimed for abandoned sessions
#[tokio::test]
async fn scenario_session_abandonment_cleans_up_memory() {
    let mock = MockBackend::new();
    let handle = mock.into_handle();

    // Create several sessions
    let session1 = handle.new_session().await.unwrap();
    let session2 = handle.new_session().await.unwrap();
    let session3 = handle.new_session().await.unwrap();

    assert_eq!(handle.tracked_session_count(), 3, "All sessions should be tracked");

    // Abandon session2 without using it
    handle.abandon_session(&session2);
    assert_eq!(handle.tracked_session_count(), 2, "Abandoned session should be removed");

    // Abandon a session that doesn't exist (should be safe no-op)
    handle.abandon_session("nonexistent-session");
    assert_eq!(handle.tracked_session_count(), 2, "Abandoning nonexistent session is safe");

    // Abandon remaining sessions
    handle.abandon_session(&session1);
    handle.abandon_session(&session3);
    assert_eq!(handle.tracked_session_count(), 0, "All sessions should be cleaned up");
}

/// Scenario: Loaded sessions are treated differently from new sessions
/// Given: A session ID representing a previously saved session
/// When: The session is loaded and a prompt is sent
/// Then: The backend should treat it as an existing session (is_new_session=false)
#[tokio::test]
async fn scenario_loaded_session_not_marked_as_new() {
    let mock = MockBackend::new()
        .on_prompt("resume").respond_text("Resumed session");

    let handle = mock.into_handle();

    // Load an existing session (simulating resume from disk)
    let session_id = "previously-saved-session-123";
    handle.load_session(session_id).await.unwrap();

    // Loaded sessions are not tracked in the new session map
    assert_eq!(handle.tracked_session_count(), 0, "Loaded session should not be in new session map");

    // Send a prompt - it should work normally
    let mut rx = handle.prompt(session_id, "resume previous work").await.unwrap();
    let event = rx.recv().await.unwrap();

    if let AgentEvent::Result { text, .. } = event {
        assert!(text.contains("Resumed"), "Should get response for loaded session");
    } else {
        panic!("Expected Result event");
    }
}

/// Scenario: Multiple sessions can be active simultaneously
/// Given: Multiple independent sessions
/// When: Prompts are sent to each session concurrently
/// Then: Each session should receive its own responses correctly
#[tokio::test]
async fn scenario_multiple_concurrent_sessions() {
    let mock = MockBackend::new()
        .on_prompt("session-a").respond_text("Response A")
        .on_prompt("session-b").respond_text("Response B")
        .on_prompt("session-c").respond_text("Response C");

    let handle = mock.into_handle();

    // Create multiple sessions
    let session_a = handle.new_session().await.unwrap();
    let session_b = handle.new_session().await.unwrap();
    let session_c = handle.new_session().await.unwrap();

    assert_eq!(handle.tracked_session_count(), 3);

    // Send prompts to all sessions concurrently
    let h1 = handle.clone();
    let h2 = handle.clone();
    let h3 = handle.clone();
    let sa = session_a.clone();
    let sb = session_b.clone();
    let sc = session_c.clone();

    let (ra, rb, rc) = tokio::join!(
        async move {
            let mut rx = h1.prompt(&sa, "session-a query").await.unwrap();
            rx.recv().await
        },
        async move {
            let mut rx = h2.prompt(&sb, "session-b query").await.unwrap();
            rx.recv().await
        },
        async move {
            let mut rx = h3.prompt(&sc, "session-c query").await.unwrap();
            rx.recv().await
        }
    );

    // Verify each session got its expected response
    assert!(matches!(ra, Some(AgentEvent::Result { text, .. }) if text.contains("Response A")));
    assert!(matches!(rb, Some(AgentEvent::Result { text, .. }) if text.contains("Response B")));
    assert!(matches!(rc, Some(AgentEvent::Result { text, .. }) if text.contains("Response C")));

    // All sessions should be cleaned up
    assert_eq!(handle.tracked_session_count(), 0);
}

/// Scenario: Cloned handles share session state
/// Given: An AgentHandle that has been cloned
/// When: A session is created on one handle
/// Then: The session should be visible/usable from the other handle
#[tokio::test]
async fn scenario_cloned_handles_share_session_state() {
    let mock = MockBackend::new()
        .on_prompt("test").respond_text("Shared response");

    let handle1 = mock.into_handle();
    let handle2 = handle1.clone();

    // Create session on handle1
    let session_id = handle1.new_session().await.unwrap();

    // Both handles should see the tracked session
    assert_eq!(handle1.tracked_session_count(), 1);
    assert_eq!(handle2.tracked_session_count(), 1);

    // Use handle2 to send prompt to session created by handle1
    let mut rx = handle2.prompt(&session_id, "test from handle2").await.unwrap();
    let event = rx.recv().await.unwrap();

    assert!(matches!(event, AgentEvent::Result { text, .. } if text.contains("Shared")));

    // Session should be cleaned up on both handles
    assert_eq!(handle1.tracked_session_count(), 0);
    assert_eq!(handle2.tracked_session_count(), 0);
}

/// Scenario: Mock backend FIFO ordering with matching prompts
/// Given: Multiple expectations set up in order
/// When: Prompts arrive that match in order
/// Then: Expectations are consumed in FIFO order
#[tokio::test]
async fn scenario_mock_backend_fifo_ordering() {
    let mock = MockBackend::new()
        .on_prompt("step").respond_text("First step")
        .on_prompt("step").respond_text("Second step")
        .on_prompt("step").respond_text("Third step");

    let handle = mock.into_handle();
    let session = handle.new_session().await.unwrap();

    // Send matching prompts - should get responses in FIFO order
    let mut rx1 = handle.prompt(&session, "step one").await.unwrap();
    let e1 = rx1.recv().await.unwrap();

    let mut rx2 = handle.prompt(&session, "step two").await.unwrap();
    let e2 = rx2.recv().await.unwrap();

    let mut rx3 = handle.prompt(&session, "step three").await.unwrap();
    let e3 = rx3.recv().await.unwrap();

    assert!(matches!(e1, AgentEvent::Result { text, .. } if text.contains("First")));
    assert!(matches!(e2, AgentEvent::Result { text, .. } if text.contains("Second")));
    assert!(matches!(e3, AgentEvent::Result { text, .. } if text.contains("Third")));
}

/// Scenario: Mock backend fallback when front doesn't match
/// Given: Expectations that match different patterns
/// When: A prompt matches a non-front expectation
/// Then: The matching expectation is found and consumed
#[tokio::test]
async fn scenario_mock_backend_fallback_matching() {
    let mock = MockBackend::new()
        .on_prompt("alpha").respond_text("Alpha response")
        .on_prompt("beta").respond_text("Beta response")
        .on_prompt("gamma").respond_text("Gamma response");

    let handle = mock.into_handle();
    let session = handle.new_session().await.unwrap();

    // Send "beta" first - should skip "alpha" and find "beta"
    let mut rx = handle.prompt(&session, "I want beta").await.unwrap();
    let event = rx.recv().await.unwrap();

    assert!(matches!(event, AgentEvent::Result { text, .. } if text.contains("Beta")));

    // Now send "alpha" - should find it
    let mut rx = handle.prompt(&session, "now alpha").await.unwrap();
    let event = rx.recv().await.unwrap();

    assert!(matches!(event, AgentEvent::Result { text, .. } if text.contains("Alpha")));

    // Finally "gamma"
    let mut rx = handle.prompt(&session, "finally gamma").await.unwrap();
    let event = rx.recv().await.unwrap();

    assert!(matches!(event, AgentEvent::Result { text, .. } if text.contains("Gamma")));
}

/// Scenario: Mock backend handles unmatched prompts gracefully
/// Given: No expectations match the prompt
/// When: A prompt is sent
/// Then: A descriptive error response is returned
#[tokio::test]
async fn scenario_mock_backend_unmatched_prompt() {
    let mock = MockBackend::new()
        .on_prompt("specific").respond_text("Matched");

    let handle = mock.into_handle();
    let session = handle.new_session().await.unwrap();

    // Send a prompt that doesn't match
    let mut rx = handle.prompt(&session, "completely different").await.unwrap();
    let event = rx.recv().await.unwrap();

    // Should get a "no expectation" message
    if let AgentEvent::Result { text, .. } = event {
        assert!(text.contains("no expectation"), "Should indicate no match found: {}", text);
    } else {
        panic!("Expected Result event for unmatched prompt");
    }
}

/// Scenario: Mock backend with tool events
/// Given: An expectation that includes tool start/end events
/// When: A prompt triggers the expectation
/// Then: All tool events are streamed in order
#[tokio::test]
async fn scenario_mock_backend_tool_events() {
    let mock = MockBackend::new()
        .on_prompt("read file").respond_with(vec![
            AgentEvent::ToolStart {
                id: "tool-1".to_string(),
                name: "Read".to_string(),
                input: serde_json::json!({"path": "/tmp/test.txt"}),
            },
            AgentEvent::ToolEnd {
                id: "tool-1".to_string(),
                name: "Read".to_string(),
                output: serde_json::json!({"content": "file contents here"}),
                success: true,
                duration_ms: 42,
            },
            AgentEvent::Result {
                text: "I read the file for you".to_string(),
                usage: None,
                metadata: serde_json::json!({}),
            },
        ]);

    let handle = mock.into_handle();
    let session = handle.new_session().await.unwrap();

    let mut rx = handle.prompt(&session, "please read file /tmp/test.txt").await.unwrap();

    // Collect all events
    let mut events = vec![];
    while let Some(event) = rx.recv().await {
        events.push(event);
    }

    assert_eq!(events.len(), 3, "Should receive all 3 events");

    // Verify event order and types
    assert!(matches!(&events[0], AgentEvent::ToolStart { name, .. } if name == "Read"));
    assert!(matches!(&events[1], AgentEvent::ToolEnd { success: true, .. }));
    assert!(matches!(&events[2], AgentEvent::Result { .. }));
}

/// Scenario: Mock backend error responses
/// Given: An expectation configured to return an error
/// When: A matching prompt is sent
/// Then: The error event is returned with correct code and message
#[tokio::test]
async fn scenario_mock_backend_error_response() {
    let mock = MockBackend::new()
        .on_prompt("fail").respond_error(ErrorCode::BackendError, "Simulated backend failure");

    let handle = mock.into_handle();
    let session = handle.new_session().await.unwrap();

    let mut rx = handle.prompt(&session, "this should fail").await.unwrap();
    let event = rx.recv().await.unwrap();

    if let AgentEvent::Error { code, message, recoverable } = event {
        assert_eq!(code, ErrorCode::BackendError);
        assert!(message.contains("Simulated"));
        assert!(!recoverable);
    } else {
        panic!("Expected Error event");
    }
}

/// Scenario: Rapid session creation and usage
/// Given: High-frequency session creation
/// When: Sessions are created, used, and discarded rapidly
/// Then: No memory leaks or race conditions occur
#[tokio::test]
async fn scenario_rapid_session_lifecycle() {
    let mock = MockBackend::new()
        .on_prompt("ping").respond_text("pong")
        .on_prompt("ping").respond_text("pong")
        .on_prompt("ping").respond_text("pong")
        .on_prompt("ping").respond_text("pong")
        .on_prompt("ping").respond_text("pong")
        .on_prompt("ping").respond_text("pong")
        .on_prompt("ping").respond_text("pong")
        .on_prompt("ping").respond_text("pong")
        .on_prompt("ping").respond_text("pong")
        .on_prompt("ping").respond_text("pong");

    let handle = mock.into_handle();

    // Rapidly create and use sessions
    for i in 0..10 {
        let session = handle.new_session().await.unwrap();
        assert!(handle.tracked_session_count() <= 1, "At most 1 session tracked at a time");

        let mut rx = handle.prompt(&session, &format!("ping {}", i)).await.unwrap();
        let event = rx.recv().await.unwrap();

        assert!(matches!(event, AgentEvent::Result { text, .. } if text.contains("pong")));
    }

    // All sessions should be cleaned up
    assert_eq!(handle.tracked_session_count(), 0, "All sessions cleaned up after rapid usage");
}

/// Scenario: Backend name is correctly reported
/// Given: Different backend types
/// When: Querying the backend name
/// Then: Each backend reports its correct identifier
#[tokio::test]
async fn scenario_backend_name_identification() {
    let mock = MockBackend::new();
    let handle = mock.into_handle();

    assert_eq!(handle.name(), "mock", "Mock backend should identify as 'mock'");
}

/// Scenario: Empty session ID handling
/// Given: An edge case with empty string session ID
/// When: Operations are performed with empty session ID
/// Then: Operations should handle gracefully (no panic)
#[tokio::test]
async fn scenario_empty_session_id_handling() {
    let mock = MockBackend::new()
        .on_prompt("test").respond_text("OK");

    let handle = mock.into_handle();

    // Load session with empty ID (edge case)
    let result = handle.load_session("").await;
    assert!(result.is_ok(), "Empty session ID load should not panic");

    // Prompt with empty session ID
    let mut rx = handle.prompt("", "test").await.unwrap();
    let event = rx.recv().await.unwrap();

    assert!(matches!(event, AgentEvent::Result { .. }), "Should still get response");

    // Abandon empty session ID
    handle.abandon_session(""); // Should not panic
}
