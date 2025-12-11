// ABOUTME: Admin panel route handlers
// ABOUTME: Handles config viewing, editing, and session management

use axum::{
    extract::{Path as AxumPath, State},
    routing::{get, post},
    Form, Router,
};
use chrono_tz::Tz;
use serde::Deserialize;
use std::path::Path;
use std::sync::Arc;

use crate::admin::templates::{
    ChannelDetailTemplate, ChannelListTemplate, ChannelRow, ConfigTemplate, DashboardTemplate,
    HealthTemplate, ScheduleRow, SchedulesTemplate, ToastTemplate,
};
use crate::config::Config;
use crate::paths;
use crate::scheduler::{ScheduleStatus, SchedulerStore};
use crate::session::SessionStore;

#[derive(Clone)]
pub struct AdminState {
    pub config: Arc<Config>,
    pub session_store: SessionStore,
    pub scheduler_store: SchedulerStore,
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
        .route("/channels", get(channels_list))
        .route("/channels/create", post(channel_create))
        .route("/channels/{name}", get(channel_detail))
        .route("/channels/{name}/delete", post(channel_delete))
        .route("/channels/{name}/debug", post(channel_toggle_debug))
        .route("/health", get(health_view))
        .route("/schedules", get(schedules_list))
        .route("/schedules/{id}/cancel", post(schedule_cancel))
        .route("/schedules/{id}/pause", post(schedule_pause))
        .route("/schedules/{id}/resume", post(schedule_resume))
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
    // Validate workspace path
    let workspace_path = Path::new(&form.workspace_path);
    if form.workspace_path.contains("..") {
        return ToastTemplate {
            message: "Invalid workspace path: contains path traversal".to_string(),
            is_error: true,
        };
    }
    if !workspace_path.exists() {
        return ToastTemplate {
            message: format!("Workspace path does not exist: {}", form.workspace_path),
            is_error: true,
        };
    }
    if !workspace_path.is_dir() {
        return ToastTemplate {
            message: format!("Workspace path is not a directory: {}", form.workspace_path),
            is_error: true,
        };
    }

    // Validate timezone
    if form.scheduler_timezone.parse::<Tz>().is_err() {
        return ToastTemplate {
            message: format!("Invalid timezone: {}", form.scheduler_timezone),
            is_error: true,
        };
    }

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

// ============================================================================
// Channel Management Handlers
// ============================================================================

#[derive(Deserialize)]
pub struct CreateChannelForm {
    pub name: String,
}

async fn channels_list(State(state): State<AdminState>) -> ChannelListTemplate {
    let channels = state.session_store.list_all().unwrap_or_default();

    let channel_rows: Vec<ChannelRow> = channels
        .iter()
        .map(|ch| {
            let debug_enabled = is_debug_enabled(ch);
            ChannelRow {
                name: ch.channel_name.clone(),
                room_id: ch.room_id.clone(),
                started: ch.started,
                debug_enabled,
                directory: ch.directory.clone(),
                created_at: ch.created_at.clone(),
            }
        })
        .collect();

    ChannelListTemplate {
        title: "Channels - gorp Admin".to_string(),
        channels: channel_rows,
    }
}

async fn channel_detail(
    State(state): State<AdminState>,
    AxumPath(name): AxumPath<String>,
) -> Result<ChannelDetailTemplate, ToastTemplate> {
    let channel = state
        .session_store
        .get_by_name(&name)
        .map_err(|e| ToastTemplate {
            message: format!("Database error: {}", e),
            is_error: true,
        })?
        .ok_or_else(|| ToastTemplate {
            message: format!("Channel not found: {}", name),
            is_error: true,
        })?;

    // Validate directory path
    channel.validate_directory().map_err(|e| ToastTemplate {
        message: format!("Invalid channel directory: {}", e),
        is_error: true,
    })?;

    let debug_enabled = is_debug_enabled(&channel);
    let webhook_url = format!(
        "http://{}:{}/webhook/session/{}",
        state.config.webhook.host, state.config.webhook.port, channel.session_id
    );

    Ok(ChannelDetailTemplate {
        title: format!("Channel: {} - gorp Admin", channel.channel_name),
        name: channel.channel_name,
        room_id: channel.room_id,
        session_id: channel.session_id,
        directory: channel.directory,
        started: channel.started,
        debug_enabled,
        webhook_url,
        created_at: channel.created_at,
    })
}

async fn channel_create(
    State(_state): State<AdminState>,
    Form(form): Form<CreateChannelForm>,
) -> ToastTemplate {
    // Validate channel name
    let name = form.name.trim().to_lowercase();
    if name.is_empty() {
        return ToastTemplate {
            message: "Channel name cannot be empty".to_string(),
            is_error: true,
        };
    }

    if name.len() > 64 {
        return ToastTemplate {
            message: "Channel name too long (max 64 characters)".to_string(),
            is_error: true,
        };
    }

    if !name
        .chars()
        .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
    {
        return ToastTemplate {
            message: "Channel name must be alphanumeric with dashes/underscores only".to_string(),
            is_error: true,
        };
    }

    // Channel creation requires Matrix client which isn't available in admin state
    // Direct users to proper creation methods
    ToastTemplate {
        message: format!(
            "To create channel '{}': DM the bot with !create {} or use the MCP create_channel tool from a Claude session.",
            name, name
        ),
        is_error: false, // Info message, not an error
    }
}

async fn channel_delete(
    State(state): State<AdminState>,
    AxumPath(name): AxumPath<String>,
) -> ToastTemplate {
    // Get channel first to verify it exists
    let channel = match state.session_store.get_by_name(&name) {
        Ok(Some(ch)) => ch,
        Ok(None) => {
            return ToastTemplate {
                message: format!("Channel not found: {}", name),
                is_error: true,
            }
        }
        Err(e) => {
            return ToastTemplate {
                message: format!("Database error: {}", e),
                is_error: true,
            }
        }
    };

    // Delete from database
    if let Err(e) = state.session_store.delete_channel(&channel.channel_name) {
        return ToastTemplate {
            message: format!("Failed to delete channel: {}", e),
            is_error: true,
        };
    }

    ToastTemplate {
        message: format!(
            "Channel '{}' deleted. Workspace preserved at: {}",
            name, channel.directory
        ),
        is_error: false,
    }
}

async fn channel_toggle_debug(
    State(state): State<AdminState>,
    AxumPath(name): AxumPath<String>,
) -> ToastTemplate {
    let channel = match state.session_store.get_by_name(&name) {
        Ok(Some(ch)) => ch,
        Ok(None) => {
            return ToastTemplate {
                message: format!("Channel not found: {}", name),
                is_error: true,
            }
        }
        Err(e) => {
            return ToastTemplate {
                message: format!("Database error: {}", e),
                is_error: true,
            }
        }
    };

    // Validate directory path to prevent path traversal attacks
    if let Err(e) = channel.validate_directory() {
        return ToastTemplate {
            message: format!("Invalid channel directory: {}", e),
            is_error: true,
        };
    }

    let debug_dir = Path::new(&channel.directory).join(".matrix");
    let debug_file = debug_dir.join("enable-debug");
    let currently_enabled = debug_file.exists();

    if currently_enabled {
        // Disable debug
        if let Err(e) = std::fs::remove_file(&debug_file) {
            return ToastTemplate {
                message: format!("Failed to disable debug: {}", e),
                is_error: true,
            };
        }
        ToastTemplate {
            message: format!("Debug mode DISABLED for channel '{}'", name),
            is_error: false,
        }
    } else {
        // Enable debug
        if let Err(e) = std::fs::create_dir_all(&debug_dir) {
            return ToastTemplate {
                message: format!("Failed to create debug directory: {}", e),
                is_error: true,
            };
        }
        if let Err(e) = std::fs::write(&debug_file, "") {
            return ToastTemplate {
                message: format!("Failed to enable debug: {}", e),
                is_error: true,
            };
        }
        ToastTemplate {
            message: format!("Debug mode ENABLED for channel '{}'", name),
            is_error: false,
        }
    }
}

/// Check if debug mode is enabled for a channel
/// Returns false if the directory path is invalid (safe default)
fn is_debug_enabled(channel: &crate::session::Channel) -> bool {
    // Validate directory path to prevent path traversal
    if channel.validate_directory().is_err() {
        return false;
    }
    let debug_path = Path::new(&channel.directory)
        .join(".matrix")
        .join("enable-debug");
    debug_path.exists()
}

// ============================================================================
// Health & Monitoring Handlers
// ============================================================================

async fn health_view(State(state): State<AdminState>) -> HealthTemplate {
    let channels = state.session_store.list_all().unwrap_or_default();
    let active_channels = channels.iter().filter(|c| c.started).count();

    let schedules = state.scheduler_store.list_all().unwrap_or_default();
    let active_schedules = schedules
        .iter()
        .filter(|s| s.status == ScheduleStatus::Active)
        .count();

    HealthTemplate {
        title: "Health - gorp Admin".to_string(),
        homeserver: state.config.matrix.home_server.clone(),
        bot_user_id: state.config.matrix.user_id.clone(),
        device_name: state.config.matrix.device_name.clone(),
        webhook_port: state.config.webhook.port,
        webhook_host: state.config.webhook.host.clone(),
        timezone: state.config.scheduler.timezone.clone(),
        total_channels: channels.len(),
        active_channels,
        total_schedules: schedules.len(),
        active_schedules,
    }
}

async fn schedules_list(State(state): State<AdminState>) -> SchedulesTemplate {
    let schedules = match state.scheduler_store.list_all() {
        Ok(s) => s,
        Err(e) => {
            tracing::error!(error = %e, "Failed to list schedules");
            Vec::new()
        }
    };

    let schedule_rows: Vec<ScheduleRow> = schedules
        .iter()
        .map(|s| {
            let status_icon = match s.status {
                ScheduleStatus::Active => "🟢",
                ScheduleStatus::Paused => "⏸️",
                ScheduleStatus::Completed => "✅",
                ScheduleStatus::Failed => "❌",
                ScheduleStatus::Executing => "⏳",
                ScheduleStatus::Cancelled => "🚫",
            };
            let schedule_type = if s.cron_expression.is_some() {
                "Recurring"
            } else {
                "One-time"
            };
            ScheduleRow {
                id: s.id.clone(),
                channel_name: s.channel_name.clone(),
                prompt_preview: s.prompt.chars().take(50).collect(),
                schedule_type: schedule_type.to_string(),
                cron_expression: s.cron_expression.clone(),
                next_execution: s.next_execution_at.chars().take(19).collect(),
                status: format!("{:?}", s.status),
                status_icon: status_icon.to_string(),
                execution_count: s.execution_count,
                created_at: s.created_at.clone(),
                error_message: s.error_message.clone(),
            }
        })
        .collect();

    SchedulesTemplate {
        title: "Schedules - gorp Admin".to_string(),
        schedules: schedule_rows,
    }
}

async fn schedule_cancel(
    State(state): State<AdminState>,
    AxumPath(id): AxumPath<String>,
) -> ToastTemplate {
    // Validate ID
    if id.is_empty() || id.len() > 256 {
        return ToastTemplate {
            message: "Invalid schedule ID".to_string(),
            is_error: true,
        };
    }

    match state.scheduler_store.cancel_schedule(&id) {
        Ok(true) => ToastTemplate {
            message: "Schedule cancelled".to_string(),
            is_error: false,
        },
        Ok(false) => ToastTemplate {
            message: "Schedule not found".to_string(),
            is_error: true,
        },
        Err(e) => ToastTemplate {
            message: format!("Failed to cancel schedule: {}", e),
            is_error: true,
        },
    }
}

async fn schedule_pause(
    State(state): State<AdminState>,
    AxumPath(id): AxumPath<String>,
) -> ToastTemplate {
    // Validate ID
    if id.is_empty() || id.len() > 256 {
        return ToastTemplate {
            message: "Invalid schedule ID".to_string(),
            is_error: true,
        };
    }

    match state.scheduler_store.pause_schedule(&id) {
        Ok(true) => ToastTemplate {
            message: "Schedule paused".to_string(),
            is_error: false,
        },
        Ok(false) => ToastTemplate {
            // Could be not found OR not in active status
            message: "Could not pause schedule (not found or not active)".to_string(),
            is_error: true,
        },
        Err(e) => ToastTemplate {
            message: format!("Failed to pause schedule: {}", e),
            is_error: true,
        },
    }
}

async fn schedule_resume(
    State(state): State<AdminState>,
    AxumPath(id): AxumPath<String>,
) -> ToastTemplate {
    // Validate ID
    if id.is_empty() || id.len() > 256 {
        return ToastTemplate {
            message: "Invalid schedule ID".to_string(),
            is_error: true,
        };
    }

    match state.scheduler_store.resume_schedule(&id) {
        Ok(true) => ToastTemplate {
            message: "Schedule resumed".to_string(),
            is_error: false,
        },
        Ok(false) => ToastTemplate {
            // Could be not found OR not in paused status
            message: "Could not resume schedule (not found or not paused)".to_string(),
            is_error: true,
        },
        Err(e) => ToastTemplate {
            message: format!("Failed to resume schedule: {}", e),
            is_error: true,
        },
    }
}
