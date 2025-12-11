// ABOUTME: Askama template structs for admin panel
// ABOUTME: Templates are compiled into binary at build time

use askama::Template;

#[derive(Template)]
#[template(path = "admin/dashboard.html")]
pub struct DashboardTemplate {
    pub title: String,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dashboard_template_renders() {
        let template = DashboardTemplate {
            title: "Test Dashboard".to_string(),
        };
        let rendered = template.render().unwrap();
        assert!(rendered.contains("Test Dashboard"));
        assert!(rendered.contains("gorp"));
        assert!(rendered.contains("Configuration"));
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
}
