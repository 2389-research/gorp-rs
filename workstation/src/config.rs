// ABOUTME: Configuration loading for workstation webapp.
// ABOUTME: Reads environment variables and config files.

use anyhow::Result;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub port: u16,
    pub gorp_api_url: String,
    pub workspace_path: String,
    pub matrix_homeserver: String,
}

impl Config {
    pub fn load() -> Result<Self> {
        Ok(Self {
            port: std::env::var("WORKSTATION_PORT")
                .unwrap_or_else(|_| "8088".to_string())
                .parse()?,
            gorp_api_url: std::env::var("GORP_API_URL")
                .unwrap_or_else(|_| "http://localhost:13000".to_string()),
            workspace_path: std::env::var("WORKSPACE_PATH")
                .unwrap_or_else(|_| "./workspace".to_string()),
            matrix_homeserver: std::env::var("MATRIX_HOMESERVER")
                .unwrap_or_else(|_| "https://matrix.org".to_string()),
        })
    }
}
