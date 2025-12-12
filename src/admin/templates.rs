// ABOUTME: Askama template structs for admin panel
// ABOUTME: Templates are compiled into binary at build time

use askama::Template;

#[derive(Template)]
#[template(path = "admin/dashboard.html")]
pub struct DashboardTemplate {
    pub title: String,
    pub total_channels: usize,
    pub active_channels: usize,
    pub total_schedules: usize,
    pub messages_today: usize,
}

#[derive(Template)]
#[template(path = "admin/config.html")]
pub struct ConfigTemplate {
    pub title: String,
    pub home_server: String,
    pub user_id: String,
    pub device_name: String,
    pub room_prefix: String,
    pub allowed_users: String,
    pub webhook_port: u16,
    pub webhook_host: String,
    pub webhook_api_key_set: bool,
    pub workspace_path: String,
    pub scheduler_timezone: String,
    pub password_set: bool,
    pub access_token_set: bool,
    pub recovery_key_set: bool,
}

#[derive(Template)]
#[template(path = "partials/toast.html")]
pub struct ToastTemplate {
    pub message: String,
    pub is_error: bool,
}

/// Channel row data for list view
#[derive(Clone)]
pub struct ChannelRow {
    pub name: String,
    pub room_id: String,
    pub started: bool,
    pub debug_enabled: bool,
    pub directory: String,
    pub created_at: String,
}

#[derive(Template)]
#[template(path = "admin/channels/list.html")]
pub struct ChannelListTemplate {
    pub title: String,
    pub channels: Vec<ChannelRow>,
}

#[derive(Template)]
#[template(path = "admin/channels/detail.html")]
pub struct ChannelDetailTemplate {
    pub title: String,
    pub name: String,
    pub room_id: String,
    pub session_id: String,
    pub directory: String,
    pub started: bool,
    pub debug_enabled: bool,
    pub webhook_url: String,
    pub created_at: String,
}

#[derive(Template)]
#[template(path = "admin/health.html")]
pub struct HealthTemplate {
    pub title: String,
    pub homeserver: String,
    pub bot_user_id: String,
    pub device_name: String,
    pub webhook_port: u16,
    pub webhook_host: String,
    pub timezone: String,
    pub total_channels: usize,
    pub active_channels: usize,
    pub total_schedules: usize,
    pub active_schedules: usize,
    pub recent_errors: Vec<ErrorEntry>,
}

/// Error entry data for health view
#[derive(Clone)]
pub struct ErrorEntry {
    pub timestamp: String,
    pub source: String,
    pub message: String,
}

/// Schedule row data for list view
#[derive(Clone)]
pub struct ScheduleRow {
    pub id: String,
    pub channel_name: String,
    pub prompt_preview: String,
    pub schedule_type: String,
    pub cron_expression: Option<String>,
    pub next_execution: String,
    pub status: String,
    pub status_icon: String,
    pub execution_count: i32,
    pub created_at: String,
    pub error_message: Option<String>,
}

#[derive(Template)]
#[template(path = "admin/schedules.html")]
pub struct SchedulesTemplate {
    pub title: String,
    pub schedules: Vec<ScheduleRow>,
}

#[derive(Template)]
#[template(path = "admin/channels/logs.html")]
pub struct LogViewerTemplate {
    pub title: String,
    pub channel_name: String,
    pub log_lines: Vec<String>,
}

/// Message entry for message history view
#[derive(Clone)]
pub struct MessageEntry {
    pub timestamp: String,
    pub channel_name: String,
    pub direction: String,
    pub sender: String,
    pub content_preview: String,
}

#[derive(Template)]
#[template(path = "admin/messages.html")]
pub struct MessageHistoryTemplate {
    pub title: String,
    pub messages: Vec<MessageEntry>,
}

#[derive(Template)]
#[template(path = "admin/schedules/new.html")]
pub struct ScheduleFormTemplate {
    pub title: String,
    pub channels: Vec<String>,
}

/// Browse entry for workspace directory listing
#[derive(Clone)]
pub struct BrowseEntry {
    pub name: String,
    pub path: String,        // URL path for linking
    pub is_dir: bool,
    pub is_markdown: bool,   // Whether file is markdown
    pub size_bytes: Option<u64>,   // File size in bytes (None for dirs)
    pub size_display: String,      // Human-readable size
    pub modified: String,    // Human-readable date
}

#[derive(Template)]
#[template(path = "admin/browse/directory.html")]
pub struct DirectoryTemplate {
    pub title: String,
    pub current_path: String,  // Display path
    pub parent_path: Option<String>,  // Link to parent (None at root)
    pub entries: Vec<BrowseEntry>,
}

#[derive(Template)]
#[template(path = "admin/browse/file.html")]
pub struct FileTemplate {
    pub title: String,
    pub path: String,
    pub parent_path: String,  // For back navigation
    pub content: String,      // File content (truncated if too large)
    pub size_display: String, // Human-readable size
    pub is_truncated: bool,
    pub is_markdown: bool,    // Whether file is markdown
}

#[derive(Template)]
#[template(path = "admin/browse/markdown.html")]
pub struct MarkdownTemplate {
    pub title: String,
    pub path: String,
    pub parent_path: String,
    pub content_html: String,  // Already converted to HTML
}

/// .matrix/ directory file entry
#[derive(Clone)]
pub struct MatrixFileEntry {
    pub name: String,
    pub size_display: String,
    pub modified: String,
    pub is_log: bool,  // true for .log files
}

#[derive(Template)]
#[template(path = "admin/channels/matrix.html")]
pub struct MatrixDirTemplate {
    pub title: String,
    pub channel_name: String,
    pub files: Vec<MatrixFileEntry>,
    pub context_json: Option<String>,  // Pretty-printed context.json if exists
    pub debug_enabled: bool,
}

/// Search result entry
#[derive(Clone)]
pub struct SearchResult {
    pub channel_name: String,
    pub file_path: String,       // Relative path within channel (for display)
    pub browse_path: String,     // Full path for /admin/browse URL (channel/file_path)
    pub file_name: String,
    pub match_preview: String,   // Context around match
    pub line_number: Option<u32>,
}

#[derive(Template)]
#[template(path = "admin/search.html")]
pub struct SearchTemplate {
    pub title: String,
    pub query: String,
    pub results: Vec<SearchResult>,
    pub search_performed: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dashboard_template_renders() {
        let template = DashboardTemplate {
            title: "Test Dashboard".to_string(),
            total_channels: 5,
            active_channels: 3,
            total_schedules: 10,
            messages_today: 42,
        };
        let rendered = template.render().unwrap();
        assert!(rendered.contains("Test Dashboard"));
        assert!(rendered.contains("gorp"));
        assert!(rendered.contains("Configuration"));
        assert!(rendered.contains("Total Channels"));
        assert!(rendered.contains("Active Channels"));
        assert!(rendered.contains("Total Schedules"));
        assert!(rendered.contains("Messages Today"));
    }

    #[test]
    fn test_config_template_renders() {
        let template = ConfigTemplate {
            title: "Config Test".to_string(),
            home_server: "https://matrix.org".to_string(),
            user_id: "@test:matrix.org".to_string(),
            device_name: "test-device".to_string(),
            room_prefix: "TEST".to_string(),
            allowed_users: "@user1:matrix.org, @user2:matrix.org".to_string(),
            webhook_port: 13000,
            webhook_host: "localhost".to_string(),
            webhook_api_key_set: true,
            workspace_path: "/home/test/workspace".to_string(),
            scheduler_timezone: "America/Chicago".to_string(),
            password_set: true,
            access_token_set: false,
            recovery_key_set: true,
        };
        let rendered = template.render().unwrap();
        assert!(rendered.contains("https://matrix.org"));
        assert!(rendered.contains("@test:matrix.org"));
        assert!(rendered.contains("Configured")); // For password_set = true
        assert!(rendered.contains("Not set")); // For access_token_set = false
    }

    #[test]
    fn test_toast_success_renders() {
        let template = ToastTemplate {
            message: "Config saved!".to_string(),
            is_error: false,
        };
        let rendered = template.render().unwrap();
        assert!(rendered.contains("Config saved!"));
        assert!(rendered.contains("bg-green-500"));
    }

    #[test]
    fn test_toast_error_renders() {
        let template = ToastTemplate {
            message: "Save failed".to_string(),
            is_error: true,
        };
        let rendered = template.render().unwrap();
        assert!(rendered.contains("Save failed"));
        assert!(rendered.contains("bg-red-500"));
    }

    #[test]
    fn test_health_template_renders_no_errors() {
        let template = HealthTemplate {
            title: "Health Test".to_string(),
            homeserver: "https://matrix.org".to_string(),
            bot_user_id: "@bot:matrix.org".to_string(),
            device_name: "test-device".to_string(),
            webhook_port: 13000,
            webhook_host: "localhost".to_string(),
            timezone: "America/Chicago".to_string(),
            total_channels: 5,
            active_channels: 3,
            total_schedules: 10,
            active_schedules: 7,
            recent_errors: vec![],
        };
        let rendered = template.render().unwrap();
        assert!(rendered.contains("Health Test"));
        assert!(rendered.contains("No Recent Errors"));
        assert!(rendered.contains("All systems operating normally"));
        assert!(rendered.contains("bg-green-50"));
    }

    #[test]
    fn test_health_template_renders_with_errors() {
        let template = HealthTemplate {
            title: "Health Test".to_string(),
            homeserver: "https://matrix.org".to_string(),
            bot_user_id: "@bot:matrix.org".to_string(),
            device_name: "test-device".to_string(),
            webhook_port: 13000,
            webhook_host: "localhost".to_string(),
            timezone: "America/Chicago".to_string(),
            total_channels: 5,
            active_channels: 3,
            total_schedules: 10,
            active_schedules: 7,
            recent_errors: vec![
                ErrorEntry {
                    timestamp: "2025-12-11T10:00:00".to_string(),
                    source: "Schedule: test-channel".to_string(),
                    message: "Failed to execute prompt".to_string(),
                },
                ErrorEntry {
                    timestamp: "2025-12-11T09:00:00".to_string(),
                    source: "Schedule: another-channel".to_string(),
                    message: "Channel no longer exists".to_string(),
                },
            ],
        };
        let rendered = template.render().unwrap();
        assert!(rendered.contains("Health Test"));
        assert!(rendered.contains("Schedule: test-channel"));
        assert!(rendered.contains("Failed to execute prompt"));
        assert!(rendered.contains("2025-12-11T10:00:00"));
        assert!(rendered.contains("bg-red-50"));
        assert!(!rendered.contains("No Recent Errors"));
    }
}
