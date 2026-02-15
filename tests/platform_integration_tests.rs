// ABOUTME: Integration tests for multi-platform configuration and feature gating
// ABOUTME: Tests config loading with different platform combinations

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
    std::env::remove_var("TELEGRAM_BOT_TOKEN");
    std::env::remove_var("SLACK_BOT_TOKEN");
    std::env::remove_var("SLACK_APP_TOKEN");
}

/// Base config snippet with required sections (webhook + workspace)
const BASE_CONFIG: &str = r#"
[webhook]
port = 13000

[workspace]
path = "./test-workspace"
"#;

/// Helper to create a temp config file and set the env var
fn setup_config(platform_config: &str) -> tempfile::TempDir {
    let temp_dir = tempfile::tempdir().unwrap();
    let config_path = temp_dir.path().join("config.toml");
    let full_config = format!("{}\n{}", platform_config, BASE_CONFIG);
    let mut file = std::fs::File::create(&config_path).unwrap();
    file.write_all(full_config.as_bytes()).unwrap();
    std::env::set_var("GORP_CONFIG_PATH", config_path.to_str().unwrap());
    temp_dir
}

// =============================================================================
// Config loading with different platform combinations
// =============================================================================

#[test]
#[serial]
fn test_config_matrix_only() {
    clear_config_env_vars();
    let _dir = setup_config(
        r#"
[matrix]
home_server = "https://matrix.org"
user_id = "@bot:matrix.org"
password = "secret"
allowed_users = ["@user:matrix.org"]
"#,
    );

    let config = gorp::config::Config::load().unwrap();
    assert!(config.matrix.is_some());
    assert!(config.telegram.is_none());
    assert!(config.slack.is_none());
    assert!(config.whatsapp.is_none());
    assert!(config.coven.is_none());

    clear_config_env_vars();
}

#[test]
#[serial]
fn test_config_telegram_only() {
    clear_config_env_vars();
    let _dir = setup_config(
        r#"
[telegram]
bot_token = "123456:ABC-DEF1234ghIkl-zyx57W2v1u123ew11"
allowed_users = [12345]
allowed_chats = [-100123456]
"#,
    );

    let config = gorp::config::Config::load().unwrap();
    assert!(config.matrix.is_none());
    assert!(config.telegram.is_some());
    let tg = config.telegram.as_ref().unwrap();
    assert_eq!(tg.bot_token, "123456:ABC-DEF1234ghIkl-zyx57W2v1u123ew11");
    assert_eq!(tg.allowed_users, vec![12345i64]);
    assert_eq!(tg.allowed_chats, vec![-100123456i64]);

    clear_config_env_vars();
}

#[test]
#[serial]
fn test_config_slack_only() {
    clear_config_env_vars();
    let _dir = setup_config(
        r#"
[slack]
bot_token = "xoxb-test-token"
app_token = "xapp-test-token"
signing_secret = "test-signing-secret"
allowed_users = ["U12345"]
"#,
    );

    let config = gorp::config::Config::load().unwrap();
    assert!(config.matrix.is_none());
    assert!(config.slack.is_some());
    let slack = config.slack.as_ref().unwrap();
    assert_eq!(slack.bot_token, "xoxb-test-token");
    assert_eq!(slack.app_token, "xapp-test-token");
    assert_eq!(slack.signing_secret, "test-signing-secret");

    clear_config_env_vars();
}

#[test]
#[serial]
fn test_config_all_platforms() {
    clear_config_env_vars();
    let _dir = setup_config(
        r#"
[matrix]
home_server = "https://matrix.org"
user_id = "@bot:matrix.org"
password = "secret"
allowed_users = ["@user:matrix.org"]

[telegram]
bot_token = "123456:ABC-DEF"
allowed_users = [12345]
allowed_chats = []

[slack]
bot_token = "xoxb-test"
app_token = "xapp-test"
signing_secret = "secret"
allowed_users = []

[whatsapp]
allowed_users = ["1234567890"]

[coven]
gateway_addr = "http://localhost:50051"
"#,
    );

    let config = gorp::config::Config::load().unwrap();
    assert!(config.matrix.is_some());
    assert!(config.telegram.is_some());
    assert!(config.slack.is_some());
    assert!(config.whatsapp.is_some());
    assert!(config.coven.is_some());

    clear_config_env_vars();
}

#[test]
#[serial]
fn test_config_no_platforms() {
    clear_config_env_vars();
    let _dir = setup_config("");

    let config = gorp::config::Config::load().unwrap();
    assert!(config.matrix.is_none());
    assert!(config.telegram.is_none());
    assert!(config.slack.is_none());
    assert!(config.whatsapp.is_none());
    assert!(config.coven.is_none());

    clear_config_env_vars();
}

#[test]
#[serial]
fn test_config_with_coven() {
    clear_config_env_vars();
    let _dir = setup_config(
        r#"
[matrix]
home_server = "https://matrix.org"
user_id = "@bot:matrix.org"
password = "secret"
allowed_users = ["@user:matrix.org"]

[coven]
gateway_addr = "http://localhost:50051"
"#,
    );

    let config = gorp::config::Config::load().unwrap();
    assert!(config.matrix.is_some());
    assert!(config.coven.is_some());
    let coven = config.coven.as_ref().unwrap();
    assert_eq!(coven.gateway_addr, "http://localhost:50051");
    assert!(coven.register_dispatch); // default true

    clear_config_env_vars();
}

#[test]
#[serial]
fn test_config_webhook_defaults() {
    clear_config_env_vars();
    let temp_dir = tempfile::tempdir().unwrap();
    let config_path = temp_dir.path().join("config.toml");
    let content = r#"
[webhook]
port = 9999

[workspace]
path = "./ws"
"#;
    let mut file = std::fs::File::create(&config_path).unwrap();
    file.write_all(content.as_bytes()).unwrap();
    std::env::set_var("GORP_CONFIG_PATH", config_path.to_str().unwrap());

    let config = gorp::config::Config::load().unwrap();
    assert_eq!(config.webhook.port, 9999);

    clear_config_env_vars();
}

// =============================================================================
// PlatformRegistry integration tests
// =============================================================================

use gorp::platform::PlatformRegistry;

#[test]
fn test_registry_empty_has_no_platforms() {
    let registry = PlatformRegistry::new();
    assert!(registry.is_empty());
    assert_eq!(registry.len(), 0);
    assert!(registry.platform_ids().is_empty());
}

#[tokio::test]
async fn test_registry_shutdown_on_empty() {
    let registry = PlatformRegistry::new();
    // Should complete without error on empty registry
    registry.shutdown().await;
}

#[test]
fn test_registry_health_empty() {
    let registry = PlatformRegistry::new();
    let health = registry.health();
    assert!(health.is_empty());
}

// =============================================================================
// Feature-gate compile checks
// These tests verify that types are available based on features.
// =============================================================================

#[test]
fn test_matrix_types_always_available() {
    // Matrix types should always be available (matrix is always compiled)
    let _name = std::any::type_name::<gorp::platform::MatrixPlatform>();
    assert!(!_name.is_empty());
}

#[cfg(feature = "telegram")]
#[test]
fn test_telegram_types_with_feature() {
    let _name = std::any::type_name::<gorp::platform::TelegramPlatform>();
    assert!(!_name.is_empty());
}

#[cfg(feature = "slack")]
#[test]
fn test_slack_types_with_feature() {
    let _name = std::any::type_name::<gorp::platform::SlackPlatform>();
    assert!(!_name.is_empty());
}

#[cfg(feature = "coven")]
#[test]
fn test_coven_types_with_feature() {
    let _name = std::any::type_name::<gorp::coven::CovenProvider>();
    assert!(!_name.is_empty());
}
