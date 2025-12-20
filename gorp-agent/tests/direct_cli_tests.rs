use gorp_agent::backends::direct_cli::DirectCliConfig;

#[test]
fn test_direct_cli_config_deserializes() {
    let json = serde_json::json!({
        "binary": "claude",
        "sdk_url": "http://localhost:8080",
        "working_dir": "/tmp"
    });
    let config: DirectCliConfig = serde_json::from_value(json).unwrap();
    assert_eq!(config.binary, "claude");
    assert_eq!(config.sdk_url, Some("http://localhost:8080".to_string()));
}

#[test]
fn test_direct_cli_config_without_sdk_url() {
    let json = serde_json::json!({
        "binary": "claude",
        "working_dir": "/tmp"
    });
    let config: DirectCliConfig = serde_json::from_value(json).unwrap();
    assert!(config.sdk_url.is_none());
}

#[test]
fn test_direct_cli_config_minimal() {
    let json = serde_json::json!({
        "binary": "/usr/local/bin/claude",
        "working_dir": "/home/user/project"
    });
    let config: DirectCliConfig = serde_json::from_value(json).unwrap();
    assert_eq!(config.binary, "/usr/local/bin/claude");
    assert_eq!(config.working_dir.to_str().unwrap(), "/home/user/project");
}
