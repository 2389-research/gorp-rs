// ABOUTME: Tests for the scenario test runner functionality.
// ABOUTME: Covers loading scenarios, matching events, and running scenarios against backends.

use gorp_agent::testing::scenarios::{EventMatcher, Scenario};
use gorp_agent::testing::scenarios::{load_scenario, run_scenario};
use gorp_agent::backends::mock::MockBackend;
use gorp_agent::{AgentEvent, ErrorCode};
use serde_json::json;

#[test]
fn test_deserialize_simple_scenario() {
    let json = r#"{
        "name": "simple_test",
        "prompt": "Say hello",
        "expected_events": [
            {"type": "Result", "contains": "hello"}
        ]
    }"#;

    let scenario: Scenario = serde_json::from_str(json).unwrap();
    assert_eq!(scenario.name, "simple_test");
    assert_eq!(scenario.prompt, "Say hello");
    assert_eq!(scenario.expected_events.len(), 1);
    assert!(scenario.description.is_none());
    assert!(scenario.timeout_ms.is_none());
}

#[test]
fn test_deserialize_full_scenario() {
    let json = r#"{
        "name": "full_test",
        "description": "A comprehensive test",
        "setup": {
            "files": {
                "/tmp/test.txt": "test content"
            },
            "mcp_servers": ["memory", "notes"]
        },
        "prompt": "Read the file",
        "expected_events": [
            {"type": "ToolStart", "name": "Read", "input_contains": {"path": "/tmp/test.txt"}},
            {"type": "ToolEnd", "name": "Read", "success": true},
            {"type": "Result", "contains": "test content"}
        ],
        "assertions": {
            "files": {
                "/tmp/output.txt": {
                    "contains": "success"
                }
            }
        },
        "timeout_ms": 5000
    }"#;

    let scenario: Scenario = serde_json::from_str(json).unwrap();
    assert_eq!(scenario.name, "full_test");
    assert_eq!(scenario.description, Some("A comprehensive test".to_string()));
    assert_eq!(scenario.timeout_ms, Some(5000));

    let setup = scenario.setup.as_ref().unwrap();
    assert!(setup.files.is_some());
    assert!(setup.mcp_servers.is_some());

    assert_eq!(scenario.expected_events.len(), 3);
}

#[test]
fn test_event_matcher_text_contains() {
    let matcher = EventMatcher::Text {
        contains: "hello".to_string(),
    };

    let event = AgentEvent::Text("hello world".to_string());
    assert!(matcher.matches(&event));

    let event = AgentEvent::Text("goodbye world".to_string());
    assert!(!matcher.matches(&event));
}

#[test]
fn test_event_matcher_result_contains() {
    let matcher = EventMatcher::Result {
        contains: "success".to_string(),
    };

    let event = AgentEvent::Result {
        text: "Operation was successful".to_string(),
        usage: None,
        metadata: json!({}),
    };
    assert!(matcher.matches(&event));

    let event = AgentEvent::Result {
        text: "Operation failed".to_string(),
        usage: None,
        metadata: json!({}),
    };
    assert!(!matcher.matches(&event));
}

#[test]
fn test_event_matcher_tool_start() {
    let matcher = EventMatcher::ToolStart {
        name: "Read".to_string(),
        input_contains: Some(json!({"path": "/tmp/test.txt"})),
    };

    let event = AgentEvent::ToolStart {
        id: "t1".to_string(),
        name: "Read".to_string(),
        input: json!({"path": "/tmp/test.txt", "other": "value"}),
    };
    assert!(matcher.matches(&event));

    // Wrong tool name
    let event = AgentEvent::ToolStart {
        id: "t1".to_string(),
        name: "Write".to_string(),
        input: json!({"path": "/tmp/test.txt"}),
    };
    assert!(!matcher.matches(&event));

    // Missing expected field
    let event = AgentEvent::ToolStart {
        id: "t1".to_string(),
        name: "Read".to_string(),
        input: json!({"different": "field"}),
    };
    assert!(!matcher.matches(&event));
}

#[test]
fn test_event_matcher_tool_end() {
    let matcher = EventMatcher::ToolEnd {
        name: "Read".to_string(),
        success: true,
    };

    let event = AgentEvent::ToolEnd {
        id: "t1".to_string(),
        name: "Read".to_string(),
        output: json!({}),
        success: true,
        duration_ms: 100,
    };
    assert!(matcher.matches(&event));

    // Wrong success state
    let event = AgentEvent::ToolEnd {
        id: "t1".to_string(),
        name: "Read".to_string(),
        output: json!({}),
        success: false,
        duration_ms: 100,
    };
    assert!(!matcher.matches(&event));
}

#[test]
fn test_event_matcher_error() {
    let matcher = EventMatcher::Error {
        code: Some(ErrorCode::Timeout),
    };

    let event = AgentEvent::Error {
        code: ErrorCode::Timeout,
        message: "Request timed out".to_string(),
        recoverable: true,
    };
    assert!(matcher.matches(&event));

    // Different error code
    let event = AgentEvent::Error {
        code: ErrorCode::RateLimited,
        message: "Too many requests".to_string(),
        recoverable: true,
    };
    assert!(!matcher.matches(&event));

    // Match any error
    let matcher = EventMatcher::Error { code: None };
    assert!(matcher.matches(&event));
}

#[test]
fn test_event_matcher_custom() {
    let matcher = EventMatcher::Custom {
        kind: "test_event".to_string(),
    };

    let event = AgentEvent::Custom {
        kind: "test_event".to_string(),
        payload: json!({}),
    };
    assert!(matcher.matches(&event));

    let event = AgentEvent::Custom {
        kind: "different_event".to_string(),
        payload: json!({}),
    };
    assert!(!matcher.matches(&event));
}

#[test]
fn test_event_matcher_any() {
    let matcher = EventMatcher::Any { count: 2 };

    let event = AgentEvent::Text("anything".to_string());
    assert!(matcher.matches(&event));

    let event = AgentEvent::Result {
        text: "result".to_string(),
        usage: None,
        metadata: json!({}),
    };
    assert!(matcher.matches(&event));
}

#[tokio::test]
async fn test_run_simple_scenario() {
    let scenario = Scenario {
        name: "simple_hello".to_string(),
        description: None,
        setup: None,
        prompt: "Say hello".to_string(),
        expected_events: vec![
            EventMatcher::Result {
                contains: "hello".to_string(),
            },
        ],
        assertions: None,
        timeout_ms: None,
    };

    let mock = MockBackend::new()
        .on_prompt("Say hello")
        .respond_text("hello world");

    let handle = mock.into_handle();
    let result = run_scenario(&handle, &scenario).await;

    assert!(result.passed, "Failures: {:?}", result.failures);
    assert_eq!(result.name, "simple_hello");
    assert!(result.failures.is_empty());
}

#[tokio::test]
async fn test_run_scenario_with_tool_events() {
    let scenario = Scenario {
        name: "tool_test".to_string(),
        description: None,
        setup: None,
        prompt: "read file".to_string(),
        expected_events: vec![
            EventMatcher::ToolStart {
                name: "Read".to_string(),
                input_contains: None,
            },
            EventMatcher::ToolEnd {
                name: "Read".to_string(),
                success: true,
            },
            EventMatcher::Result {
                contains: "file".to_string(),
            },
        ],
        assertions: None,
        timeout_ms: None,
    };

    let mock = MockBackend::new()
        .on_prompt("read file")
        .respond_with(vec![
            AgentEvent::ToolStart {
                id: "t1".to_string(),
                name: "Read".to_string(),
                input: json!({"path": "/tmp/test"}),
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
    let result = run_scenario(&handle, &scenario).await;

    assert!(result.passed, "Failures: {:?}", result.failures);
}

#[tokio::test]
async fn test_run_scenario_failure() {
    let scenario = Scenario {
        name: "failing_test".to_string(),
        description: None,
        setup: None,
        prompt: "test".to_string(),
        expected_events: vec![
            EventMatcher::Result {
                contains: "expected text".to_string(),
            },
        ],
        assertions: None,
        timeout_ms: None,
    };

    let mock = MockBackend::new()
        .on_prompt("test")
        .respond_text("different text");

    let handle = mock.into_handle();
    let result = run_scenario(&handle, &scenario).await;

    assert!(!result.passed);
    assert!(!result.failures.is_empty());
}

#[tokio::test]
async fn test_run_scenario_with_any_matcher() {
    let scenario = Scenario {
        name: "any_test".to_string(),
        description: None,
        setup: None,
        prompt: "stream".to_string(),
        expected_events: vec![
            EventMatcher::Any { count: 2 },
            EventMatcher::Result {
                contains: "done".to_string(),
            },
        ],
        assertions: None,
        timeout_ms: None,
    };

    let mock = MockBackend::new()
        .on_prompt("stream")
        .respond_with(vec![
            AgentEvent::Text("Hello ".to_string()),
            AgentEvent::Text("world!".to_string()),
            AgentEvent::Result {
                text: "done".to_string(),
                usage: None,
                metadata: json!({}),
            },
        ]);

    let handle = mock.into_handle();
    let result = run_scenario(&handle, &scenario).await;

    assert!(result.passed, "Failures: {:?}", result.failures);
}

#[test]
fn test_load_scenario_from_json() {
    // Create a temporary scenario file
    let json = r#"{
        "name": "test_scenario",
        "prompt": "Test prompt",
        "expected_events": [
            {"type": "Result", "contains": "success"}
        ]
    }"#;

    let temp_dir = std::env::temp_dir();
    let scenario_path = temp_dir.join("test_scenario.json");
    std::fs::write(&scenario_path, json).unwrap();

    let scenario = load_scenario(&scenario_path).unwrap();
    assert_eq!(scenario.name, "test_scenario");
    assert_eq!(scenario.prompt, "Test prompt");

    // Cleanup
    std::fs::remove_file(&scenario_path).ok();
}
