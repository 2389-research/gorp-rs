// ABOUTME: Tests for the enhanced MockAgentBuilder testing utilities.
// ABOUTME: Validates tool sequence helpers, delays, streaming, and expectation verification.

use gorp_agent::testing::mock_builder::{MockAgentBuilder, ToolCall};
use gorp_agent::AgentEvent;
use serde_json::json;
use std::time::{Duration, Instant};

#[tokio::test]
async fn test_mock_builder_basic_text_response() {
    let mock = MockAgentBuilder::new()
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
async fn test_respond_with_tools_single_tool() {
    let tools = vec![ToolCall {
        name: "Read".to_string(),
        input: json!({"path": "/tmp/test.txt"}),
        output: json!({"content": "file contents"}),
        success: true,
        duration_ms: 10,
    }];

    let mock = MockAgentBuilder::new()
        .on_prompt("read the file")
        .respond_with_tools(tools, "Read the file successfully");

    let handle = mock.into_handle();
    let session_id = handle.new_session().await.unwrap();
    let mut receiver = handle.prompt(&session_id, "read the file").await.unwrap();

    let mut events = vec![];
    while let Some(e) = receiver.recv().await {
        events.push(e);
    }

    assert_eq!(events.len(), 3); // ToolStart, ToolEnd, Result
    assert!(matches!(&events[0], AgentEvent::ToolStart { name, .. } if name == "Read"));
    assert!(matches!(
        &events[1],
        AgentEvent::ToolEnd {
            success: true,
            duration_ms: 10,
            ..
        }
    ));
    match &events[2] {
        AgentEvent::Result { text, .. } => assert_eq!(text, "Read the file successfully"),
        _ => panic!("Expected Result event"),
    }
}

#[tokio::test]
async fn test_respond_with_tools_multiple_tools() {
    let tools = vec![
        ToolCall {
            name: "Read".to_string(),
            input: json!({"path": "/tmp/test.txt"}),
            output: json!({"content": "foo"}),
            success: true,
            duration_ms: 5,
        },
        ToolCall {
            name: "Write".to_string(),
            input: json!({"path": "/tmp/output.txt", "content": "foo"}),
            output: json!({}),
            success: true,
            duration_ms: 8,
        },
    ];

    let mock = MockAgentBuilder::new()
        .on_prompt("copy file")
        .respond_with_tools(tools, "Copied the file");

    let handle = mock.into_handle();
    let session_id = handle.new_session().await.unwrap();
    let mut receiver = handle.prompt(&session_id, "copy file").await.unwrap();

    let mut events = vec![];
    while let Some(e) = receiver.recv().await {
        events.push(e);
    }

    assert_eq!(events.len(), 5); // 2 ToolStart, 2 ToolEnd, 1 Result
    assert!(matches!(&events[0], AgentEvent::ToolStart { name, .. } if name == "Read"));
    assert!(matches!(&events[1], AgentEvent::ToolEnd { name, .. } if name == "Read"));
    assert!(matches!(&events[2], AgentEvent::ToolStart { name, .. } if name == "Write"));
    assert!(matches!(&events[3], AgentEvent::ToolEnd { name, .. } if name == "Write"));
    assert!(matches!(&events[4], AgentEvent::Result { .. }));
}

#[tokio::test]
async fn test_respond_with_tools_failed_tool() {
    let tools = vec![ToolCall {
        name: "Bash".to_string(),
        input: json!({"command": "false"}),
        output: json!({"exit_code": 1}),
        success: false,
        duration_ms: 50,
    }];

    let mock = MockAgentBuilder::new()
        .on_prompt("run command")
        .respond_with_tools(tools, "Command failed");

    let handle = mock.into_handle();
    let session_id = handle.new_session().await.unwrap();
    let mut receiver = handle.prompt(&session_id, "run command").await.unwrap();

    let mut events = vec![];
    while let Some(e) = receiver.recv().await {
        events.push(e);
    }

    assert_eq!(events.len(), 3);
    assert!(matches!(
        &events[1],
        AgentEvent::ToolEnd { success: false, .. }
    ));
}

#[tokio::test]
async fn test_with_delay() {
    let mock = MockAgentBuilder::new()
        .on_prompt("slow")
        .with_delay(Duration::from_millis(100))
        .respond_text("Done!");

    let handle = mock.into_handle();
    let session_id = handle.new_session().await.unwrap();

    let start = Instant::now();
    let mut receiver = handle.prompt(&session_id, "slow").await.unwrap();

    let event = receiver.recv().await.unwrap();
    let elapsed = start.elapsed();

    match event {
        AgentEvent::Result { text, .. } => assert_eq!(text, "Done!"),
        _ => panic!("Expected Result event"),
    }

    // Should take at least 100ms
    assert!(
        elapsed >= Duration::from_millis(100),
        "Expected delay of at least 100ms, got {:?}",
        elapsed
    );
}

#[tokio::test]
async fn test_with_streaming() {
    let chunks = vec!["Hello ".to_string(), "world".to_string(), "!".to_string()];

    let mock = MockAgentBuilder::new()
        .on_prompt("stream")
        .with_streaming(chunks)
        .respond_text("Hello world!");

    let handle = mock.into_handle();
    let session_id = handle.new_session().await.unwrap();
    let mut receiver = handle.prompt(&session_id, "stream").await.unwrap();

    let mut events = vec![];
    while let Some(e) = receiver.recv().await {
        events.push(e);
    }

    // 3 Text events + 1 Result event
    assert_eq!(events.len(), 4);
    assert!(matches!(&events[0], AgentEvent::Text(s) if s == "Hello "));
    assert!(matches!(&events[1], AgentEvent::Text(s) if s == "world"));
    assert!(matches!(&events[2], AgentEvent::Text(s) if s == "!"));
    assert!(matches!(&events[3], AgentEvent::Result { .. }));
}

#[tokio::test]
async fn test_expect_prompt_count_success() {
    let mock = MockAgentBuilder::new()
        .on_prompt("first")
        .respond_text("1")
        .on_prompt("second")
        .respond_text("2")
        .expect_prompt_count(2);

    let handle = mock.into_handle();
    let session_id = handle.new_session().await.unwrap();

    // Drain first prompt
    let mut receiver1 = handle.prompt(&session_id, "first").await.unwrap();
    while receiver1.recv().await.is_some() {}

    // Drain second prompt
    let mut receiver2 = handle.prompt(&session_id, "second").await.unwrap();
    while receiver2.recv().await.is_some() {}

    // Should succeed since we sent exactly 2 prompts
    handle.verify_all_expectations_met().unwrap();
}

#[tokio::test]
#[should_panic(expected = "Expected 2 prompts but received 1")]
async fn test_expect_prompt_count_failure_too_few() {
    let mock = MockAgentBuilder::new()
        .on_prompt("first")
        .respond_text("1")
        .expect_prompt_count(2);

    let handle = mock.into_handle();
    let session_id = handle.new_session().await.unwrap();

    // Drain the prompt
    let mut receiver = handle.prompt(&session_id, "first").await.unwrap();
    while receiver.recv().await.is_some() {}

    // Should panic - only sent 1 prompt but expected 2
    handle.verify_all_expectations_met().unwrap();
}

#[tokio::test]
#[should_panic(expected = "Expected 1 prompts but received 2")]
async fn test_expect_prompt_count_failure_too_many() {
    let mock = MockAgentBuilder::new()
        .on_prompt("first")
        .respond_text("1")
        .on_prompt("second")
        .respond_text("2")
        .expect_prompt_count(1);

    let handle = mock.into_handle();
    let session_id = handle.new_session().await.unwrap();

    // Drain first prompt
    let mut receiver1 = handle.prompt(&session_id, "first").await.unwrap();
    while receiver1.recv().await.is_some() {}

    // Drain second prompt
    let mut receiver2 = handle.prompt(&session_id, "second").await.unwrap();
    while receiver2.recv().await.is_some() {}

    // Should panic - sent 2 prompts but expected 1
    handle.verify_all_expectations_met().unwrap();
}

#[tokio::test]
#[should_panic(expected = "Not all expectations were consumed")]
async fn test_verify_all_expectations_met_failure() {
    let mock = MockAgentBuilder::new()
        .on_prompt("first")
        .respond_text("1")
        .on_prompt("second")
        .respond_text("2");

    let handle = mock.into_handle();
    let session_id = handle.new_session().await.unwrap();

    // Only consume one expectation - drain it
    let mut receiver = handle.prompt(&session_id, "first").await.unwrap();
    while receiver.recv().await.is_some() {}

    // Should panic - still have one unconsumed expectation
    handle.verify_all_expectations_met().unwrap();
}

#[tokio::test]
async fn test_verify_all_expectations_met_success() {
    let mock = MockAgentBuilder::new()
        .on_prompt("first")
        .respond_text("1")
        .on_prompt("second")
        .respond_text("2");

    let handle = mock.into_handle();
    let session_id = handle.new_session().await.unwrap();

    // Drain first prompt
    let mut receiver1 = handle.prompt(&session_id, "first").await.unwrap();
    while receiver1.recv().await.is_some() {}

    // Drain second prompt
    let mut receiver2 = handle.prompt(&session_id, "second").await.unwrap();
    while receiver2.recv().await.is_some() {}

    // Should succeed - all expectations consumed
    handle.verify_all_expectations_met().unwrap();
}

#[tokio::test]
async fn test_combined_features_delay_and_streaming() {
    let chunks = vec!["Thinking".to_string(), "...".to_string()];

    let mock = MockAgentBuilder::new()
        .on_prompt("think")
        .with_delay(Duration::from_millis(50))
        .with_streaming(chunks)
        .respond_text("Thinking...");

    let handle = mock.into_handle();
    let session_id = handle.new_session().await.unwrap();

    let start = Instant::now();
    let mut receiver = handle.prompt(&session_id, "think").await.unwrap();

    let mut events = vec![];
    while let Some(e) = receiver.recv().await {
        events.push(e);
    }

    let elapsed = start.elapsed();

    assert_eq!(events.len(), 3); // 2 Text + 1 Result
    assert!(elapsed >= Duration::from_millis(50));
}

#[tokio::test]
async fn test_combined_features_tools_and_streaming() {
    let tools = vec![ToolCall {
        name: "Search".to_string(),
        input: json!({"query": "test"}),
        output: json!({"results": []}),
        success: true,
        duration_ms: 20,
    }];

    let chunks = vec!["Found ".to_string(), "results".to_string()];

    let mock = MockAgentBuilder::new()
        .on_prompt("search")
        .with_streaming(chunks)
        .respond_with_tools(tools, "Found results");

    let handle = mock.into_handle();
    let session_id = handle.new_session().await.unwrap();
    let mut receiver = handle.prompt(&session_id, "search").await.unwrap();

    let mut events = vec![];
    while let Some(e) = receiver.recv().await {
        events.push(e);
    }

    // 2 Text + 1 ToolStart + 1 ToolEnd + 1 Result
    assert_eq!(events.len(), 5);
    assert!(matches!(&events[0], AgentEvent::Text(s) if s == "Found "));
    assert!(matches!(&events[1], AgentEvent::Text(s) if s == "results"));
    assert!(matches!(&events[2], AgentEvent::ToolStart { .. }));
    assert!(matches!(&events[3], AgentEvent::ToolEnd { .. }));
    assert!(matches!(&events[4], AgentEvent::Result { .. }));
}
