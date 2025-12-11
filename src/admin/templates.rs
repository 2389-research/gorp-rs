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
