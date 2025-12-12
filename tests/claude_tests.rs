// ABOUTME: Tests for Claude CLI response parsing
// ABOUTME: Verifies JSON response parsing including result extraction and error handling

#[test]
fn test_parse_claude_response_success() {
    // Current format uses "result" field directly
    let json = r#"{"result": "Hello, world!"}"#;

    let result = gorp::claude::parse_response(json).unwrap();

    assert_eq!(result, "Hello, world!");
}

#[test]
fn test_parse_claude_response_empty_result() {
    let json = r#"{"result": ""}"#;

    let result = gorp::claude::parse_response(json).unwrap();

    assert_eq!(result, "");
}

#[test]
fn test_parse_claude_response_malformed() {
    let json = "not valid json";

    let result = gorp::claude::parse_response(json);

    assert!(result.is_err());
}

#[test]
fn test_parse_claude_response_error_during_execution() {
    let json = r#"{
        "subtype": "error_during_execution",
        "is_error": true
    }"#;

    let result = gorp::claude::parse_response(json);

    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(err_msg.contains("error during execution"));
}

#[test]
fn test_parse_claude_response_with_permission_denials() {
    let json = r#"{
        "subtype": "error_during_execution",
        "is_error": true,
        "permission_denials": [
            {"tool_name": "Bash", "reason": "User denied"}
        ]
    }"#;

    let result = gorp::claude::parse_response(json);

    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(err_msg.contains("Permission Denials"));
    assert!(err_msg.contains("Bash"));
}
