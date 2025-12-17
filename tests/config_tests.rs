// ABOUTME: Tests for configuration loading and validation
// ABOUTME: Verifies TOML parsing, env var overrides, and required field validation

use serial_test::serial;
use std::io::Write;

/// Helper to clear all config-related env vars
fn clear_config_env_vars() {
    std::env::remove_var("GORP_CONFIG_PATH");
    std::env::remove_var("MATRIX_HOME_SERVER");
    std::env::remove_var("MATRIX_PASSWORD");
    std::env::remove_var("MATRIX_USER_ID");
    std::env::remove_var("MATRIX_ACCESS_TOKEN");
    std::env::remove_var("MATRIX_DEVICE_NAME");
    std::env::remove_var("ALLOWED_USERS");
}

#[test]
#[serial]
fn test_config_loads_from_toml_file() {
    // Clear ALL config env vars to prevent test contamination
    clear_config_env_vars();

    let temp_dir = std::env::temp_dir().join("gorp-config-test");
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir).unwrap();
    let config_path = temp_dir.join("config.toml");

    let config_content = r#"
[matrix]
home_server = "https://test.matrix.org"
user_id = "@bot:test.matrix.org"
password = "secret123"
allowed_users = ["@user1:test.matrix.org", "@user2:test.matrix.org"]

[acp]
agent_binary = "claude"

[webhook]
port = 8080

[workspace]
path = "./test-workspace"
"#;

    let mut file = std::fs::File::create(&config_path).unwrap();
    file.write_all(config_content.as_bytes()).unwrap();

    // Set the config path env var
    std::env::set_var("GORP_CONFIG_PATH", config_path.to_str().unwrap());

    let config = gorp::config::Config::load().unwrap();

    assert_eq!(config.matrix.home_server, "https://test.matrix.org");
    assert_eq!(config.matrix.user_id, "@bot:test.matrix.org");
    assert_eq!(config.matrix.password, Some("secret123".to_string()));
    assert_eq!(config.matrix.allowed_users.len(), 2);
    assert!(config.matrix.allowed_users.contains(&"@user1:test.matrix.org".to_string()));
    assert_eq!(config.webhook.port, 8080);

    // Cleanup
    clear_config_env_vars();
    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
#[serial]
fn test_config_env_var_overrides() {
    // Clear ALL config env vars first
    clear_config_env_vars();

    let temp_dir = std::env::temp_dir().join("gorp-config-env-test");
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir).unwrap();
    let config_path = temp_dir.join("config.toml");

    let config_content = r#"
[matrix]
home_server = "https://original.matrix.org"
user_id = "@bot:original.matrix.org"
password = "original-password"
allowed_users = ["@user:original.matrix.org"]

[acp]
agent_binary = "claude"

[webhook]
port = 8080

[workspace]
path = "./workspace"
"#;

    let mut file = std::fs::File::create(&config_path).unwrap();
    file.write_all(config_content.as_bytes()).unwrap();

    std::env::set_var("GORP_CONFIG_PATH", config_path.to_str().unwrap());
    std::env::set_var("MATRIX_HOME_SERVER", "https://override.matrix.org");
    std::env::set_var("MATRIX_PASSWORD", "override-password");

    let config = gorp::config::Config::load().unwrap();

    // Env vars should override TOML values
    assert_eq!(config.matrix.home_server, "https://override.matrix.org");
    assert_eq!(config.matrix.password, Some("override-password".to_string()));

    // Cleanup
    clear_config_env_vars();
    let _ = std::fs::remove_dir_all(&temp_dir);
}
