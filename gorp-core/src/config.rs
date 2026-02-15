// ABOUTME: Configuration parsing from TOML file with environment variable overrides
// ABOUTME: Validates required fields and provides sensible defaults for optional ones
use crate::paths;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::collections::HashSet;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matrix: Option<MatrixConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub telegram: Option<TelegramConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slack: Option<SlackConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub whatsapp: Option<WhatsAppConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coven: Option<CovenConfig>,
    #[serde(default)]
    pub backend: BackendConfig,
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
pub struct BackendConfig {
    /// Backend type: "acp", "direct", "mock", "mux"
    #[serde(rename = "type", default = "default_backend_type")]
    pub backend_type: String,
    /// Path to the agent binary (for acp/direct backends)
    pub binary: Option<String>,
    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: u64,
    #[serde(default = "default_keep_alive_secs")]
    pub keep_alive_secs: u64,
    #[serde(default = "default_pre_warm_secs")]
    pub pre_warm_secs: u64,
    /// Model to use (for mux backend, e.g., "claude-sonnet-4-20250514")
    pub model: Option<String>,
    /// Max tokens for response (for mux backend)
    pub max_tokens: Option<u32>,
    /// Path to global system prompt (for mux backend, e.g., "~/.mux/system.md")
    pub global_system_prompt_path: Option<String>,
    /// MCP servers to connect to (for mux backend)
    #[serde(default)]
    pub mcp_servers: Vec<McpServerConfig>,
}

/// Configuration for an MCP server (used by mux backend)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    /// Server name (used as tool prefix)
    pub name: String,
    /// Command to run
    pub command: String,
    /// Command arguments
    #[serde(default)]
    pub args: Vec<String>,
    /// Environment variables
    #[serde(default)]
    pub env: std::collections::HashMap<String, String>,
}

fn default_backend_type() -> String {
    "acp".to_string()
}

impl Default for BackendConfig {
    fn default() -> Self {
        Self {
            backend_type: default_backend_type(),
            binary: None,
            timeout_secs: default_timeout_secs(),
            keep_alive_secs: default_keep_alive_secs(),
            pre_warm_secs: default_pre_warm_secs(),
            model: None,
            max_tokens: None,
            global_system_prompt_path: None,
            mcp_servers: Vec::new(),
        }
    }
}

fn default_timeout_secs() -> u64 {
    300 // 5 minutes default timeout
}

fn default_keep_alive_secs() -> u64 {
    3600 // 1 hour
}

fn default_pre_warm_secs() -> u64 {
    300 // 5 minutes
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

fn default_true() -> bool {
    true
}

fn default_wa_data_dir() -> String {
    "./data".to_string()
}

fn default_wa_daily_limit() -> u32 {
    100
}

fn default_wa_rate_limit() -> u64 {
    5
}

fn default_coven_prefix() -> String {
    "gorp".to_string()
}

// ─── TelegramConfig ─────────────────────────────────────────────

#[derive(Clone, Serialize, Deserialize)]
pub struct TelegramConfig {
    pub bot_token: String,
    pub allowed_users: Vec<i64>,
    pub allowed_chats: Vec<i64>,
}

// Custom Debug impl to redact bot_token
impl std::fmt::Debug for TelegramConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TelegramConfig")
            .field("bot_token", &"[REDACTED]")
            .field("allowed_users", &self.allowed_users)
            .field("allowed_chats", &self.allowed_chats)
            .finish()
    }
}

// ─── SlackConfig ────────────────────────────────────────────────

#[derive(Clone, Serialize, Deserialize)]
pub struct SlackConfig {
    pub app_token: String,
    pub bot_token: String,
    pub signing_secret: String,
    pub allowed_users: Vec<String>,
    #[serde(default)]
    pub allowed_channels: Vec<String>,
    #[serde(default = "default_true")]
    pub thread_in_channels: bool,
}

// Custom Debug impl to redact app_token, bot_token, signing_secret
impl std::fmt::Debug for SlackConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SlackConfig")
            .field("app_token", &"[REDACTED]")
            .field("bot_token", &"[REDACTED]")
            .field("signing_secret", &"[REDACTED]")
            .field("allowed_users", &self.allowed_users)
            .field("allowed_channels", &self.allowed_channels)
            .field("thread_in_channels", &self.thread_in_channels)
            .finish()
    }
}

// ─── WhatsAppConfig ─────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WhatsAppSafetyConfig {
    #[serde(default = "default_wa_daily_limit")]
    pub daily_message_limit: u32,
    #[serde(default = "default_wa_rate_limit")]
    pub min_seconds_between: u64,
    #[serde(default)]
    pub quiet_hours_start: Option<u8>,
    #[serde(default)]
    pub quiet_hours_end: Option<u8>,
}

impl Default for WhatsAppSafetyConfig {
    fn default() -> Self {
        Self {
            daily_message_limit: default_wa_daily_limit(),
            min_seconds_between: default_wa_rate_limit(),
            quiet_hours_start: None,
            quiet_hours_end: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WhatsAppConfig {
    #[serde(default = "default_wa_data_dir")]
    pub data_dir: String,
    pub allowed_users: Vec<String>,
    pub node_binary: Option<String>,
    #[serde(default)]
    pub safety: WhatsAppSafetyConfig,
    #[serde(default)]
    pub group_workspaces: HashMap<String, String>,
}

// ─── CovenConfig ────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CovenConfig {
    pub gateway_addr: String,
    #[serde(default = "default_true")]
    pub register_dispatch: bool,
    #[serde(default = "default_coven_prefix")]
    pub agent_name_prefix: String,
    pub ssh_key_path: Option<String>,
}

/// Expand tilde (~) to home directory in paths
/// Logs a warning if expansion fails and falls back to the original path
fn expand_tilde(path: &str) -> String {
    if let Some(stripped) = path.strip_prefix("~/") {
        if let Some(base_dirs) = directories::BaseDirs::new() {
            return base_dirs
                .home_dir()
                .join(stripped)
                .to_string_lossy()
                .to_string();
        } else {
            tracing::warn!(
                path = %path,
                "Failed to expand tilde in path: could not determine home directory"
            );
        }
    } else if path == "~" {
        if let Some(base_dirs) = directories::BaseDirs::new() {
            return base_dirs.home_dir().to_string_lossy().to_string();
        } else {
            tracing::warn!("Failed to expand tilde: could not determine home directory");
        }
    }
    path.to_string()
}

impl Config {
    /// Find the config file, checking multiple locations in order:
    /// 1. GORP_CONFIG_PATH env var (if set)
    /// 2. ./config.toml (current directory - for development)
    /// 3. ~/.config/gorp/config.toml (XDG config dir)
    fn find_config_file() -> Option<PathBuf> {
        // Check GORP_CONFIG_PATH env var first (useful for testing and deployment)
        if let Ok(env_path) = std::env::var("GORP_CONFIG_PATH") {
            let path = PathBuf::from(&env_path);
            if path.exists() {
                return Some(path);
            }
        }

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
    /// Searches: GORP_CONFIG_PATH env var, ./config.toml, then ~/.config/gorp/config.toml
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
            // If no config file, create default config with no platforms enabled
            Config {
                matrix: None,
                telegram: None,
                slack: None,
                whatsapp: None,
                coven: None,
                backend: BackendConfig::default(),
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

        // Override matrix config with environment variables if present
        if let Some(ref mut matrix) = config.matrix {
            if let Ok(val) = std::env::var("MATRIX_HOME_SERVER") {
                matrix.home_server = val;
            }
            if let Ok(val) = std::env::var("MATRIX_USER_ID") {
                matrix.user_id = val;
            }
            if let Ok(val) = std::env::var("MATRIX_PASSWORD") {
                matrix.password = Some(val);
            }
            if let Ok(val) = std::env::var("MATRIX_ACCESS_TOKEN") {
                matrix.access_token = Some(val);
            }
            if let Ok(val) = std::env::var("MATRIX_DEVICE_NAME") {
                matrix.device_name = val;
            }
            if let Ok(val) = std::env::var("ALLOWED_USERS") {
                matrix.allowed_users = val
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
            }
            if let Ok(val) = std::env::var("MATRIX_ROOM_PREFIX") {
                matrix.room_prefix = val;
            }
            if let Ok(val) = std::env::var("MATRIX_RECOVERY_KEY") {
                matrix.recovery_key = Some(val);
                // Clear from environment to prevent exposure via /proc or ps
                std::env::remove_var("MATRIX_RECOVERY_KEY");
            }
        }
        if let Ok(val) = std::env::var("BACKEND_TYPE") {
            config.backend.backend_type = val;
        }
        if let Ok(val) = std::env::var("BACKEND_BINARY") {
            config.backend.binary = Some(val);
        }
        // Legacy env var support
        if let Ok(val) = std::env::var("ACP_AGENT_BINARY") {
            config.backend.binary = Some(val);
        }
        if let Ok(val) = std::env::var("ACP_TIMEOUT_SECS") {
            config.backend.timeout_secs = val.parse().with_context(|| {
                format!("ACP_TIMEOUT_SECS must be a valid number, got: {}", val)
            })?;
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

        // Validate required matrix fields when matrix config is present
        if let Some(ref mut matrix) = config.matrix {
            if matrix.home_server.trim().is_empty() {
                anyhow::bail!(
                    "matrix.home_server is required (set in config.toml or MATRIX_HOME_SERVER env var)"
                );
            }
            if matrix.user_id.trim().is_empty() {
                anyhow::bail!(
                    "matrix.user_id is required (set in config.toml or MATRIX_USER_ID env var)"
                );
            }
            if matrix.password.is_none() && matrix.access_token.is_none() {
                anyhow::bail!("Either matrix.password or matrix.access_token is required");
            }

            // Clean and validate allowed_users
            matrix.allowed_users.retain(|s| !s.trim().is_empty());
            if matrix.allowed_users.is_empty() {
                anyhow::bail!("matrix.allowed_users must contain at least one user ID");
            }
            for user in &matrix.allowed_users {
                if !user.starts_with('@') || !user.contains(':') {
                    anyhow::bail!("Invalid Matrix user ID in allowed_users: {}", user);
                }
            }
        }

        Ok(config)
    }

    /// Convert matrix allowed_users Vec to HashSet for efficient lookups.
    /// Returns an empty set if matrix config is not present.
    pub fn allowed_users_set(&self) -> HashSet<String> {
        self.matrix
            .as_ref()
            .map(|m| m.allowed_users.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Check if a sender is allowed for a given platform.
    /// Each platform has its own allowed_users list in its config section.
    /// Returns true if the user is in the platform's allowlist (or platform has no config).
    pub fn is_user_allowed(&self, platform_id: &str, sender: &str) -> bool {
        match platform_id {
            "matrix" => self
                .matrix
                .as_ref()
                .map(|m| m.allowed_users.iter().any(|u| u == sender))
                .unwrap_or(false),
            "slack" => self
                .slack
                .as_ref()
                .map(|s| s.allowed_users.iter().any(|u| u == sender))
                .unwrap_or(false),
            "whatsapp" => self
                .whatsapp
                .as_ref()
                .map(|w| w.allowed_users.iter().any(|u| u == sender))
                .unwrap_or(false),
            "telegram" => {
                // Telegram uses numeric user IDs
                let sender_id: i64 = match sender.parse() {
                    Ok(id) => id,
                    Err(_) => return false,
                };
                self.telegram
                    .as_ref()
                    .map(|t| t.allowed_users.contains(&sender_id))
                    .unwrap_or(false)
            }
            _ => false,
        }
    }

    /// Get a reference to the Matrix config, returning an error if not configured.
    /// Convenience method for call sites that require Matrix to be present.
    pub fn matrix_config(&self) -> Result<&MatrixConfig> {
        self.matrix
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Matrix configuration is required but not present"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ─── TelegramConfig tests ───────────────────────────────────────

    #[test]
    fn test_telegram_config_deserialize() {
        let toml_str = r#"
            bot_token = "123456:ABC-DEF"
            allowed_users = [111, 222]
            allowed_chats = [-333, -444]
        "#;
        let config: TelegramConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.bot_token, "123456:ABC-DEF");
        assert_eq!(config.allowed_users, vec![111, 222]);
        assert_eq!(config.allowed_chats, vec![-333, -444]);
    }

    #[test]
    fn test_telegram_config_debug_redacts_token() {
        let config = TelegramConfig {
            bot_token: "secret-token".to_string(),
            allowed_users: vec![111],
            allowed_chats: vec![-222],
        };
        let debug_str = format!("{:?}", config);
        assert!(!debug_str.contains("secret-token"), "bot_token should be redacted in Debug output");
        assert!(debug_str.contains("[REDACTED]"));
    }

    #[test]
    fn test_telegram_config_serialize_roundtrip() {
        let config = TelegramConfig {
            bot_token: "tok".to_string(),
            allowed_users: vec![1],
            allowed_chats: vec![-2],
        };
        let serialized = toml::to_string(&config).unwrap();
        let deserialized: TelegramConfig = toml::from_str(&serialized).unwrap();
        assert_eq!(deserialized.bot_token, "tok");
        assert_eq!(deserialized.allowed_users, vec![1]);
        assert_eq!(deserialized.allowed_chats, vec![-2]);
    }

    // ─── SlackConfig tests ──────────────────────────────────────────

    #[test]
    fn test_slack_config_deserialize_full() {
        let toml_str = r#"
            app_token = "xapp-1-A111-222-abc"
            bot_token = "xoxb-111-222-abc"
            signing_secret = "deadbeef"
            allowed_users = ["U111", "U222"]
            allowed_channels = ["C111"]
            thread_in_channels = false
        "#;
        let config: SlackConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.app_token, "xapp-1-A111-222-abc");
        assert_eq!(config.bot_token, "xoxb-111-222-abc");
        assert_eq!(config.signing_secret, "deadbeef");
        assert_eq!(config.allowed_users, vec!["U111", "U222"]);
        assert_eq!(config.allowed_channels, vec!["C111"]);
        assert!(!config.thread_in_channels);
    }

    #[test]
    fn test_slack_config_defaults() {
        let toml_str = r#"
            app_token = "xapp"
            bot_token = "xoxb"
            signing_secret = "secret"
            allowed_users = ["U111"]
        "#;
        let config: SlackConfig = toml::from_str(toml_str).unwrap();
        assert!(config.allowed_channels.is_empty());
        assert!(config.thread_in_channels);
    }

    #[test]
    fn test_slack_config_debug_redacts_secrets() {
        let config = SlackConfig {
            app_token: "xapp-secret".to_string(),
            bot_token: "xoxb-secret".to_string(),
            signing_secret: "signing-secret".to_string(),
            allowed_users: vec!["U111".to_string()],
            allowed_channels: vec![],
            thread_in_channels: true,
        };
        let debug_str = format!("{:?}", config);
        assert!(!debug_str.contains("xapp-secret"), "app_token should be redacted");
        assert!(!debug_str.contains("xoxb-secret"), "bot_token should be redacted");
        assert!(!debug_str.contains("signing-secret"), "signing_secret should be redacted");
        assert!(debug_str.contains("[REDACTED]"));
    }

    // ─── WhatsAppSafetyConfig tests ─────────────────────────────────

    #[test]
    fn test_whatsapp_safety_config_defaults() {
        let config = WhatsAppSafetyConfig::default();
        assert_eq!(config.daily_message_limit, 100);
        assert_eq!(config.min_seconds_between, 5);
        assert!(config.quiet_hours_start.is_none());
        assert!(config.quiet_hours_end.is_none());
    }

    #[test]
    fn test_whatsapp_safety_config_deserialize() {
        let toml_str = r#"
            daily_message_limit = 50
            min_seconds_between = 10
            quiet_hours_start = 22
            quiet_hours_end = 8
        "#;
        let config: WhatsAppSafetyConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.daily_message_limit, 50);
        assert_eq!(config.min_seconds_between, 10);
        assert_eq!(config.quiet_hours_start, Some(22));
        assert_eq!(config.quiet_hours_end, Some(8));
    }

    // ─── WhatsAppConfig tests ───────────────────────────────────────

    #[test]
    fn test_whatsapp_config_deserialize_minimal() {
        let toml_str = r#"
            allowed_users = ["+1234567890"]
        "#;
        let config: WhatsAppConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.data_dir, "./data");
        assert_eq!(config.allowed_users, vec!["+1234567890"]);
        assert!(config.node_binary.is_none());
        assert_eq!(config.safety.daily_message_limit, 100);
        assert!(config.group_workspaces.is_empty());
    }

    #[test]
    fn test_whatsapp_config_deserialize_full() {
        let toml_str = r#"
            data_dir = "/opt/whatsapp"
            allowed_users = ["+1111111111", "+2222222222"]
            node_binary = "/usr/bin/node"

            [safety]
            daily_message_limit = 200
            min_seconds_between = 3
            quiet_hours_start = 23
            quiet_hours_end = 7

            [group_workspaces]
            "12345@g.us" = "project-alpha"
            "67890@g.us" = "project-beta"
        "#;
        let config: WhatsAppConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.data_dir, "/opt/whatsapp");
        assert_eq!(config.allowed_users.len(), 2);
        assert_eq!(config.node_binary, Some("/usr/bin/node".to_string()));
        assert_eq!(config.safety.daily_message_limit, 200);
        assert_eq!(config.safety.min_seconds_between, 3);
        assert_eq!(config.safety.quiet_hours_start, Some(23));
        assert_eq!(config.safety.quiet_hours_end, Some(7));
        assert_eq!(config.group_workspaces.get("12345@g.us"), Some(&"project-alpha".to_string()));
        assert_eq!(config.group_workspaces.get("67890@g.us"), Some(&"project-beta".to_string()));
    }

    // ─── CovenConfig tests ──────────────────────────────────────────

    #[test]
    fn test_coven_config_deserialize_minimal() {
        let toml_str = r#"
            gateway_addr = "http://localhost:9090"
        "#;
        let config: CovenConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.gateway_addr, "http://localhost:9090");
        assert!(config.register_dispatch);
        assert_eq!(config.agent_name_prefix, "gorp");
        assert!(config.ssh_key_path.is_none());
    }

    #[test]
    fn test_coven_config_deserialize_full() {
        let toml_str = r#"
            gateway_addr = "http://coven.local:7777"
            register_dispatch = false
            agent_name_prefix = "custom-prefix"
            ssh_key_path = "/home/user/.ssh/id_ed25519"
        "#;
        let config: CovenConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.gateway_addr, "http://coven.local:7777");
        assert!(!config.register_dispatch);
        assert_eq!(config.agent_name_prefix, "custom-prefix");
        assert_eq!(config.ssh_key_path, Some("/home/user/.ssh/id_ed25519".to_string()));
    }

    // ─── Config struct with optional matrix ─────────────────────────

    #[test]
    fn test_config_without_matrix() {
        let toml_str = r#"
            [webhook]
            port = 13000
            host = "localhost"

            [workspace]
            path = "./workspace"
        "#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert!(config.matrix.is_none());
        assert!(config.telegram.is_none());
        assert!(config.slack.is_none());
        assert!(config.whatsapp.is_none());
        assert!(config.coven.is_none());
    }

    #[test]
    fn test_config_with_matrix() {
        let toml_str = r#"
            [matrix]
            home_server = "https://matrix.org"
            user_id = "@bot:matrix.org"
            access_token = "syt_token"
            allowed_users = ["@user:matrix.org"]

            [webhook]
            port = 13000
            host = "localhost"

            [workspace]
            path = "./workspace"
        "#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert!(config.matrix.is_some());
        let matrix = config.matrix.unwrap();
        assert_eq!(matrix.home_server, "https://matrix.org");
        assert_eq!(matrix.user_id, "@bot:matrix.org");
    }

    #[test]
    fn test_config_with_telegram() {
        let toml_str = r#"
            [telegram]
            bot_token = "123:ABC"
            allowed_users = [111]
            allowed_chats = [-222]

            [webhook]
            port = 13000
            host = "localhost"

            [workspace]
            path = "./workspace"
        "#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert!(config.matrix.is_none());
        assert!(config.telegram.is_some());
        let telegram = config.telegram.unwrap();
        assert_eq!(telegram.bot_token, "123:ABC");
    }

    #[test]
    fn test_config_with_all_platforms() {
        let toml_str = r#"
            [matrix]
            home_server = "https://matrix.org"
            user_id = "@bot:matrix.org"
            access_token = "tok"
            allowed_users = ["@user:matrix.org"]

            [telegram]
            bot_token = "123:ABC"
            allowed_users = [111]
            allowed_chats = []

            [slack]
            app_token = "xapp"
            bot_token = "xoxb"
            signing_secret = "secret"
            allowed_users = ["U111"]

            [whatsapp]
            allowed_users = ["+1234567890"]

            [coven]
            gateway_addr = "http://localhost:9090"

            [webhook]
            port = 13000
            host = "localhost"

            [workspace]
            path = "./workspace"
        "#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert!(config.matrix.is_some());
        assert!(config.telegram.is_some());
        assert!(config.slack.is_some());
        assert!(config.whatsapp.is_some());
        assert!(config.coven.is_some());
    }

    // ─── Config::load() with optional matrix ────────────────────────
    // These tests are consolidated into a single test function to avoid
    // env var race conditions (tests run in parallel and share the process env).

    #[test]
    fn test_load_optional_matrix_scenarios() {
        // Save and clear all matrix-related env vars to avoid interference
        let saved_vars: Vec<(&str, Option<String>)> = vec![
            "GORP_CONFIG_PATH",
            "MATRIX_HOME_SERVER",
            "MATRIX_USER_ID",
            "MATRIX_PASSWORD",
            "MATRIX_ACCESS_TOKEN",
            "MATRIX_DEVICE_NAME",
            "MATRIX_ROOM_PREFIX",
            "MATRIX_RECOVERY_KEY",
            "ALLOWED_USERS",
        ]
        .into_iter()
        .map(|k| (k, std::env::var(k).ok()))
        .collect();

        let cleanup = |saved: &[(&str, Option<String>)]| {
            for (key, val) in saved {
                match val {
                    Some(v) => std::env::set_var(key, v),
                    None => std::env::remove_var(key),
                }
            }
        };

        // Clear all to start fresh
        for (key, _) in &saved_vars {
            std::env::remove_var(key);
        }

        let tmpdir = tempfile::tempdir().unwrap();

        // --- Scenario 1: No config file => matrix is None ---
        std::env::set_var(
            "GORP_CONFIG_PATH",
            tmpdir.path().join("nonexistent.toml"),
        );
        let config = Config::load().unwrap();
        assert!(
            config.matrix.is_none(),
            "With no config file, matrix should be None"
        );

        // --- Scenario 2: Config file without matrix section => matrix is None ---
        let config_no_matrix = tmpdir.path().join("no_matrix.toml");
        std::fs::write(
            &config_no_matrix,
            r#"
                [webhook]
                port = 13000
                host = "localhost"

                [workspace]
                path = "./workspace"
            "#,
        )
        .unwrap();
        std::env::set_var("GORP_CONFIG_PATH", &config_no_matrix);
        let config = Config::load().unwrap();
        assert!(
            config.matrix.is_none(),
            "Without [matrix] section, matrix should be None"
        );

        // --- Scenario 3: Config with matrix + env override ---
        let config_with_matrix = tmpdir.path().join("with_matrix.toml");
        std::fs::write(
            &config_with_matrix,
            r#"
                [matrix]
                home_server = "https://original.org"
                user_id = "@original:original.org"
                access_token = "original_token"
                allowed_users = ["@user:original.org"]

                [webhook]
                port = 13000
                host = "localhost"

                [workspace]
                path = "./workspace"
            "#,
        )
        .unwrap();
        std::env::set_var("GORP_CONFIG_PATH", &config_with_matrix);
        std::env::set_var("MATRIX_HOME_SERVER", "https://override.org");
        let config = Config::load().unwrap();
        let matrix = config.matrix.as_ref().unwrap();
        assert_eq!(
            matrix.home_server, "https://override.org",
            "Env var should override config file value"
        );
        std::env::remove_var("MATRIX_HOME_SERVER");

        // --- Scenario 4: Config with matrix but invalid => error ---
        let config_invalid_matrix = tmpdir.path().join("invalid_matrix.toml");
        std::fs::write(
            &config_invalid_matrix,
            r#"
                [matrix]
                home_server = ""
                user_id = "@bot:matrix.org"
                access_token = "tok"
                allowed_users = ["@user:matrix.org"]

                [webhook]
                port = 13000
                host = "localhost"

                [workspace]
                path = "./workspace"
            "#,
        )
        .unwrap();
        std::env::set_var("GORP_CONFIG_PATH", &config_invalid_matrix);
        let result = Config::load();
        assert!(result.is_err(), "Empty home_server should fail validation");
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("home_server"),
            "Error should mention home_server: {}",
            err_msg
        );

        // Restore env vars
        cleanup(&saved_vars);
    }

    // ─── is_user_allowed tests ──────────────────────────────────────

    fn make_config_with_all_platforms() -> Config {
        toml::from_str(
            r#"
            [matrix]
            home_server = "https://matrix.org"
            user_id = "@bot:matrix.org"
            access_token = "tok"
            allowed_users = ["@alice:matrix.org", "@bob:matrix.org"]

            [telegram]
            bot_token = "123:ABC"
            allowed_users = [111, 222]
            allowed_chats = []

            [slack]
            app_token = "xapp"
            bot_token = "xoxb"
            signing_secret = "secret"
            allowed_users = ["U111", "U222"]

            [whatsapp]
            allowed_users = ["+15551234567", "+15559876543"]

            [webhook]
            port = 13000
            host = "localhost"

            [workspace]
            path = "./workspace"
        "#,
        )
        .unwrap()
    }

    #[test]
    fn test_is_user_allowed_matrix() {
        let config = make_config_with_all_platforms();
        assert!(config.is_user_allowed("matrix", "@alice:matrix.org"));
        assert!(config.is_user_allowed("matrix", "@bob:matrix.org"));
        assert!(!config.is_user_allowed("matrix", "@eve:matrix.org"));
    }

    #[test]
    fn test_is_user_allowed_telegram() {
        let config = make_config_with_all_platforms();
        assert!(config.is_user_allowed("telegram", "111"));
        assert!(config.is_user_allowed("telegram", "222"));
        assert!(!config.is_user_allowed("telegram", "333"));
        assert!(!config.is_user_allowed("telegram", "not-a-number"));
    }

    #[test]
    fn test_is_user_allowed_slack() {
        let config = make_config_with_all_platforms();
        assert!(config.is_user_allowed("slack", "U111"));
        assert!(config.is_user_allowed("slack", "U222"));
        assert!(!config.is_user_allowed("slack", "U999"));
    }

    #[test]
    fn test_is_user_allowed_whatsapp() {
        let config = make_config_with_all_platforms();
        assert!(config.is_user_allowed("whatsapp", "+15551234567"));
        assert!(!config.is_user_allowed("whatsapp", "+15550000000"));
    }

    #[test]
    fn test_is_user_allowed_unknown_platform() {
        let config = make_config_with_all_platforms();
        assert!(!config.is_user_allowed("discord", "user123"));
    }

    #[test]
    fn test_is_user_allowed_no_platform_config() {
        let config: Config = toml::from_str(
            r#"
            [webhook]
            port = 13000
            host = "localhost"

            [workspace]
            path = "./workspace"
        "#,
        )
        .unwrap();
        assert!(!config.is_user_allowed("matrix", "@alice:matrix.org"));
        assert!(!config.is_user_allowed("telegram", "111"));
        assert!(!config.is_user_allowed("slack", "U111"));
    }
}
