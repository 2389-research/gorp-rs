// ABOUTME: Settings view - displays current gorp configuration with refined card styling
// ABOUTME: Read-only view of Config from ServerState, organized in collapsible sections

use crate::gui::app::Message;
use crate::gui::theme::{colors, content_style, radius, spacing, surface_style, text_size};
use crate::server::ServerState;
use iced::widget::{column, container, row, scrollable, text, Space};
use iced::{Alignment, Border, Element, Length};
use std::sync::Arc;

/// Setting row with label and value
fn setting_row<'a>(label: &'a str, value: String, is_sensitive: bool) -> Element<'a, Message> {
    let display_value = if is_sensitive && !value.is_empty() && value != "(not set)" {
        "••••••••".to_string()
    } else {
        value
    };

    row![
        text(label)
            .size(text_size::BODY)
            .color(colors::TEXT_SECONDARY)
            .width(Length::Fixed(160.0)),
        text(display_value)
            .size(text_size::BODY)
            .color(colors::TEXT_PRIMARY),
    ]
    .spacing(spacing::SM)
    .into()
}

/// Section card with title and content
fn section_card<'a>(
    icon: &'a str,
    title: &'a str,
    children: Vec<Element<'a, Message>>,
) -> Element<'a, Message> {
    let mut content = column![
        row![
            text(icon)
                .size(text_size::LARGE)
                .color(colors::ACCENT_PRIMARY),
            Space::with_width(spacing::SM),
            text(title)
                .size(text_size::LARGE)
                .color(colors::TEXT_PRIMARY),
        ]
        .align_y(Alignment::Center),
        Space::with_height(spacing::MD),
    ]
    .spacing(spacing::XS);

    for child in children {
        content = content.push(child);
    }

    container(content)
        .padding(spacing::LG)
        .width(Length::Fill)
        .style(surface_style)
        .into()
}

/// Config file path hint
fn config_hint<'a>() -> Element<'a, Message> {
    container(
        row![
            text("ℹ").size(text_size::BODY).color(colors::TEXT_TERTIARY),
            Space::with_width(spacing::SM),
            text("Edit config.toml to change settings. Changes require restart.")
                .size(text_size::SMALL)
                .color(colors::TEXT_TERTIARY),
        ]
        .align_y(Alignment::Center),
    )
    .padding([spacing::SM, spacing::MD])
    .style(|_theme| container::Style {
        background: Some(colors::BG_ELEVATED.into()),
        border: Border {
            radius: radius::MD.into(),
            ..Default::default()
        },
        ..Default::default()
    })
    .into()
}

pub fn view(server: Option<&Arc<ServerState>>) -> Element<'static, Message> {
    let content: Element<'static, Message> = if let Some(server) = server {
        let config = &server.config;

        // Pre-compute all strings
        let homeserver = config.matrix.home_server.clone();
        let user_id = config.matrix.user_id.clone();
        let device_name = config.matrix.device_name.clone();
        let room_prefix = config.matrix.room_prefix.clone();
        let allowed_users = if config.matrix.allowed_users.is_empty() {
            "(none)".to_string()
        } else {
            config.matrix.allowed_users.join(", ")
        };
        let auth_method = if config.matrix.access_token.is_some() {
            "Access Token".to_string()
        } else {
            "Password".to_string()
        };

        let backend_type = config.backend.backend_type.clone();
        let binary = config
            .backend
            .binary
            .clone()
            .unwrap_or_else(|| "(default)".to_string());
        let model = config
            .backend
            .model
            .clone()
            .unwrap_or_else(|| "(not set)".to_string());
        let timeout = format!("{} sec", config.backend.timeout_secs);
        let keep_alive = format!("{} sec", config.backend.keep_alive_secs);
        let pre_warm = format!("{} sec", config.backend.pre_warm_secs);
        let mcp_count = config.backend.mcp_servers.len();
        let mcp_servers = if mcp_count == 0 {
            "None configured".to_string()
        } else {
            format!(
                "{} server{}",
                mcp_count,
                if mcp_count == 1 { "" } else { "s" }
            )
        };

        let webhook_host = config.webhook.host.clone();
        let webhook_port = config.webhook.port.to_string();
        let api_key_status = if config.webhook.api_key.is_some() {
            "Configured".to_string()
        } else {
            "(not set)".to_string()
        };

        let workspace_path = config.workspace.path.clone();
        let timezone = config.scheduler.timezone.clone();

        // Header
        let header = column![
            text("Settings")
                .size(text_size::HEADING)
                .color(colors::TEXT_PRIMARY),
            Space::with_height(spacing::XXS),
            text("Current configuration (read-only)")
                .size(text_size::BODY)
                .color(colors::TEXT_SECONDARY),
        ];

        // Matrix section
        let matrix_section = section_card(
            "🔗",
            "Matrix Connection",
            vec![
                setting_row("Homeserver", homeserver, false),
                setting_row("User ID", user_id, false),
                setting_row("Device Name", device_name, false),
                setting_row("Room Prefix", room_prefix, false),
                setting_row("Allowed Users", allowed_users, false),
                setting_row("Auth Method", auth_method, false),
            ],
        );

        // Backend section
        let backend_section = section_card(
            "🤖",
            "Agent Backend",
            vec![
                setting_row("Type", backend_type, false),
                setting_row("Binary", binary, false),
                setting_row("Model", model, false),
                setting_row("Timeout", timeout, false),
                setting_row("Keep-alive", keep_alive, false),
                setting_row("Pre-warm", pre_warm, false),
                setting_row("MCP Servers", mcp_servers, false),
            ],
        );

        // Webhook section
        let webhook_section = section_card(
            "🌐",
            "Web Admin / Webhook",
            vec![
                setting_row("Host", webhook_host, false),
                setting_row("Port", webhook_port, false),
                setting_row("API Key", api_key_status, true),
            ],
        );

        // Workspace section
        let workspace_section = section_card(
            "📁",
            "Workspace",
            vec![setting_row("Path", workspace_path, false)],
        );

        // Scheduler section
        let scheduler_section = section_card(
            "⏰",
            "Scheduler",
            vec![setting_row("Timezone", timezone, false)],
        );

        // Layout
        let all_content = column![
            header,
            Space::with_height(spacing::LG),
            config_hint(),
            Space::with_height(spacing::LG),
            matrix_section,
            Space::with_height(spacing::MD),
            backend_section,
            Space::with_height(spacing::MD),
            row![
                container(webhook_section).width(Length::FillPortion(1)),
                Space::with_width(spacing::MD),
                column![
                    workspace_section,
                    Space::with_height(spacing::MD),
                    scheduler_section,
                ]
                .width(Length::FillPortion(1)),
            ],
        ]
        .padding(spacing::XL)
        .width(Length::Fill);

        scrollable(all_content).height(Length::Fill).into()
    } else {
        container(
            column![
                text("◌")
                    .size(text_size::DISPLAY)
                    .color(colors::ACCENT_PRIMARY),
                Space::with_height(spacing::MD),
                text("Loading configuration...")
                    .size(text_size::LARGE)
                    .color(colors::TEXT_PRIMARY),
            ]
            .align_x(Alignment::Center),
        )
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .into()
    };

    container(content)
        .style(content_style)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}
