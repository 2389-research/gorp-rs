use anyhow::{Context, Result};
use std::collections::HashSet;

#[derive(Debug, Clone)]
pub struct Config {
    pub matrix_home_server: String,
    pub matrix_user_id: String,
    pub matrix_room_id: String,
    pub matrix_password: Option<String>,
    pub matrix_access_token: Option<String>,
    pub matrix_device_name: String,
    pub allowed_users: HashSet<String>,
    pub claude_binary_path: String,
    pub claude_sdk_url: Option<String>,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let matrix_home_server = std::env::var("MATRIX_HOME_SERVER")
            .context("MATRIX_HOME_SERVER is required")?;
        let matrix_user_id = std::env::var("MATRIX_USER_ID")
            .context("MATRIX_USER_ID is required")?;
        let matrix_room_id = std::env::var("MATRIX_ROOM_ID")
            .context("MATRIX_ROOM_ID is required")?;
        let matrix_password = std::env::var("MATRIX_PASSWORD").ok();
        let matrix_access_token = std::env::var("MATRIX_ACCESS_TOKEN").ok();
        let matrix_device_name = std::env::var("MATRIX_DEVICE_NAME")
            .unwrap_or_else(|_| "claude-matrix-bridge".to_string());

        let allowed_users_str = std::env::var("ALLOWED_USERS")
            .context("ALLOWED_USERS is required")?;
        let allowed_users: HashSet<String> = allowed_users_str
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        let claude_binary_path = std::env::var("CLAUDE_BINARY_PATH")
            .unwrap_or_else(|_| "claude".to_string());
        let claude_sdk_url = std::env::var("CLAUDE_SDK_URL").ok();

        Ok(Config {
            matrix_home_server,
            matrix_user_id,
            matrix_room_id,
            matrix_password,
            matrix_access_token,
            matrix_device_name,
            allowed_users,
            claude_binary_path,
            claude_sdk_url,
        })
    }
}
