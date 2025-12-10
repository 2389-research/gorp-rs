// ABOUTME: Configuration parsing from TOML file with environment variable overrides
// ABOUTME: Validates required fields and provides sensible defaults for optional ones
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub matrix: MatrixConfig,
    pub claude: ClaudeConfig,
    pub webhook: WebhookConfig,
    pub workspace: WorkspaceConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatrixConfig {
    pub home_server: String,
    pub user_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub access_token: Option<String>,
    #[serde(default = "default_device_name")]
    pub device_name: String,
    pub allowed_users: Vec<String>,
    #[serde(default = "default_room_prefix")]
    pub room_prefix: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeConfig {
    #[serde(default = "default_claude_binary")]
    pub binary_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sdk_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookConfig {
    #[serde(default = "default_webhook_port")]
    pub port: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    #[serde(default = "default_webhook_host")]
    pub host: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceConfig {
    #[serde(default = "default_workspace_path")]
    pub path: String,
}

fn default_device_name() -> String {
    "claude-matrix-bridge".to_string()
}

fn default_claude_binary() -> String {
    "claude".to_string()
}

fn default_webhook_port() -> u16 {
    13000
}

fn default_webhook_host() -> String {
    "localhost".to_string()
}

fn default_workspace_path() -> String {
    "./workspace".to_string()
}

fn default_room_prefix() -> String {
    "Claude".to_string()
}

impl Config {
    /// Load configuration from config.toml with environment variable overrides
    pub fn load() -> Result<Self> {
        // Try to load from config.toml first
        let config_path = "config.toml";
        let mut config = if Path::new(config_path).exists() {
            let content =
                std::fs::read_to_string(config_path).context("Failed to read config.toml")?;
            toml::from_str::<Config>(&content).context("Failed to parse config.toml")?
        } else {
            // If no config file, create default config
            Config {
                matrix: MatrixConfig {
                    home_server: String::new(),
                    user_id: String::new(),
                    password: None,
                    access_token: None,
                    device_name: default_device_name(),
                    allowed_users: Vec::new(),
                    room_prefix: default_room_prefix(),
                },
                claude: ClaudeConfig {
                    binary_path: default_claude_binary(),
                    sdk_url: None,
                },
                webhook: WebhookConfig {
                    port: default_webhook_port(),
                    api_key: None,
                    host: default_webhook_host(),
                },
                workspace: WorkspaceConfig {
                    path: default_workspace_path(),
                },
            }
        };

        // Override with environment variables if present
        if let Ok(val) = std::env::var("MATRIX_HOME_SERVER") {
            config.matrix.home_server = val;
        }
        if let Ok(val) = std::env::var("MATRIX_USER_ID") {
            config.matrix.user_id = val;
        }
        if let Ok(val) = std::env::var("MATRIX_PASSWORD") {
            config.matrix.password = Some(val);
        }
        if let Ok(val) = std::env::var("MATRIX_ACCESS_TOKEN") {
            config.matrix.access_token = Some(val);
        }
        if let Ok(val) = std::env::var("MATRIX_DEVICE_NAME") {
            config.matrix.device_name = val;
        }
        if let Ok(val) = std::env::var("ALLOWED_USERS") {
            config.matrix.allowed_users = val
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
        }
        if let Ok(val) = std::env::var("MATRIX_ROOM_PREFIX") {
            config.matrix.room_prefix = val;
        }
        if let Ok(val) = std::env::var("CLAUDE_BINARY_PATH") {
            config.claude.binary_path = val;
        }
        if let Ok(val) = std::env::var("CLAUDE_SDK_URL") {
            config.claude.sdk_url = Some(val);
        }
        if let Ok(val) = std::env::var("WEBHOOK_PORT") {
            config.webhook.port = val.parse().with_context(|| {
                format!("WEBHOOK_PORT must be a valid port number, got: {}", val)
            })?;
        }
        if let Ok(val) = std::env::var("WEBHOOK_API_KEY") {
            config.webhook.api_key = Some(val);
        }
        if let Ok(val) = std::env::var("WEBHOOK_HOST") {
            config.webhook.host = val;
        }
        if let Ok(val) = std::env::var("WORKSPACE_PATH") {
            config.workspace.path = val;
        }

        // Validate required fields
        if config.matrix.home_server.trim().is_empty() {
            anyhow::bail!(
                "matrix.home_server is required (set in config.toml or MATRIX_HOME_SERVER env var)"
            );
        }
        if config.matrix.user_id.trim().is_empty() {
            anyhow::bail!(
                "matrix.user_id is required (set in config.toml or MATRIX_USER_ID env var)"
            );
        }
        if config.matrix.password.is_none() && config.matrix.access_token.is_none() {
            anyhow::bail!("Either matrix.password or matrix.access_token is required");
        }

        // Clean and validate allowed_users
        config.matrix.allowed_users.retain(|s| !s.trim().is_empty());
        if config.matrix.allowed_users.is_empty() {
            anyhow::bail!("matrix.allowed_users must contain at least one user ID");
        }
        for user in &config.matrix.allowed_users {
            if !user.starts_with('@') || !user.contains(':') {
                anyhow::bail!("Invalid Matrix user ID in allowed_users: {}", user);
            }
        }

        Ok(config)
    }

    /// Convert allowed_users Vec to HashSet for efficient lookups
    pub fn allowed_users_set(&self) -> HashSet<String> {
        self.matrix.allowed_users.iter().cloned().collect()
    }
}
