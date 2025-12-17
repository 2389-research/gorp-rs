// ABOUTME: Admin panel route handlers
// ABOUTME: Handles config viewing, editing, and session management

use axum::{
    extract::{Path as AxumPath, State},
    response::{IntoResponse, Response},
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
    BrowseEntry, ChannelDetailTemplate, ChannelListTemplate, ChannelRow, ConfigTemplate,
    DashboardTemplate, DirectoryTemplate, ErrorEntry, FileTemplate, HealthTemplate,
    LogViewerTemplate, MarkdownTemplate, MatrixDirTemplate, MatrixFileEntry, MessageEntry,
    MessageHistoryTemplate, ScheduleFormTemplate, ScheduleRow, SchedulesTemplate, SearchResult,
    SearchTemplate, ToastTemplate,
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
        .route("/channels/{name}/matrix", get(channel_matrix_dir))
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
        .route("/browse", get(browse_root))
        .route("/browse/{*path}", get(browse_path))
        .route("/render/{*path}", get(render_markdown))
        .route("/search", get(search_workspace))
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
                .join(".gorp")
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
        .join(".gorp")
        .join("matrix_messages.log");

    // Use efficient tail reading - only reads last 100 lines without loading entire file
    let log_lines = read_last_n_lines(&log_path, 100);

    Ok(LogViewerTemplate {
        title: format!("Logs: {} - gorp Admin", channel.channel_name),
        channel_name: channel.channel_name,
        log_lines,
    })
}

async fn channel_matrix_dir(
    State(state): State<AdminState>,
    AxumPath(name): AxumPath<String>,
) -> Result<MatrixDirTemplate, ToastTemplate> {
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

    let gorp_dir = Path::new(&channel.directory).join(".gorp");

    // Check if .gorp directory exists
    if !gorp_dir.exists() {
        return Err(ToastTemplate {
            message: format!("No .gorp/ directory found for channel '{}'", name),
            is_error: true,
        });
    }

    // Check debug mode
    let debug_enabled = gorp_dir.join("enable-debug").exists();

    // Read context.json if it exists
    let context_json = {
        let context_path = gorp_dir.join("context.json");
        if context_path.exists() {
            match std::fs::read_to_string(&context_path) {
                Ok(content) => {
                    // Try to parse and pretty-print the JSON
                    match serde_json::from_str::<serde_json::Value>(&content) {
                        Ok(value) => match serde_json::to_string_pretty(&value) {
                            Ok(pretty) => Some(pretty),
                            Err(_) => Some(content), // Fall back to raw content
                        },
                        Err(_) => Some(content), // Not valid JSON, show raw
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        channel = %name,
                        error = %e,
                        "Failed to read context.json"
                    );
                    None
                }
            }
        } else {
            None
        }
    };

    // Read directory contents
    let entries = std::fs::read_dir(&gorp_dir).map_err(|e| ToastTemplate {
        message: format!("Failed to read .gorp/ directory: {}", e),
        is_error: true,
    })?;

    let mut files: Vec<MatrixFileEntry> = Vec::new();

    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                tracing::warn!(error = %e, "Failed to read directory entry, skipping");
                continue;
            }
        };

        let metadata = match entry.metadata() {
            Ok(m) => m,
            Err(e) => {
                tracing::warn!(
                    path = %entry.path().display(),
                    error = %e,
                    "Failed to read metadata, skipping entry"
                );
                continue;
            }
        };

        // Only include files, not directories
        if !metadata.is_file() {
            continue;
        }

        let file_name = entry.file_name();
        let name = file_name.to_string_lossy().to_string();

        let size_display = format_file_size(metadata.len());
        let modified = metadata
            .modified()
            .map(format_modified_time)
            .unwrap_or_else(|_| "Unknown".to_string());

        let is_log = name.ends_with(".log");

        files.push(MatrixFileEntry {
            name,
            size_display,
            modified,
            is_log,
        });
    }

    // Sort files alphabetically
    files.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

    Ok(MatrixDirTemplate {
        title: format!(".gorp/: {} - gorp Admin", channel.channel_name),
        channel_name: channel.channel_name,
        files,
        context_json,
        debug_enabled,
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

    let debug_dir = Path::new(&channel.directory).join(".gorp");
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
        .join(".gorp")
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
            .join(".gorp")
            .join("matrix_messages.log");

        // Use efficient tail reading
        let lines = read_last_n_lines(&log_path, lines_per_channel);

        for line in lines {
            if line.trim().is_empty() {
                continue;
            }

            if let Ok(entry) = serde_json::from_str::<MessageLogEntry>(&line) {
                let content_preview: String = entry.content.chars().take(50).collect::<String>()
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
    use crate::scheduler::{ParsedSchedule, ScheduleStatus, ScheduledPrompt};

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
        && execute_at
            .chars()
            .all(|c| c.is_ascii_digit() || " */-,".contains(c));

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
            Ok(ParsedSchedule::Recurring { cron, next }) => (next.to_rfc3339(), Some(cron), None),
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

// ============================================================================
// Workspace Browser Handlers
// ============================================================================

/// Maximum file size to display in browser (100KB)
const MAX_DISPLAY_FILE_SIZE: u64 = 100 * 1024;

/// Browse workspace root (list all channel directories)
async fn browse_root(State(state): State<AdminState>) -> Result<DirectoryTemplate, ToastTemplate> {
    browse_directory(state, "").await
}

/// Unified response type for browse endpoints
enum BrowseResponse {
    Directory(DirectoryTemplate),
    File(FileTemplate),
    Error(ToastTemplate),
}

impl IntoResponse for BrowseResponse {
    fn into_response(self) -> Response {
        match self {
            BrowseResponse::Directory(t) => t.into_response(),
            BrowseResponse::File(t) => t.into_response(),
            BrowseResponse::Error(t) => t.into_response(),
        }
    }
}

/// Browse a specific path in the workspace
async fn browse_path(
    State(state): State<AdminState>,
    AxumPath(path): AxumPath<String>,
) -> BrowseResponse {
    let workspace_root = Path::new(&state.config.workspace.path);
    let full_path = match validate_and_resolve_path(workspace_root, &path) {
        Ok(p) => p,
        Err(e) => return BrowseResponse::Error(e),
    };

    if full_path.is_file() {
        // View file content
        match view_file(&full_path, &path) {
            Ok(template) => BrowseResponse::File(template),
            Err(e) => BrowseResponse::Error(e),
        }
    } else {
        // Browse directory
        match browse_directory(state, &path).await {
            Ok(template) => BrowseResponse::Directory(template),
            Err(e) => BrowseResponse::Error(e),
        }
    }
}

/// View file content with size limiting
fn view_file(
    full_path: &std::path::Path,
    relative_path: &str,
) -> Result<FileTemplate, ToastTemplate> {
    let metadata = std::fs::metadata(full_path).map_err(|e| ToastTemplate {
        message: format!("Failed to read file metadata: {}", e),
        is_error: true,
    })?;

    let size = metadata.len();
    let is_truncated = size > MAX_DISPLAY_FILE_SIZE;

    // Check if file is markdown
    let is_markdown = full_path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("md"))
        .unwrap_or(false);

    // Read file content (truncated if too large)
    let content = if is_truncated {
        let mut file = File::open(full_path).map_err(|e| ToastTemplate {
            message: format!("Failed to open file: {}", e),
            is_error: true,
        })?;
        let mut buffer = vec![0u8; MAX_DISPLAY_FILE_SIZE as usize];
        file.read(&mut buffer).map_err(|e| ToastTemplate {
            message: format!("Failed to read file: {}", e),
            is_error: true,
        })?;
        String::from_utf8_lossy(&buffer).to_string()
    } else {
        std::fs::read_to_string(full_path).unwrap_or_else(|_| {
            // Binary file - show hex preview
            match std::fs::read(full_path) {
                Ok(bytes) => {
                    let preview: String = bytes
                        .iter()
                        .take(1024)
                        .map(|b| format!("{:02x} ", b))
                        .collect();
                    format!("[Binary file - hex preview:]\n{}", preview)
                }
                Err(e) => format!("[Failed to read file: {}]", e),
            }
        })
    };

    // Calculate parent path for back navigation
    let parent_path = if relative_path.contains('/') {
        let segments: Vec<&str> = relative_path.rsplitn(2, '/').collect();
        segments.get(1).unwrap_or(&"").to_string()
    } else {
        String::new() // Root level
    };

    let file_name = full_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| relative_path.to_string());

    Ok(FileTemplate {
        title: format!("View: {} - gorp Admin", file_name),
        path: relative_path.to_string(),
        parent_path,
        content,
        size_display: format_file_size(size),
        is_truncated,
        is_markdown,
    })
}

/// Validate path and prevent directory traversal attacks
fn validate_and_resolve_path(
    workspace_root: &Path,
    user_path: &str,
) -> Result<std::path::PathBuf, ToastTemplate> {
    // Reject paths with ".." to prevent traversal
    if user_path.contains("..") {
        tracing::warn!(
            path = user_path,
            "Path traversal attempt blocked: contains '..'"
        );
        return Err(ToastTemplate {
            message: "Invalid path: contains path traversal".to_string(),
            is_error: true,
        });
    }

    // Build the full path
    let full_path = if user_path.is_empty() {
        workspace_root.to_path_buf()
    } else {
        workspace_root.join(user_path)
    };

    // Canonicalize both paths to resolve symlinks and validate
    let canonical_workspace = workspace_root.canonicalize().map_err(|e| ToastTemplate {
        message: format!("Workspace path error: {}", e),
        is_error: true,
    })?;

    let canonical_full = full_path.canonicalize().map_err(|e| ToastTemplate {
        message: format!("Path not found: {}", e),
        is_error: true,
    })?;

    // Verify the resolved path is within workspace
    if !canonical_full.starts_with(&canonical_workspace) {
        tracing::warn!(
            requested_path = user_path,
            resolved_path = %canonical_full.display(),
            workspace_root = %canonical_workspace.display(),
            "Path traversal attempt blocked: resolved path outside workspace"
        );
        return Err(ToastTemplate {
            message: "Access denied: path outside workspace".to_string(),
            is_error: true,
        });
    }

    Ok(canonical_full)
}

/// Format file size in human-readable format
fn format_file_size(size: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if size >= GB {
        format!("{:.2} GB", size as f64 / GB as f64)
    } else if size >= MB {
        format!("{:.2} MB", size as f64 / MB as f64)
    } else if size >= KB {
        format!("{:.2} KB", size as f64 / KB as f64)
    } else {
        format!("{} B", size)
    }
}

/// Format modified time in human-readable format
fn format_modified_time(modified: std::time::SystemTime) -> String {
    use chrono::{DateTime, Local};
    let datetime: DateTime<Local> = modified.into();
    datetime.format("%Y-%m-%d %H:%M:%S").to_string()
}

/// Browse a directory and return listing
async fn browse_directory(
    state: AdminState,
    relative_path: &str,
) -> Result<DirectoryTemplate, ToastTemplate> {
    let workspace_root = Path::new(&state.config.workspace.path);
    let full_path = validate_and_resolve_path(workspace_root, relative_path)?;

    if !full_path.is_dir() {
        return Err(ToastTemplate {
            message: "Not a directory".to_string(),
            is_error: true,
        });
    }

    // Read directory entries
    let entries = std::fs::read_dir(&full_path).map_err(|e| ToastTemplate {
        message: format!("Failed to read directory: {}", e),
        is_error: true,
    })?;

    let mut browse_entries: Vec<BrowseEntry> = Vec::new();

    for entry in entries {
        // Skip entries we can't read (permission denied, broken symlinks, etc.)
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                tracing::warn!(error = %e, "Failed to read directory entry, skipping");
                continue;
            }
        };

        // Skip entries we can't get metadata for
        let metadata = match entry.metadata() {
            Ok(m) => m,
            Err(e) => {
                tracing::warn!(
                    path = %entry.path().display(),
                    error = %e,
                    "Failed to read metadata, skipping entry"
                );
                continue;
            }
        };

        let file_name = entry.file_name();
        let name = file_name.to_string_lossy().to_string();

        // Skip hidden files/directories (starting with .)
        if name.starts_with('.') {
            continue;
        }

        let is_dir = metadata.is_dir();
        let (size_bytes, size_display) = if is_dir {
            (None, "-".to_string())
        } else {
            let bytes = metadata.len();
            (Some(bytes), format_file_size(bytes))
        };

        // Check if file is markdown
        let is_markdown = if !is_dir {
            entry
                .path()
                .extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| ext.eq_ignore_ascii_case("md"))
                .unwrap_or(false)
        } else {
            false
        };

        let modified = metadata
            .modified()
            .map(format_modified_time)
            .unwrap_or_else(|_| "Unknown".to_string());

        // Build URL path
        let url_path = if relative_path.is_empty() {
            name.clone()
        } else {
            format!("{}/{}", relative_path, name)
        };

        browse_entries.push(BrowseEntry {
            name,
            path: url_path,
            is_dir,
            is_markdown,
            size_bytes,
            size_display,
            modified,
        });
    }

    // Sort: directories first, then files, alphabetically within each group
    browse_entries.sort_by(|a, b| match (a.is_dir, b.is_dir) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
    });

    // Calculate parent path for breadcrumb navigation
    let parent_path = if relative_path.is_empty() {
        None
    } else {
        // Get parent by removing last segment
        let segments: Vec<&str> = relative_path.split('/').collect();
        if segments.len() == 1 {
            Some(String::new()) // Back to root
        } else {
            Some(segments[..segments.len() - 1].join("/"))
        }
    };

    let current_path = if relative_path.is_empty() {
        "Workspace Root".to_string()
    } else {
        relative_path.to_string()
    };

    Ok(DirectoryTemplate {
        title: format!("Browse: {} - gorp Admin", current_path),
        current_path,
        parent_path,
        entries: browse_entries,
    })
}

// ============================================================================
// Markdown Renderer Handler
// ============================================================================

/// Render markdown file as HTML
async fn render_markdown(
    State(state): State<AdminState>,
    AxumPath(path): AxumPath<String>,
) -> Result<MarkdownTemplate, ToastTemplate> {
    let workspace_root = Path::new(&state.config.workspace.path);
    let full_path = validate_and_resolve_path(workspace_root, &path)?;

    // Verify it's a file
    if !full_path.is_file() {
        return Err(ToastTemplate {
            message: "Not a file".to_string(),
            is_error: true,
        });
    }

    // Verify it's a markdown file
    let is_markdown = full_path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("md"))
        .unwrap_or(false);

    if !is_markdown {
        return Err(ToastTemplate {
            message: "Not a markdown file".to_string(),
            is_error: true,
        });
    }

    // Check file size before reading (limit to 1MB for markdown rendering)
    const MAX_MARKDOWN_SIZE: u64 = 1024 * 1024;
    let metadata = std::fs::metadata(&full_path).map_err(|e| ToastTemplate {
        message: format!("Failed to read file metadata: {}", e),
        is_error: true,
    })?;
    if metadata.len() > MAX_MARKDOWN_SIZE {
        return Err(ToastTemplate {
            message: format!(
                "File too large to render ({} MB). Maximum is 1 MB.",
                metadata.len() / (1024 * 1024)
            ),
            is_error: true,
        });
    }

    // Read file content
    let markdown_content = std::fs::read_to_string(&full_path).map_err(|e| ToastTemplate {
        message: format!("Failed to read file: {}", e),
        is_error: true,
    })?;

    // Parse markdown to HTML using pulldown-cmark
    use pulldown_cmark::{html, Options, Parser};
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);

    let parser = Parser::new_ext(&markdown_content, options);
    let mut html_output = String::new();
    html::push_html(&mut html_output, parser);

    // Calculate parent path for back navigation
    let parent_path = if path.contains('/') {
        let segments: Vec<&str> = path.rsplitn(2, '/').collect();
        segments.get(1).unwrap_or(&"").to_string()
    } else {
        String::new() // Root level
    };

    let file_name = full_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string());

    Ok(MarkdownTemplate {
        title: format!("{} - gorp Admin", file_name),
        path: path.to_string(),
        parent_path,
        content_html: html_output,
    })
}

// ============================================================================
// Workspace Search Handler
// ============================================================================

/// Query parameter for search
#[derive(Deserialize)]
pub struct SearchQuery {
    #[serde(default)]
    q: String,
}

/// Maximum number of search results to return
const MAX_SEARCH_RESULTS: usize = 100;

/// Maximum file size to search (100KB)
const MAX_SEARCH_FILE_SIZE: u64 = 100 * 1024;

/// Maximum number of files to scan
const MAX_FILES_TO_SCAN: usize = 1000;

/// Context lines to show around matches
const SEARCH_CONTEXT_CHARS: usize = 150;

/// Search across all channel workspaces for files and content
async fn search_workspace(
    State(state): State<AdminState>,
    axum::extract::Query(query): axum::extract::Query<SearchQuery>,
) -> SearchTemplate {
    let search_query = query.q.trim();

    // Return empty search form if no query
    if search_query.is_empty() {
        return SearchTemplate {
            title: "Search Workspace - gorp Admin".to_string(),
            query: String::new(),
            results: Vec::new(),
            search_performed: false,
        };
    }

    // Get all channels
    let channels = match state.session_store.list_all() {
        Ok(c) => c,
        Err(e) => {
            tracing::error!(error = %e, "Failed to list channels for search");
            Vec::new()
        }
    };

    let mut results: Vec<SearchResult> = Vec::new();
    let mut files_scanned = 0;
    let search_query_lower = search_query.to_lowercase();

    // Search each channel workspace
    for channel in channels {
        // Validate directory path
        if let Err(e) = channel.validate_directory() {
            tracing::warn!(
                channel = %channel.channel_name,
                error = %e,
                "Skipping channel with invalid directory in search"
            );
            continue;
        }

        let channel_dir = Path::new(&channel.directory);
        if !channel_dir.exists() || !channel_dir.is_dir() {
            continue;
        }

        // Walk the directory tree
        let walker = match std::fs::read_dir(channel_dir) {
            Ok(entries) => entries,
            Err(e) => {
                tracing::warn!(
                    channel = %channel.channel_name,
                    error = %e,
                    "Failed to read channel directory for search"
                );
                continue;
            }
        };

        // Recursively search this channel
        search_directory_recursive(
            walker,
            channel_dir,
            &channel.channel_name,
            &search_query_lower,
            &mut results,
            &mut files_scanned,
            0, // Start at depth 0
        );

        // Stop if we've scanned too many files
        if files_scanned >= MAX_FILES_TO_SCAN {
            break;
        }

        // Stop if we've found enough results
        if results.len() >= MAX_SEARCH_RESULTS {
            break;
        }
    }

    // Sort results by channel name, then file path
    results.sort_by(|a, b| {
        a.channel_name
            .cmp(&b.channel_name)
            .then_with(|| a.file_path.cmp(&b.file_path))
    });

    // Limit to MAX_SEARCH_RESULTS
    results.truncate(MAX_SEARCH_RESULTS);

    SearchTemplate {
        title: format!("Search: {} - gorp Admin", search_query),
        query: search_query.to_string(),
        results,
        search_performed: true,
    }
}

/// Maximum recursion depth for directory search (prevents stack overflow)
const MAX_SEARCH_DEPTH: usize = 20;

/// Recursively search a directory
fn search_directory_recursive(
    entries: std::fs::ReadDir,
    base_dir: &Path,
    channel_name: &str,
    query: &str,
    results: &mut Vec<SearchResult>,
    files_scanned: &mut usize,
    depth: usize,
) {
    // Stop if we've exceeded depth limit
    if depth > MAX_SEARCH_DEPTH {
        return;
    }

    for entry in entries {
        // Stop if we've scanned enough files or found enough results
        if *files_scanned >= MAX_FILES_TO_SCAN || results.len() >= MAX_SEARCH_RESULTS {
            return;
        }

        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };

        let path = entry.path();
        let file_name = entry.file_name();
        let name = file_name.to_string_lossy();

        // Skip hidden files/directories
        if name.starts_with('.') {
            continue;
        }

        let metadata = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };

        if metadata.is_dir() {
            // Recursively search subdirectory
            if let Ok(sub_entries) = std::fs::read_dir(&path) {
                search_directory_recursive(
                    sub_entries,
                    base_dir,
                    channel_name,
                    query,
                    results,
                    files_scanned,
                    depth + 1,
                );
            }
        } else if metadata.is_file() {
            *files_scanned += 1;

            // Calculate relative path within channel
            let relative_path = match path.strip_prefix(base_dir) {
                Ok(p) => p.to_string_lossy().to_string(),
                Err(_) => continue,
            };

            let file_name_lower = name.to_lowercase();

            // Check if filename matches
            if file_name_lower.contains(query) {
                results.push(SearchResult {
                    channel_name: channel_name.to_string(),
                    file_path: relative_path.clone(),
                    browse_path: format!("{}/{}", channel_name, relative_path),
                    file_name: name.to_string(),
                    match_preview: format!("Filename matches: {}", name),
                    line_number: None,
                });
                continue;
            }

            // Skip files that are too large
            if metadata.len() > MAX_SEARCH_FILE_SIZE {
                continue;
            }

            // Try to search file content
            if let Some(result) =
                search_file_content(&path, &relative_path, &name, channel_name, query)
            {
                results.push(result);
            }
        }
    }
}

/// Search a file's content for the query
fn search_file_content(
    file_path: &Path,
    relative_path: &str,
    file_name: &str,
    channel_name: &str,
    query: &str,
) -> Option<SearchResult> {
    // Try to read file as text
    let content = match std::fs::read_to_string(file_path) {
        Ok(c) => c,
        Err(_) => {
            // Try to detect if it's binary
            if let Ok(bytes) = std::fs::read(file_path) {
                // Check for null bytes (common in binary files)
                if bytes.iter().take(512).any(|&b| b == 0) {
                    return None; // Skip binary files
                }
            }
            return None;
        }
    };

    let content_lower = content.to_lowercase();

    // Find first match
    if let Some(match_pos) = content_lower.find(query) {
        // Find the line number
        let line_number = content[..match_pos].lines().count() as u32 + 1;

        // Extract context around the match
        let start = match_pos.saturating_sub(SEARCH_CONTEXT_CHARS / 2);
        let end = (match_pos + query.len() + SEARCH_CONTEXT_CHARS / 2).min(content.len());

        let mut preview = content[start..end].to_string();

        // Add ellipsis if truncated
        if start > 0 {
            preview = format!("...{}", preview);
        }
        if end < content.len() {
            preview = format!("{}...", preview);
        }

        // Trim to single line if multi-line
        if let Some(first_line) = preview.lines().next() {
            preview = first_line.to_string();
        }

        Some(SearchResult {
            channel_name: channel_name.to_string(),
            file_path: relative_path.to_string(),
            browse_path: format!("{}/{}", channel_name, relative_path),
            file_name: file_name.to_string(),
            match_preview: preview,
            line_number: Some(line_number),
        })
    } else {
        None
    }
}
