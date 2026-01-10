// ABOUTME: Logs view - display application logs with level filtering
// ABOUTME: Shows timestamped, color-coded log entries with filter dropdown

use crate::gui::app::{LogEntry, Message};
use crate::gui::theme::{colors, radius, spacing, text_size, button_primary, button_secondary, content_style};
use iced::widget::{button, column, container, pick_list, row, scrollable, text, Column, Space};
use iced::{Alignment, Border, Element, Length};

/// Log level badge with color coding
fn level_badge<'a>(level: &str) -> Element<'a, Message> {
    let bg_color = match level.to_uppercase().as_str() {
        "ERROR" => colors::ACCENT_DANGER,
        "WARN" => colors::ACCENT_WARM,
        "INFO" => colors::STATUS_ONLINE,
        "DEBUG" => colors::ACCENT_PRIMARY,
        "TRACE" => colors::TEXT_TERTIARY,
        _ => colors::TEXT_TERTIARY,
    };

    container(
        text(level.to_uppercase())
            .size(text_size::CAPTION)
            .color(colors::TEXT_INVERSE),
    )
    .padding([spacing::XXXS, spacing::XS])
    .width(Length::Fixed(50.0))
    .style(move |_theme| container::Style {
        background: Some(bg_color.into()),
        border: Border {
            radius: radius::SM.into(),
            ..Default::default()
        },
        ..Default::default()
    })
    .into()
}

/// Single log entry row
fn log_entry_row(entry: &LogEntry) -> Element<'static, Message> {
    // Truncate long targets
    let target_display: String = if entry.target.chars().count() > 30 {
        format!("{}...", entry.target.chars().take(27).collect::<String>())
    } else {
        entry.target.clone()
    };

    // Format timestamp (extract just time portion if full ISO)
    let time_display = if entry.timestamp.len() > 19 {
        // Assume ISO format, extract HH:MM:SS
        entry.timestamp.chars().skip(11).take(8).collect::<String>()
    } else {
        entry.timestamp.clone()
    };

    let level = entry.level.clone();
    let message = entry.message.clone();

    container(
        row![
            // Timestamp
            text(time_display)
                .size(text_size::CAPTION)
                .color(colors::TEXT_TERTIARY)
                .width(Length::Fixed(70.0)),
            Space::with_width(spacing::SM),
            // Level badge
            level_badge(&level),
            Space::with_width(spacing::SM),
            // Target (module path)
            text(target_display)
                .size(text_size::CAPTION)
                .color(colors::TEXT_SECONDARY)
                .width(Length::Fixed(200.0)),
            Space::with_width(spacing::SM),
            // Message
            text(message)
                .size(text_size::SMALL)
                .color(colors::TEXT_PRIMARY),
        ]
        .align_y(Alignment::Center)
        .width(Length::Fill),
    )
    .padding([spacing::XS, spacing::SM])
    .style(|_theme| container::Style {
        background: Some(colors::BG_ELEVATED.into()),
        border: Border {
            radius: radius::SM.into(),
            ..Default::default()
        },
        ..Default::default()
    })
    .into()
}

/// Loading state
fn loading_state<'a>() -> Element<'a, Message> {
    container(
        column![
            text("◌").size(48.0).color(colors::ACCENT_PRIMARY),
            Space::with_height(spacing::MD),
            text("Loading logs...")
                .size(text_size::LARGE)
                .color(colors::TEXT_PRIMARY),
        ]
        .align_x(Alignment::Center),
    )
    .center_x(Length::Fill)
    .center_y(Length::Fill)
    .into()
}

/// Empty state when no logs
fn empty_state<'a>() -> Element<'a, Message> {
    container(
        column![
            text("📋").size(48.0),
            Space::with_height(spacing::MD),
            text("No Logs Found")
                .size(text_size::LARGE)
                .color(colors::TEXT_PRIMARY),
            Space::with_height(spacing::XS),
            text("Logs are stored at ~/.local/share/gorp/logs/")
                .size(text_size::BODY)
                .color(colors::TEXT_SECONDARY),
            Space::with_height(spacing::XS),
            text("No log entries for today yet")
                .size(text_size::SMALL)
                .color(colors::TEXT_TERTIARY),
        ]
        .align_x(Alignment::Center),
    )
    .center_x(Length::Fill)
    .center_y(Length::Fill)
    .into()
}

pub fn view<'a>(
    entries: &'a [LogEntry],
    loading: bool,
    level_filter: Option<&'a str>,
) -> Element<'a, Message> {
    // Filter options
    let filter_options = vec![
        "All".to_string(),
        "ERROR".to_string(),
        "WARN".to_string(),
        "INFO".to_string(),
        "DEBUG".to_string(),
    ];

    let selected_filter = level_filter
        .map(|f| f.to_string())
        .unwrap_or_else(|| "All".to_string());

    let filter_picker = pick_list(
        filter_options,
        Some(selected_filter),
        |s| {
            if s == "All" {
                Message::LogLevelFilterChanged(None)
            } else {
                Message::LogLevelFilterChanged(Some(s))
            }
        },
    )
    .placeholder("Filter by level")
    .padding(spacing::XS);

    let refresh_btn = button(
        row![
            text("↻").size(text_size::BODY),
            Space::with_width(spacing::XS),
            text("Refresh").size(text_size::BODY),
        ]
        .align_y(Alignment::Center),
    )
    .on_press(Message::LoadLogs)
    .style(button_secondary)
    .padding([spacing::XS, spacing::SM]);

    // Header
    let header = row![
        column![
            text("Logs")
                .size(text_size::HEADING)
                .color(colors::TEXT_PRIMARY),
            Space::with_height(spacing::XXS),
            text(format!("{} entries", entries.len()))
                .size(text_size::BODY)
                .color(colors::TEXT_SECONDARY),
        ],
        Space::with_width(Length::Fill),
        filter_picker,
        Space::with_width(spacing::SM),
        refresh_btn,
    ]
    .align_y(Alignment::Center);

    // Filter entries by level
    let filtered_entries: Vec<&LogEntry> = entries
        .iter()
        .filter(|e| {
            level_filter
                .map(|f| e.level.to_uppercase() == f.to_uppercase())
                .unwrap_or(true)
        })
        .collect();

    // Main content
    let main_content: Element<'a, Message> = if loading {
        loading_state()
    } else if filtered_entries.is_empty() {
        if entries.is_empty() {
            empty_state()
        } else {
            // Filtered to empty
            container(
                column![
                    text("🔍").size(48.0),
                    Space::with_height(spacing::MD),
                    text("No Matching Logs")
                        .size(text_size::LARGE)
                        .color(colors::TEXT_PRIMARY),
                    Space::with_height(spacing::XS),
                    text(format!("No {} level logs found", level_filter.unwrap_or("filtered")))
                        .size(text_size::BODY)
                        .color(colors::TEXT_SECONDARY),
                    Space::with_height(spacing::LG),
                    button(text("Clear Filter").size(text_size::BODY))
                        .on_press(Message::LogLevelFilterChanged(None))
                        .style(button_primary)
                        .padding([spacing::SM, spacing::LG]),
                ]
                .align_x(Alignment::Center),
            )
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .into()
        }
    } else {
        let rows: Vec<Element<'a, Message>> = filtered_entries
            .into_iter()
            .map(|e| log_entry_row(e))
            .collect();

        scrollable(
            Column::with_children(rows)
                .spacing(spacing::XXS)
                .width(Length::Fill),
        )
        .height(Length::Fill)
        .into()
    };

    let content = column![
        header,
        Space::with_height(spacing::LG),
        main_content,
    ]
    .padding(spacing::XL)
    .width(Length::Fill)
    .height(Length::Fill);

    container(content)
        .style(content_style)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}
