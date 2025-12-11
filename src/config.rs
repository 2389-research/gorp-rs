// ABOUTME: Configuration parsing from TOML file with environment variable overrides
// ABOUTME: Validates required fields and provides sensible defaults for optional ones
use crate::paths;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub matrix: MatrixConfig,
    pub claude: ClaudeConfig,
    pub webhook: WebhookConfig,
    pub workspace: WorkspaceConfig,
    #[serde(default)]
    pub scheduler: SchedulerConfig,
}

#[derive(Clone, Serialize, Deserialize)]
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
    /// Recovery key for cross-signing bootstrap (auto-verifies this device)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recovery_key: Option<String>,
}

// Custom Debug impl to redact sensitive fields
impl std::fmt::Debug for MatrixConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MatrixConfig")
            .field("home_server", &self.home_server)
            .field("user_id", &self.user_id)
            .field("password", &self.password.as_ref().map(|_| "[REDACTED]"))
            .field(
                "access_token",
                &self.access_token.as_ref().map(|_| "[REDACTED]"),
            )
            .field("device_name", &self.device_name)
            .field("allowed_users", &self.allowed_users)
            .field("room_prefix", &self.room_prefix)
            .field(
                "recovery_key",
                &self.recovery_key.as_ref().map(|_| "[REDACTED]"),
            )
            .finish()
    }
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchedulerConfig {
    /// Timezone for interpreting schedule times (e.g., "America/Chicago", "UTC")
    /// Uses IANA timezone names. Defaults to system local timezone.
    #[serde(default = "default_timezone")]
    pub timezone: String,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            timezone: default_timezone(),
        }
    }
}

fn default_timezone() -> String {
    // Try to detect system timezone, fall back to UTC
    // Always validate that the timezone is parseable by chrono-tz
    if let Ok(tz) = std::env::var("TZ") {
        if tz.parse::<chrono_tz::Tz>().is_ok() {
            return tz;
        }
    }
    // On Unix systems, try to read /etc/localtime symlink
    #[cfg(unix)]
    {
        if let Ok(link) = std::fs::read_link("/etc/localtime") {
            if let Some(tz) = link.to_str() {
                // Extract timezone from path like /usr/share/zoneinfo/America/Chicago
                if let Some(pos) = tz.find("zoneinfo/") {
                    let detected = tz[pos + 9..].to_string();
                    if detected.parse::<chrono_tz::Tz>().is_ok() {
                        return detected;
                    }
                }
            }
        }
    }
    "UTC".to_string()
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

/// Expand tilde (~) to home directory in paths
fn expand_tilde(path: &str) -> String {
    if path.starts_with("~/") {
        if let Some(base_dirs) = directories::BaseDirs::new() {
            return base_dirs
                .home_dir()
                .join(&path[2..])
                .to_string_lossy()
                .to_string();
        }
    } else if path == "~" {
        if let Some(base_dirs) = directories::BaseDirs::new() {
            return base_dirs.home_dir().to_string_lossy().to_string();
        }
    }
    path.to_string()
}

impl Config {
    /// Find the config file, checking multiple locations in order:
    /// 1. ./config.toml (current directory - for development)
    /// 2. ~/.config/gorp/config.toml (XDG config dir)
    fn find_config_file() -> Option<PathBuf> {
        let local_config = PathBuf::from("config.toml");
        if local_config.exists() {
            return Some(local_config);
        }

        let xdg_config = paths::config_file();
        if xdg_config.exists() {
            return Some(xdg_config);
        }

        None
    }

    /// Load configuration from config.toml with environment variable overrides
    /// Searches: ./config.toml, then ~/.config/gorp/config.toml
    pub fn load() -> Result<Self> {
        // Try to find and load config file
        let mut config = if let Some(config_path) = Self::find_config_file() {
            tracing::info!(
                path = %config_path.display(),
                "Loading configuration from file"
            );
            let content = std::fs::read_to_string(&config_path)
                .with_context(|| format!("Failed to read {}", config_path.display()))?;
            toml::from_str::<Config>(&content)
                .with_context(|| format!("Failed to parse {}", config_path.display()))?
        } else {
            tracing::info!("No config file found, using environment variables and defaults");
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
                    recovery_key: None,
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
                scheduler: SchedulerConfig::default(),
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
        if let Ok(val) = std::env::var("MATRIX_RECOVERY_KEY") {
            config.matrix.recovery_key = Some(val);
            // Clear from environment to prevent exposure via /proc or ps
            std::env::remove_var("MATRIX_RECOVERY_KEY");
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
        if let Ok(val) = std::env::var("SCHEDULER_TIMEZONE") {
            config.scheduler.timezone = val;
        }

        // Expand tilde in workspace path
        config.workspace.path = expand_tilde(&config.workspace.path);

        // Validate timezone is a valid IANA timezone
        if config.scheduler.timezone.parse::<chrono_tz::Tz>().is_err() {
            anyhow::bail!(
                "Invalid timezone '{}'. Use IANA timezone names like 'America/Chicago', 'Europe/London', 'UTC'",
                config.scheduler.timezone
            );
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
