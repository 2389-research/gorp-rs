// ABOUTME: Configuration file support for gorp-agent.
// ABOUTME: Loads backend config from TOML with [backend] section.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Top-level configuration structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub backend: BackendConfig,
}

/// Backend configuration with type discriminator
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendConfig {
    /// Backend type: "direct", "acp", "mock", etc.
    #[serde(rename = "type")]
    pub backend_type: String,

    /// Remaining fields passed to backend factory
    #[serde(flatten)]
    pub config: toml::Table,
}

impl Config {
    /// Load configuration from a TOML file
    pub fn from_file(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read config file: {}", path.display()))?;
        Self::parse(&content)
    }

    /// Parse configuration from a TOML string
    pub fn parse(content: &str) -> Result<Self> {
        toml::from_str(content).context("Failed to parse config TOML")
    }

    /// Find config file in standard locations
    /// Searches: ./gorp-agent.toml, ~/.config/gorp/agent.toml
    pub fn find_and_load() -> Result<Option<Self>> {
        let candidates = [
            std::env::current_dir()
                .ok()
                .map(|p| p.join("gorp-agent.toml")),
            dirs_next().map(|p| p.join("gorp/agent.toml")),
        ];

        for candidate in candidates.into_iter().flatten() {
            if candidate.exists() {
                tracing::debug!(path = %candidate.display(), "Found config file");
                return Ok(Some(Self::from_file(&candidate)?));
            }
        }

        Ok(None)
    }
}

impl BackendConfig {
    /// Get backend type name
    pub fn backend_type(&self) -> &str {
        &self.backend_type
    }

    /// Convert config table to serde_json::Value for registry
    pub fn to_json_value(&self) -> serde_json::Value {
        // Convert TOML table to JSON value
        let json_str = serde_json::to_string(&self.config).unwrap_or_default();
        serde_json::from_str(&json_str).unwrap_or(serde_json::json!({}))
    }
}

/// Get user config directory
fn dirs_next() -> Option<std::path::PathBuf> {
    std::env::var("HOME")
        .ok()
        .map(|h| std::path::PathBuf::from(h).join(".config"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_direct_backend() {
        let toml = r#"
[backend]
type = "direct"
binary = "claude"
working_dir = "."
"#;
        let config = Config::from_str(toml).unwrap();
        assert_eq!(config.backend.backend_type, "direct");

        let json = config.backend.to_json_value();
        assert_eq!(json["binary"], "claude");
        assert_eq!(json["working_dir"], ".");
    }

    #[test]
    fn test_parse_acp_backend() {
        let toml = r#"
[backend]
type = "acp"
binary = "claude-code-acp"
timeout_secs = 300
working_dir = "/tmp"
extra_args = ["-v"]
"#;
        let config = Config::from_str(toml).unwrap();
        assert_eq!(config.backend.backend_type, "acp");

        let json = config.backend.to_json_value();
        assert_eq!(json["binary"], "claude-code-acp");
        assert_eq!(json["timeout_secs"], 300);
        assert_eq!(json["extra_args"], serde_json::json!(["-v"]));
    }
}
