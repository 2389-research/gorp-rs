// ABOUTME: Admin panel route handlers
// ABOUTME: Handles config viewing, editing, and session management

use axum::{
    extract::{Path as AxumPath, State},
    routing::{get, post},
    Form, Router,
};
use chrono_tz::Tz;
use serde::Deserialize;
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::Arc;

use crate::admin::templates::{
    ChannelDetailTemplate, ChannelListTemplate, ChannelRow, ConfigTemplate, DashboardTemplate,
    ErrorEntry, HealthTemplate, LogViewerTemplate, MessageEntry, MessageHistoryTemplate,
    ScheduleFormTemplate, ScheduleRow, SchedulesTemplate, ToastTemplate,
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
        .route("/channels/{name}/logs", get(channel_logs))
        .route("/channels/{name}/delete", post(channel_delete))
        .route("/channels/{name}/debug", post(channel_toggle_debug))
        .route("/messages", get(messages_view))
        .route("/health", get(health_view))
        .route("/schedules", get(schedules_list))
        .route("/schedules/new", get(schedule_form))
        .route("/schedules/create", post(schedule_create))
        .route("/schedules/{id}/cancel", post(schedule_cancel))
        .route("/schedules/{id}/pause", post(schedule_pause))
        .route("/schedules/{id}/resume", post(schedule_resume))
}

async fn dashboard(State(state): State<AdminState>) -> DashboardTemplate {
    // Get channel counts
    let channels = match state.session_store.list_all() {
        Ok(c) => c,
        Err(e) => {
            tracing::error!(error = %e, "Failed to list channels for dashboard");
            Vec::new()
        }
    };
    let total_channels = channels.len();
    let active_channels = channels.iter().filter(|c| c.started).count();

    // Get schedule count
    let schedules = match state.scheduler_store.list_all() {
        Ok(s) => s,
        Err(e) => {
            tracing::error!(error = %e, "Failed to list schedules for dashboard");
            Vec::new()
        }
    };
    let total_schedules = schedules.len();

    // Count recent messages from today across all channels
    // Uses efficient tail reading - only reads last 1000 lines per channel
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let messages_today: usize = channels
        .iter()
        .map(|channel| {
            // Validate directory path for security
            if let Err(e) = channel.validate_directory() {
                tracing::warn!(
                    channel = %channel.channel_name,
                    error = %e,
                    "Skipping channel with invalid directory in dashboard"
                );
                return 0;
            }

            let log_path = Path::new(&channel.directory)
                .join(".matrix")
                .join("matrix_messages.log");

            // Use efficient tail reading with pattern matching
            count_recent_lines_matching(&log_path, &today)
        })
        .sum();

    DashboardTemplate {
        title: "gorp Admin".to_string(),
        total_channels,
        active_channels,
        total_schedules,
        messages_today,
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
    let channels = match state.session_store.list_all() {
        Ok(c) => c,
        Err(e) => {
            tracing::error!(error = %e, "Failed to list channels");
            Vec::new()
        }
    };

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

async fn channel_logs(
    State(state): State<AdminState>,
    AxumPath(name): AxumPath<String>,
) -> Result<LogViewerTemplate, ToastTemplate> {
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

    // Validate directory path for security
    channel.validate_directory().map_err(|e| ToastTemplate {
        message: format!("Invalid channel directory: {}", e),
        is_error: true,
    })?;

    let log_path = Path::new(&channel.directory)
        .join(".matrix")
        .join("matrix_messages.log");

    // Use efficient tail reading - only reads last 100 lines without loading entire file
    let log_lines = read_last_n_lines(&log_path, 100);

    Ok(LogViewerTemplate {
        title: format!("Logs: {} - gorp Admin", channel.channel_name),
        channel_name: channel.channel_name,
        log_lines,
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

/// Maximum file size to read for message counting (10MB)
const MAX_LOG_FILE_SIZE: u64 = 10 * 1024 * 1024;

/// Read the last N lines of a file efficiently by reading backwards from end.
/// Returns empty vec if file doesn't exist, is too large, or on any error.
/// This avoids loading entire files into memory.
fn read_last_n_lines(path: &std::path::Path, n: usize) -> Vec<String> {
    let file = match File::open(path) {
        Ok(f) => f,
        Err(_) => return Vec::new(),
    };

    // Check file size - skip if too large
    let metadata = match file.metadata() {
        Ok(m) => m,
        Err(_) => return Vec::new(),
    };

    if metadata.len() > MAX_LOG_FILE_SIZE {
        tracing::warn!(
            path = %path.display(),
            size = metadata.len(),
            "Log file too large, skipping"
        );
        return Vec::new();
    }

    // For small files, just read normally
    if metadata.len() < 64 * 1024 {
        let reader = BufReader::new(file);
        let lines: Vec<String> = reader.lines().filter_map(Result::ok).collect();
        let start = lines.len().saturating_sub(n);
        return lines[start..].to_vec();
    }

    // For larger files, read backwards from end
    read_last_n_lines_reverse(file, n, metadata.len())
}

/// Chunk size for backward file reading.
/// 64KB balances memory usage with syscall overhead for typical log files.
const REVERSE_READ_CHUNK_SIZE: u64 = 64 * 1024;

/// Read last N lines by seeking backwards from file end.
/// More efficient for large files when we only need recent entries.
/// Uses lossy UTF-8 conversion to handle potential multi-byte boundary issues.
fn read_last_n_lines_reverse(mut file: File, n: usize, file_size: u64) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut pos = file_size;
    let mut leftover_bytes: Vec<u8> = Vec::new();
    let mut is_first_chunk = true;

    while lines.len() < n && pos > 0 {
        let read_size = std::cmp::min(REVERSE_READ_CHUNK_SIZE, pos);
        pos -= read_size;

        if let Err(e) = file.seek(SeekFrom::Start(pos)) {
            tracing::debug!(error = %e, pos = pos, "Failed to seek in log file");
            break;
        }

        let mut buffer = vec![0u8; read_size as usize];
        if let Err(e) = file.read_exact(&mut buffer) {
            tracing::debug!(error = %e, "Failed to read chunk from log file");
            break;
        }

        // Append leftover bytes from previous chunk (they come AFTER this chunk's content)
        buffer.extend(leftover_bytes.drain(..));

        // Use lossy conversion to handle UTF-8 boundary issues gracefully
        // This may replace partial chars at boundaries with replacement char, but won't fail
        let chunk = String::from_utf8_lossy(&buffer).to_string();

        // Split into lines
        let mut chunk_lines: Vec<&str> = chunk.lines().collect();

        // If we're not at the beginning of the file and this isn't our first chunk,
        // the first "line" may be partial (continues from earlier in file)
        if pos > 0 && !chunk_lines.is_empty() && !is_first_chunk {
            // Save the partial line to prepend to next chunk
            leftover_bytes = chunk_lines.remove(0).as_bytes().to_vec();
        } else if pos > 0 && !chunk_lines.is_empty() {
            // First chunk at end of file - first line might still be partial
            // if we didn't land exactly on a newline
            leftover_bytes = chunk_lines.remove(0).as_bytes().to_vec();
        }

        is_first_chunk = false;

        // Add lines in reverse order (they'll be reversed at end)
        for line in chunk_lines.into_iter().rev() {
            let trimmed = line.trim();
            if !trimmed.is_empty() {
                lines.push(trimmed.to_string());
                if lines.len() >= n {
                    break;
                }
            }
        }
    }

    // Include any remaining leftover if we reached start of file
    if pos == 0 && !leftover_bytes.is_empty() && lines.len() < n {
        let leftover = String::from_utf8_lossy(&leftover_bytes).to_string();
        let trimmed = leftover.trim();
        if !trimmed.is_empty() {
            lines.push(trimmed.to_string());
        }
    }

    // Reverse to get chronological order
    lines.reverse();
    lines
}

/// Count lines matching a pattern in the last portion of a file.
/// Only reads the tail of the file to avoid loading entire logs.
fn count_recent_lines_matching(path: &std::path::Path, pattern: &str) -> usize {
    // Read last 1000 lines max for counting today's messages
    // This gives a reasonable approximation without reading entire files
    let lines = read_last_n_lines(path, 1000);
    lines.iter().filter(|line| line.contains(pattern)).count()
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
    let channels = match state.session_store.list_all() {
        Ok(c) => c,
        Err(e) => {
            tracing::error!(error = %e, "Failed to list channels for health view");
            Vec::new()
        }
    };
    let active_channels = channels.iter().filter(|c| c.started).count();

    let schedules = match state.scheduler_store.list_all() {
        Ok(s) => s,
        Err(e) => {
            tracing::error!(error = %e, "Failed to list schedules for health view");
            Vec::new()
        }
    };
    let active_schedules = schedules
        .iter()
        .filter(|s| s.status == ScheduleStatus::Active)
        .count();

    // Get recent failed schedules (last 10)
    let mut recent_errors: Vec<ErrorEntry> = schedules
        .iter()
        .filter(|s| s.status == ScheduleStatus::Failed && s.error_message.is_some())
        .map(|s| {
            let timestamp = s
                .last_executed_at
                .as_ref()
                .unwrap_or(&s.created_at)
                .chars()
                .take(19)
                .collect();
            let source = format!("Schedule: {}", s.channel_name);
            let message = s.error_message.clone().unwrap_or_default();
            ErrorEntry {
                timestamp,
                source,
                message,
            }
        })
        .collect();

    // Sort by timestamp descending (most recent first) and take 10
    recent_errors.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    recent_errors.truncate(10);

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
        recent_errors,
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

// ============================================================================
// Message History Handler
// ============================================================================

#[derive(serde::Deserialize)]
struct MessageLogEntry {
    timestamp: String,
    direction: String,
    sender: String,
    content: String,
    #[allow(dead_code)]
    html: Option<String>,
}

async fn messages_view(State(state): State<AdminState>) -> MessageHistoryTemplate {
    let channels = match state.session_store.list_all() {
        Ok(c) => c,
        Err(e) => {
            tracing::error!(error = %e, "Failed to list channels for messages view");
            Vec::new()
        }
    };
    let mut all_messages: Vec<MessageEntry> = Vec::new();

    // Scale lines per channel based on channel count to avoid unbounded memory usage
    // With many channels, read fewer lines each; with few channels, read more
    // Target: ~200 total messages max before sorting (reasonable for 100 result limit)
    let num_channels = channels.len().max(1);
    let lines_per_channel = (200 / num_channels).clamp(10, 100);

    for channel in channels {
        // Validate directory path to prevent path traversal
        if let Err(e) = channel.validate_directory() {
            tracing::warn!(
                channel = %channel.channel_name,
                error = %e,
                "Skipping channel with invalid directory in messages view"
            );
            continue;
        }

        let log_path = Path::new(&channel.directory)
            .join(".matrix")
            .join("matrix_messages.log");

        // Use efficient tail reading
        let lines = read_last_n_lines(&log_path, lines_per_channel);

        for line in lines {
            if line.trim().is_empty() {
                continue;
            }

            if let Ok(entry) = serde_json::from_str::<MessageLogEntry>(&line) {
                let content_preview: String = entry
                    .content
                    .chars()
                    .take(50)
                    .collect::<String>()
                    + if entry.content.len() > 50 { "..." } else { "" };

                let timestamp = entry.timestamp.chars().take(19).collect();

                all_messages.push(MessageEntry {
                    timestamp,
                    channel_name: channel.channel_name.clone(),
                    direction: entry.direction,
                    sender: entry.sender,
                    content_preview,
                });
            }
        }
    }

    // Sort by timestamp descending (most recent first)
    all_messages.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    all_messages.truncate(100);

    MessageHistoryTemplate {
        title: "Message History - gorp Admin".to_string(),
        messages: all_messages,
    }
}

// ============================================================================
// Schedule Form Handlers
// ============================================================================

async fn schedule_form(State(state): State<AdminState>) -> ScheduleFormTemplate {
    let channels: Vec<String> = match state.session_store.list_all() {
        Ok(c) => c.into_iter().map(|c| c.channel_name).collect(),
        Err(e) => {
            tracing::error!(error = %e, "Failed to list channels for schedule form");
            Vec::new()
        }
    };

    ScheduleFormTemplate {
        title: "New Schedule - gorp Admin".to_string(),
        channels,
    }
}

#[derive(Deserialize)]
struct CreateScheduleForm {
    channel: String,
    prompt: String,
    execute_at: String,
}

/// Maximum prompt length (64KB should be plenty for any reasonable prompt)
const MAX_PROMPT_LENGTH: usize = 64 * 1024;
/// Maximum time expression length (cron + natural language should fit in 256 chars)
const MAX_TIME_EXPRESSION_LENGTH: usize = 256;
/// Maximum channel name length
const MAX_CHANNEL_NAME_LENGTH: usize = 64;

async fn schedule_create(
    State(state): State<AdminState>,
    Form(form): Form<CreateScheduleForm>,
) -> ToastTemplate {
    use crate::scheduler::{ParsedSchedule, ScheduledPrompt, ScheduleStatus};

    // Validate inputs with length limits to prevent DoS/memory exhaustion
    let channel = form.channel.trim();
    let prompt = form.prompt.trim();
    let execute_at = form.execute_at.trim();

    if channel.is_empty() {
        return ToastTemplate {
            message: "Please select a channel".to_string(),
            is_error: true,
        };
    }

    if channel.len() > MAX_CHANNEL_NAME_LENGTH {
        return ToastTemplate {
            message: format!(
                "Channel name too long (max {} characters)",
                MAX_CHANNEL_NAME_LENGTH
            ),
            is_error: true,
        };
    }

    if prompt.is_empty() {
        return ToastTemplate {
            message: "Prompt cannot be empty".to_string(),
            is_error: true,
        };
    }

    if prompt.len() > MAX_PROMPT_LENGTH {
        return ToastTemplate {
            message: format!("Prompt too long (max {} KB)", MAX_PROMPT_LENGTH / 1024),
            is_error: true,
        };
    }

    if execute_at.is_empty() {
        return ToastTemplate {
            message: "Schedule time is required".to_string(),
            is_error: true,
        };
    }

    if execute_at.len() > MAX_TIME_EXPRESSION_LENGTH {
        return ToastTemplate {
            message: format!(
                "Time expression too long (max {} characters)",
                MAX_TIME_EXPRESSION_LENGTH
            ),
            is_error: true,
        };
    }

    // Get channel to verify it exists and get room_id
    let channel_info = match state.session_store.get_by_name(channel) {
        Ok(Some(c)) => c,
        Ok(None) => {
            return ToastTemplate {
                message: format!("Channel not found: {}", channel),
                is_error: true,
            };
        }
        Err(e) => {
            return ToastTemplate {
                message: format!("Database error: {}", e),
                is_error: true,
            };
        }
    };

    let timezone = &state.config.scheduler.timezone;

    // Check if it's a cron expression (for recurring) or a time expression
    let is_cron = execute_at.split_whitespace().count() == 5
        && execute_at.chars().all(|c| c.is_ascii_digit() || " */-,".contains(c));

    // Parse time expression and build schedule
    let (next_execution_at, cron_expression, execute_at_field) = if is_cron {
        // For cron, calculate next execution time
        match crate::scheduler::compute_next_cron_execution_in_tz(execute_at, timezone) {
            Ok(t) => (t.to_rfc3339(), Some(execute_at.to_string()), None),
            Err(e) => {
                return ToastTemplate {
                    message: format!("Invalid cron expression: {}", e),
                    is_error: true,
                };
            }
        }
    } else {
        // Parse natural language time
        match crate::scheduler::parse_time_expression(execute_at, timezone) {
            Ok(ParsedSchedule::OneTime(t)) => (t.to_rfc3339(), None, Some(t.to_rfc3339())),
            Ok(ParsedSchedule::Recurring { cron, next }) => {
                (next.to_rfc3339(), Some(cron), None)
            }
            Err(e) => {
                return ToastTemplate {
                    message: format!("Could not parse time: {}", e),
                    is_error: true,
                };
            }
        }
    };

    // Create the scheduled prompt
    let schedule = ScheduledPrompt {
        id: uuid::Uuid::new_v4().to_string(),
        channel_name: channel.to_string(),
        room_id: channel_info.room_id,
        prompt: prompt.to_string(),
        created_by: "admin".to_string(),
        created_at: chrono::Utc::now().to_rfc3339(),
        execute_at: execute_at_field,
        cron_expression,
        last_executed_at: None,
        next_execution_at,
        status: ScheduleStatus::Active,
        error_message: None,
        execution_count: 0,
    };

    // Create the schedule
    match state.scheduler_store.create_schedule(&schedule) {
        Ok(_) => ToastTemplate {
            message: "Schedule created successfully".to_string(),
            is_error: false,
        },
        Err(e) => ToastTemplate {
            message: format!("Failed to create schedule: {}", e),
            is_error: true,
        },
    }
}
