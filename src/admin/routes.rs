// ABOUTME: Admin panel route handlers
// ABOUTME: Handles config viewing, editing, and session management

use axum::{extract::State, routing::{get, post}, Form, Router};
use serde::Deserialize;
use std::sync::Arc;

use crate::admin::templates::{ConfigTemplate, DashboardTemplate, ToastTemplate};
use crate::config::Config;
use crate::paths;

#[derive(Clone)]
pub struct AdminState {
    pub config: Arc<Config>,
}

#[derive(Deserialize)]
pub struct ConfigForm {
    pub home_server: String,
    pub user_id: String,
    pub device_name: String,
    pub room_prefix: String,
    pub allowed_users: String,
    pub webhook_port: u16,
    pub webhook_host: String,
    pub workspace_path: String,
    pub scheduler_timezone: String,
}

/// Build the admin router mounted at /admin
pub fn admin_router() -> Router<AdminState> {
    Router::new()
        .route("/", get(dashboard))
        .route("/config", get(config_view))
        .route("/config/save", post(config_save))
}

async fn dashboard() -> DashboardTemplate {
    DashboardTemplate {
        title: "gorp Admin".to_string(),
    }
}

async fn config_view(State(state): State<AdminState>) -> ConfigTemplate {
    let config = &state.config;
    ConfigTemplate {
        title: "Configuration - gorp Admin".to_string(),
        home_server: config.matrix.home_server.clone(),
        user_id: config.matrix.user_id.clone(),
        device_name: config.matrix.device_name.clone(),
        room_prefix: config.matrix.room_prefix.clone(),
        allowed_users: config.matrix.allowed_users.join(", "),
        webhook_port: config.webhook.port,
        webhook_host: config.webhook.host.clone(),
        webhook_api_key_set: config.webhook.api_key.is_some(),
        workspace_path: config.workspace.path.clone(),
        scheduler_timezone: config.scheduler.timezone.clone(),
        password_set: config.matrix.password.is_some(),
        access_token_set: config.matrix.access_token.is_some(),
        recovery_key_set: config.matrix.recovery_key.is_some(),
    }
}

async fn config_save(
    State(state): State<AdminState>,
    Form(form): Form<ConfigForm>,
) -> ToastTemplate {
    // Parse allowed_users from comma-separated string
    let allowed_users: Vec<String> = form
        .allowed_users
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    // Build new config preserving secrets from current config
    let mut new_config = (*state.config).clone();
    new_config.matrix.home_server = form.home_server;
    new_config.matrix.user_id = form.user_id;
    new_config.matrix.device_name = form.device_name;
    new_config.matrix.room_prefix = form.room_prefix;
    new_config.matrix.allowed_users = allowed_users;
    new_config.webhook.port = form.webhook_port;
    new_config.webhook.host = form.webhook_host;
    new_config.workspace.path = form.workspace_path;
    new_config.scheduler.timezone = form.scheduler_timezone;

    // Serialize to TOML
    let toml_str = match toml::to_string_pretty(&new_config) {
        Ok(s) => s,
        Err(e) => {
            return ToastTemplate {
                message: format!("Failed to serialize config: {}", e),
                is_error: true,
            };
        }
    };

    // Write to config file
    let config_path = paths::config_file();
    if let Err(e) = std::fs::write(&config_path, toml_str) {
        return ToastTemplate {
            message: format!("Failed to save config: {}", e),
            is_error: true,
        };
    }

    ToastTemplate {
        message: "Configuration saved! Restart required for some changes.".to_string(),
        is_error: false,
    }
}
