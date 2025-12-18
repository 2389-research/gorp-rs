// ABOUTME: Configuration loading for workstation webapp.
// ABOUTME: Reads environment variables and config files.

use anyhow::Result;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub port: u16,
    pub gorp_api_url: String,
    pub workspace_path: String,
    pub oidc_issuer: String,
    pub oidc_redirect_uri: String,
    pub session_db_path: String,
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
            oidc_issuer: std::env::var("OIDC_ISSUER")
                .unwrap_or_else(|_| "https://account.matrix.org/".to_string()),
            oidc_redirect_uri: std::env::var("OIDC_REDIRECT_URI")
                .unwrap_or_else(|_| "http://localhost:8088/auth/callback".to_string()),
            session_db_path: std::env::var("SESSION_DB_PATH")
                .unwrap_or_else(|_| "./workstation_sessions.db".to_string()),
        })
    }
}
