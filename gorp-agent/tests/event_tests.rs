use gorp_agent::{AgentEvent, ErrorCode, Usage};
use serde_json::json;

#[test]
fn test_text_event_serializes() {
    let event = AgentEvent::Text("hello".to_string());
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json, json!({"Text": "hello"}));
}

#[test]
fn test_tool_start_event_serializes() {
    let event = AgentEvent::ToolStart {
        id: "tool-1".to_string(),
        name: "Read".to_string(),
        input: json!({"path": "/tmp/foo.txt"}),
    };
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["ToolStart"]["name"], "Read");
}

#[test]
fn test_tool_end_event_serializes() {
    let event = AgentEvent::ToolEnd {
        id: "tool-1".to_string(),
        name: "Read".to_string(),
        output: json!({"content": "file contents"}),
        success: true,
        duration_ms: 42,
    };
    let json = serde_json::to_value(&event).unwrap();
    assert!(json["ToolEnd"]["success"].as_bool().unwrap());
    assert_eq!(json["ToolEnd"]["duration_ms"], 42);
}

#[test]
fn test_result_event_with_usage() {
    let event = AgentEvent::Result {
        text: "Done!".to_string(),
        usage: Some(Usage {
            input_tokens: 100,
            output_tokens: 50,
            cache_read_tokens: Some(10),
            cache_write_tokens: None,
            cost_usd: Some(0.001),
            extra: None,
        }),
        metadata: json!({}),
    };
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["Result"]["usage"]["input_tokens"], 100);
}

#[test]
fn test_error_event_with_code() {
    let event = AgentEvent::Error {
        code: ErrorCode::Timeout,
        message: "Request timed out".to_string(),
        recoverable: true,
    };
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["Error"]["code"], "Timeout");
}

#[test]
fn test_custom_event_extensibility() {
    let event = AgentEvent::Custom {
        kind: "acp.thought_chunk".to_string(),
        payload: json!({"text": "thinking..."}),
    };
    let json = serde_json::to_value(&event).unwrap();
    assert_eq!(json["Custom"]["kind"], "acp.thought_chunk");
}

#[test]
fn test_event_deserializes_roundtrip() {
    let event = AgentEvent::ToolStart {
        id: "t1".to_string(),
        name: "Bash".to_string(),
        input: json!({"command": "ls"}),
    };
    let json_str = serde_json::to_string(&event).unwrap();
    let parsed: AgentEvent = serde_json::from_str(&json_str).unwrap();
    match parsed {
        AgentEvent::ToolStart { name, .. } => assert_eq!(name, "Bash"),
        _ => panic!("Wrong variant"),
    }
}
