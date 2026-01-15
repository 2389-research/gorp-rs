// ABOUTME: Schedules view - list and manage scheduled tasks with full CRUD
// ABOUTME: Displays schedule cards with status badges, actions, and create form modal

use crate::gui::app::Message;
use crate::gui::components::common;
use crate::gui::theme::{
    button_primary, button_secondary, colors, content_style, modal_style, radius, spacing,
    stat_card_style, text_input_style, text_size,
};
use crate::scheduler::{ScheduleStatus, ScheduledPrompt};
use crate::server::RoomInfo;
use iced::widget::{
    button, column, container, pick_list, row, scrollable, text, text_input, Column, Space,
};
use iced::{Alignment, Border, Element, Length};

/// Status badge for schedule status
fn status_badge<'a>(status: &ScheduleStatus) -> Element<'a, Message> {
    let (bg_color, text_str) = match status {
        ScheduleStatus::Active => (colors::STATUS_ONLINE, "Active"),
        ScheduleStatus::Paused => (colors::STATUS_AWAY, "Paused"),
        ScheduleStatus::Completed => (colors::ACCENT_PRIMARY, "Done"),
        ScheduleStatus::Failed => (colors::ACCENT_DANGER, "Failed"),
        ScheduleStatus::Executing => (colors::ACCENT_WARM, "Running"),
        ScheduleStatus::Cancelled => (colors::TEXT_TERTIARY, "Cancelled"),
    };

    container(
        text(text_str)
            .size(text_size::CAPTION)
            .color(colors::TEXT_INVERSE),
    )
    .padding([spacing::XXXS, spacing::XS])
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

/// Single schedule card
fn schedule_card<'a>(schedule: &'a ScheduledPrompt) -> Element<'a, Message> {
    // Header row: channel name + status badge
    let header = row![
        text(&schedule.channel_name)
            .size(text_size::LARGE)
            .color(colors::TEXT_PRIMARY),
        Space::with_width(Length::Fill),
        status_badge(&schedule.status),
    ]
    .align_y(Alignment::Center)
    .width(Length::Fill);

    // Prompt text (truncated)
    let prompt_display: String = if schedule.prompt.chars().count() > 80 {
        format!(
            "{}...",
            schedule.prompt.chars().take(77).collect::<String>()
        )
    } else {
        schedule.prompt.clone()
    };

    let prompt_text = text(prompt_display)
        .size(text_size::BODY)
        .color(colors::TEXT_SECONDARY);

    // Schedule info row - build the schedule type text
    let schedule_type_text: Element<'a, Message> = if let Some(ref cron) = schedule.cron_expression
    {
        text(format!("Recurring: {}", cron))
            .size(text_size::SMALL)
            .color(colors::TEXT_TERTIARY)
            .into()
    } else if let Some(ref exec_at) = schedule.execute_at {
        text(format!("One-time: {}", exec_at))
            .size(text_size::SMALL)
            .color(colors::TEXT_TERTIARY)
            .into()
    } else {
        text("Unknown schedule")
            .size(text_size::SMALL)
            .color(colors::TEXT_TERTIARY)
            .into()
    };

    let info_row = row![
        text("⏰")
            .size(text_size::SMALL)
            .color(colors::TEXT_TERTIARY),
        Space::with_width(spacing::XS),
        schedule_type_text,
        Space::with_width(spacing::MD),
        text("Next:")
            .size(text_size::SMALL)
            .color(colors::TEXT_TERTIARY),
        Space::with_width(spacing::XS),
        text(&schedule.next_execution_at)
            .size(text_size::SMALL)
            .color(colors::TEXT_SECONDARY),
    ]
    .align_y(Alignment::Center);

    // Stats row
    let stats_row = row![
        text(format!("Runs: {}", schedule.execution_count))
            .size(text_size::CAPTION)
            .color(colors::TEXT_TERTIARY),
        Space::with_width(spacing::MD),
        text(format!(
            "ID: {}...",
            schedule.id.chars().take(8).collect::<String>()
        ))
        .size(text_size::CAPTION)
        .color(colors::TEXT_TERTIARY),
    ];

    // Action buttons
    let pause_resume_btn: Element<'a, Message> = match schedule.status {
        ScheduleStatus::Active => {
            let id = schedule.id.clone();
            button(
                text("Pause")
                    .size(text_size::SMALL)
                    .color(colors::TEXT_SECONDARY),
            )
            .on_press(Message::PauseSchedule(id))
            .style(button_secondary)
            .padding([spacing::XS, spacing::SM])
            .into()
        }
        ScheduleStatus::Paused => {
            let id = schedule.id.clone();
            button(text("Resume").size(text_size::SMALL))
                .on_press(Message::ResumeSchedule(id))
                .style(button_primary)
                .padding([spacing::XS, spacing::SM])
                .into()
        }
        _ => Space::with_width(0).into(),
    };

    let delete_id = schedule.id.clone();
    let delete_btn = button(
        text("Delete")
            .size(text_size::SMALL)
            .color(colors::ACCENT_DANGER),
    )
    .on_press(Message::DeleteSchedule(delete_id))
    .style(button_secondary)
    .padding([spacing::XS, spacing::SM]);

    let actions = row![pause_resume_btn, Space::with_width(spacing::XS), delete_btn,]
        .align_y(Alignment::Center);

    // Assemble card
    container(
        column![
            header,
            Space::with_height(spacing::SM),
            prompt_text,
            Space::with_height(spacing::SM),
            info_row,
            Space::with_height(spacing::XS),
            stats_row,
            Space::with_height(spacing::MD),
            actions,
        ]
        .width(Length::Fill),
    )
    .padding(spacing::LG)
    .style(stat_card_style)
    .into()
}

/// Empty state when no schedules
fn empty_state<'a>() -> Element<'a, Message> {
    container(
        column![
            text("⏰").size(48.0),
            Space::with_height(spacing::MD),
            text("No Schedules")
                .size(text_size::LARGE)
                .color(colors::TEXT_PRIMARY),
            Space::with_height(spacing::XS),
            text("Create a scheduled task to run prompts automatically")
                .size(text_size::BODY)
                .color(colors::TEXT_SECONDARY),
            Space::with_height(spacing::LG),
            button(
                row![
                    text("+").size(text_size::BODY),
                    Space::with_width(spacing::XS),
                    text("New Schedule").size(text_size::BODY),
                ]
                .align_y(Alignment::Center),
            )
            .on_press(Message::ShowCreateSchedule)
            .style(button_primary)
            .padding([spacing::SM, spacing::LG]),
        ]
        .align_x(Alignment::Center),
    )
    .center_x(Length::Fill)
    .center_y(Length::Fill)
    .into()
}

/// Create schedule form modal
fn create_form_modal<'a>(
    channel: &'a str,
    prompt: &'a str,
    time: &'a str,
    error: Option<&'a str>,
    rooms: &'a [RoomInfo],
) -> Element<'a, Message> {
    // Channel picker (using text input for now - picklist needs owned strings)
    let room_names: Vec<String> = rooms.iter().map(|r| r.name.clone()).collect();
    let selected_room = if channel.is_empty() {
        None
    } else {
        Some(channel.to_string())
    };

    let channel_input = pick_list(room_names, selected_room, |s| {
        Message::ScheduleFormChannelChanged(s)
    })
    .placeholder("Select a room...")
    .width(Length::Fill)
    .padding(spacing::SM);

    let prompt_input = text_input("Enter the prompt to execute...", prompt)
        .on_input(Message::ScheduleFormPromptChanged)
        .padding(spacing::SM)
        .size(text_size::BODY)
        .width(Length::Fill)
        .style(text_input_style);

    let time_input = text_input("e.g., 'every day at 9am', 'in 2 hours'", time)
        .on_input(Message::ScheduleFormTimeChanged)
        .padding(spacing::SM)
        .size(text_size::BODY)
        .width(Length::Fill)
        .style(text_input_style);

    let cancel_btn = button(text("Cancel").size(text_size::BODY))
        .on_press(Message::HideCreateSchedule)
        .style(button_secondary)
        .padding([spacing::SM, spacing::LG]);

    let create_btn = button(text("Create").size(text_size::BODY))
        .on_press(Message::CreateSchedule)
        .style(button_primary)
        .padding([spacing::SM, spacing::LG]);

    // Error message display
    let error_display: Element<'a, Message> = if let Some(err) = error {
        text(err)
            .size(text_size::SMALL)
            .color(colors::ACCENT_DANGER)
            .into()
    } else {
        Space::with_height(0).into()
    };

    let form_content = container(
        column![
            // Header
            text("New Schedule")
                .size(text_size::TITLE)
                .color(colors::TEXT_PRIMARY),
            Space::with_height(spacing::LG),
            // Error message (if any)
            error_display,
            // Channel field
            text("Room")
                .size(text_size::SMALL)
                .color(colors::TEXT_SECONDARY),
            Space::with_height(spacing::XXS),
            channel_input,
            Space::with_height(spacing::MD),
            // Prompt field
            text("Prompt")
                .size(text_size::SMALL)
                .color(colors::TEXT_SECONDARY),
            Space::with_height(spacing::XXS),
            prompt_input,
            Space::with_height(spacing::MD),
            // Time field
            text("Schedule")
                .size(text_size::SMALL)
                .color(colors::TEXT_SECONDARY),
            Space::with_height(spacing::XXS),
            time_input,
            Space::with_height(spacing::XS),
            text("Examples: 'every day at 9am', 'in 30 minutes', 'every monday at 2pm'")
                .size(text_size::CAPTION)
                .color(colors::TEXT_TERTIARY),
            Space::with_height(spacing::LG),
            // Buttons
            row![
                Space::with_width(Length::Fill),
                cancel_btn,
                Space::with_width(spacing::SM),
                create_btn,
            ]
            .align_y(Alignment::Center),
        ]
        .padding(spacing::LG)
        .width(Length::Fixed(480.0)),
    )
    .style(modal_style);

    common::modal_frame(form_content.into())
}

pub fn view<'a>(
    schedules: &'a [ScheduledPrompt],
    loading: bool,
    show_form: bool,
    form_channel: &'a str,
    form_prompt: &'a str,
    form_time: &'a str,
    form_error: Option<&'a str>,
    rooms: &'a [RoomInfo],
) -> Element<'a, Message> {
    // Header with title and create button
    let header = row![
        column![
            text("Schedules")
                .size(text_size::HEADING)
                .color(colors::TEXT_PRIMARY),
            Space::with_height(spacing::XXS),
            text(format!("{} scheduled tasks", schedules.len()))
                .size(text_size::BODY)
                .color(colors::TEXT_SECONDARY),
        ],
        Space::with_width(Length::Fill),
        button(
            row![
                text("+").size(text_size::BODY),
                Space::with_width(spacing::XS),
                text("New Schedule").size(text_size::BODY),
            ]
            .align_y(Alignment::Center),
        )
        .on_press(Message::ShowCreateSchedule)
        .style(button_primary)
        .padding([spacing::SM, spacing::LG]),
    ]
    .align_y(Alignment::Center);

    // Main content
    let main_content: Element<'a, Message> = if loading {
        common::loading_state("Loading schedules...")
    } else if schedules.is_empty() {
        empty_state()
    } else {
        let cards: Vec<Element<'a, Message>> = schedules.iter().map(|s| schedule_card(s)).collect();

        scrollable(
            Column::with_children(cards)
                .spacing(spacing::MD)
                .width(Length::Fill),
        )
        .height(Length::Fill)
        .into()
    };

    let content = column![header, Space::with_height(spacing::LG), main_content,]
        .padding(spacing::XL)
        .width(Length::Fill)
        .height(Length::Fill);

    let base = container(content)
        .style(content_style)
        .width(Length::Fill)
        .height(Length::Fill);

    // Overlay modal if showing form
    if show_form {
        let modal = create_form_modal(form_channel, form_prompt, form_time, form_error, rooms);
        iced::widget::stack![base, modal].into()
    } else {
        base.into()
    }
}
