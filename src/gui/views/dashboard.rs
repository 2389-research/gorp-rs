// ABOUTME: Dashboard view - main home screen with refined stat cards and status
// ABOUTME: Displays connection status, workspace stats, and quick actions

use crate::gui::app::Message;
use crate::gui::theme::{colors, content_style, radius, spacing, stat_card_style, text_size};
use crate::server::ServerState;
use iced::widget::{column, container, row, text, Space};
use iced::{Alignment, Border, Element, Length};
use std::sync::Arc;

/// Stat card component
fn stat_card(
    icon: &'static str,
    value: String,
    label: &'static str,
    accent_color: iced::Color,
) -> Element<'static, Message> {
    container(
        column![
            row![
                text(icon).size(text_size::HEADING).color(accent_color),
                Space::with_width(Length::Fill),
            ],
            Space::with_height(spacing::SM),
            text(value)
                .size(text_size::DISPLAY)
                .color(colors::TEXT_PRIMARY),
            Space::with_height(spacing::XXS),
            text(label)
                .size(text_size::SMALL)
                .color(colors::TEXT_SECONDARY),
        ]
        .width(Length::Fill),
    )
    .padding(spacing::LG)
    .width(Length::FillPortion(1))
    .style(stat_card_style)
    .into()
}

/// Status indicator with dot and text
fn status_row<'a>(status: &'a str, label: &'a str) -> Element<'a, Message> {
    let dot_color = match status {
        "online" | "connected" => colors::STATUS_ONLINE,
        "connecting" | "syncing" => colors::STATUS_AWAY,
        _ => colors::STATUS_OFFLINE,
    };

    row![
        container(Space::with_width(8).height(8)).style(move |_theme| container::Style {
            background: Some(dot_color.into()),
            border: Border {
                radius: radius::FULL.into(),
                ..Default::default()
            },
            ..Default::default()
        }),
        Space::with_width(spacing::XS),
        text(label)
            .size(text_size::BODY)
            .color(colors::TEXT_PRIMARY),
    ]
    .align_y(Alignment::Center)
    .into()
}

/// Info row for the connection details card
fn info_row<'a>(label: &'a str, value: String) -> Element<'a, Message> {
    row![
        text(label)
            .size(text_size::BODY)
            .color(colors::TEXT_SECONDARY)
            .width(Length::Fixed(120.0)),
        text(value)
            .size(text_size::BODY)
            .color(colors::TEXT_PRIMARY),
    ]
    .spacing(spacing::SM)
    .into()
}

pub fn view(server: Option<&Arc<ServerState>>) -> Element<'static, Message> {
    let content: Element<'static, Message> = if let Some(server) = server {
        let user_id = server
            .matrix_client
            .user_id()
            .map(|id| id.to_string())
            .unwrap_or_else(|| "Unknown".to_string());

        let homeserver = server.config.matrix.home_server.clone();
        let device_name = server.config.matrix.device_name.clone();

        // Get counts
        let session_count = server
            .session_store
            .list_all()
            .map(|s| s.len())
            .unwrap_or(0);

        let schedule_count = server
            .scheduler_store
            .list_all()
            .map(|s| s.len())
            .unwrap_or(0);

        let active_schedules = server
            .scheduler_store
            .list_all()
            .map(|s| {
                s.iter()
                    .filter(|sched| {
                        matches!(sched.status, crate::scheduler::ScheduleStatus::Active)
                    })
                    .count()
            })
            .unwrap_or(0);

        // Header section
        let header = column![
            text("Dashboard")
                .size(text_size::HEADING)
                .color(colors::TEXT_PRIMARY),
            Space::with_height(spacing::XXS),
            text("Overview of your gorp instance")
                .size(text_size::BODY)
                .color(colors::TEXT_SECONDARY),
        ];

        // Stats row
        let stats_row = row![
            stat_card(
                "📊",
                session_count.to_string(),
                "Workspace Rooms",
                colors::ACCENT_PRIMARY,
            ),
            Space::with_width(spacing::MD),
            stat_card(
                "⏰",
                schedule_count.to_string(),
                "Scheduled Tasks",
                colors::ACCENT_WARM,
            ),
            Space::with_width(spacing::MD),
            stat_card(
                "▶",
                active_schedules.to_string(),
                "Active Schedules",
                colors::ACCENT_SUCCESS,
            ),
        ];

        // Connection card
        let connection_card = container(
            column![
                row![
                    text("Connection")
                        .size(text_size::LARGE)
                        .color(colors::TEXT_PRIMARY),
                    Space::with_width(Length::Fill),
                    status_row("connected", "Connected"),
                ]
                .align_y(Alignment::Center),
                Space::with_height(spacing::LG),
                info_row("User ID", user_id),
                Space::with_height(spacing::XS),
                info_row("Homeserver", homeserver),
                Space::with_height(spacing::XS),
                info_row("Device", device_name),
            ]
            .width(Length::Fill),
        )
        .padding(spacing::LG)
        .style(stat_card_style);

        // Backend card
        let backend_type = server.config.backend.backend_type.clone();
        let model = server
            .config
            .backend
            .model
            .clone()
            .unwrap_or_else(|| "(default)".to_string());

        let backend_card = container(
            column![
                text("Agent Backend")
                    .size(text_size::LARGE)
                    .color(colors::TEXT_PRIMARY),
                Space::with_height(spacing::LG),
                info_row("Type", backend_type),
                Space::with_height(spacing::XS),
                info_row("Model", model),
            ]
            .width(Length::Fill),
        )
        .padding(spacing::LG)
        .style(stat_card_style);

        // Layout
        column![
            header,
            Space::with_height(spacing::XL),
            stats_row,
            Space::with_height(spacing::LG),
            row![
                container(connection_card).width(Length::FillPortion(1)),
                Space::with_width(spacing::MD),
                container(backend_card).width(Length::FillPortion(1)),
            ],
        ]
        .padding(spacing::XL)
        .width(Length::Fill)
        .into()
    } else {
        // Loading state
        container(
            column![
                text("◌")
                    .size(text_size::DISPLAY)
                    .color(colors::ACCENT_PRIMARY),
                Space::with_height(spacing::MD),
                text("Connecting to Matrix...")
                    .size(text_size::LARGE)
                    .color(colors::TEXT_PRIMARY),
                Space::with_height(spacing::XS),
                text("Establishing secure connection")
                    .size(text_size::BODY)
                    .color(colors::TEXT_SECONDARY),
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
