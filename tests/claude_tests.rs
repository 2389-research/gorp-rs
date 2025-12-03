#[test]
fn test_parse_claude_response_success() {
    let json = r#"{
        "content": [
            {"type": "text", "text": "Hello, "},
            {"type": "text", "text": "world!"}
        ]
    }"#;

    let result = matrix_bridge::claude::parse_response(json).unwrap();

    assert_eq!(result, "Hello, world!");
}

#[test]
fn test_parse_claude_response_empty() {
    let json = r#"{"content": []}"#;

    let result = matrix_bridge::claude::parse_response(json).unwrap();

    assert_eq!(result, "");
}

#[test]
fn test_parse_claude_response_malformed() {
    let json = "not valid json";

    let result = matrix_bridge::claude::parse_response(json);

    assert!(result.is_err());
}
