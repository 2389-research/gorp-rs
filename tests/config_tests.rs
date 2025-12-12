// ABOUTME: Tests for configuration loading and validation
// ABOUTME: Verifies TOML parsing, env var overrides, and required field validation

use std::io::Write;

#[test]
fn test_config_loads_from_toml_file() {
    // Clear any env vars that might contaminate this test
    std::env::remove_var("MATRIX_HOME_SERVER");
    std::env::remove_var("MATRIX_PASSWORD");
    std::env::remove_var("MATRIX_USER_ID");

    let temp_dir = std::env::temp_dir().join("gorp-config-test");
    let _ = std::fs::create_dir_all(&temp_dir);
    let config_path = temp_dir.join("config.toml");

    let config_content = r#"
[matrix]
home_server = "https://test.matrix.org"
user_id = "@bot:test.matrix.org"
password = "secret123"
allowed_users = ["@user1:test.matrix.org", "@user2:test.matrix.org"]

[claude]
binary_path = "claude"

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
    std::env::remove_var("GORP_CONFIG_PATH");
    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_config_env_var_overrides() {
    let temp_dir = std::env::temp_dir().join("gorp-config-env-test");
    let _ = std::fs::create_dir_all(&temp_dir);
    let config_path = temp_dir.join("config.toml");

    let config_content = r#"
[matrix]
home_server = "https://original.matrix.org"
user_id = "@bot:original.matrix.org"
password = "original-password"
allowed_users = ["@user:original.matrix.org"]

[claude]
binary_path = "claude"

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
    std::env::remove_var("GORP_CONFIG_PATH");
    std::env::remove_var("MATRIX_HOME_SERVER");
    std::env::remove_var("MATRIX_PASSWORD");
    let _ = std::fs::remove_dir_all(&temp_dir);
}
