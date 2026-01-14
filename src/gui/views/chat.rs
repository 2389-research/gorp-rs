// ABOUTME: Chat view - displays messages in a Matrix room with refined styling
// ABOUTME: Message bubbles with visual distinction for own/other messages, typing indicators

use crate::gui::app::Message;
use crate::gui::components::common;
use crate::gui::theme::{
    self, colors, radius, spacing, text_size, button_primary,
    content_style, header_style, message_other_style, message_own_style, text_input_style,
};
use iced::widget::{button, column, container, row, scrollable, text, text_input, Column, Space};
use iced::{Alignment, Border, Element, Length};

/// A message to display in the chat
#[derive(Debug, Clone)]
pub struct ChatMessage {
    pub sender: String,
    pub content: String,
    pub timestamp: String,
    pub is_own: bool,
    /// Used for deduplication - content hash or event ID
    pub dedup_key: Option<String>,
}

/// Single message bubble
fn message_bubble<'a>(msg: &'a ChatMessage) -> Element<'a, Message> {
    let style = if msg.is_own {
        message_own_style
    } else {
        message_other_style
    };

    let sender_color = if msg.is_own {
        colors::ACCENT_PRIMARY
    } else {
        colors::ACCENT_WARM
    };

    // Message header with sender and time
    let header = row![
        text(&msg.sender).size(text_size::SMALL).color(sender_color),
        Space::with_width(Length::Fill),
        text(&msg.timestamp)
            .size(text_size::CAPTION)
            .color(colors::TEXT_TERTIARY),
    ]
    .width(Length::Fill);

    // Message content
    let content = text(&msg.content)
        .size(text_size::BODY)
        .color(colors::TEXT_PRIMARY);

    let bubble = container(
        column![header, Space::with_height(spacing::XXS), content,]
            .width(Length::Fill),
    )
    .padding([spacing::SM, spacing::MD])
    .max_width(theme::MESSAGE_MAX_WIDTH)
    .style(style);

    // Align own messages to the right
    if msg.is_own {
        row![Space::with_width(Length::FillPortion(1)), bubble,]
            .width(Length::Fill)
            .into()
    } else {
        row![bubble, Space::with_width(Length::FillPortion(1)),]
            .width(Length::Fill)
            .into()
    }
}

/// Typing indicator component
fn typing_indicator<'a>(typing_users: &'a [String]) -> Element<'a, Message> {
    if typing_users.is_empty() {
        return Space::with_height(spacing::SM).into();
    }

    let typing_text = if typing_users.len() == 1 {
        format!("{} is typing", typing_users[0])
    } else if typing_users.len() == 2 {
        format!("{} and {} are typing", typing_users[0], typing_users[1])
    } else {
        format!("{} people are typing", typing_users.len())
    };

    container(
        row![
            // Animated dots (static for now - could animate with subscription)
            text("•••").size(text_size::BODY).color(colors::ACCENT_PRIMARY),
            Space::with_width(spacing::XS),
            text(typing_text)
                .size(text_size::SMALL)
                .color(colors::TEXT_TERTIARY),
        ]
        .align_y(Alignment::Center),
    )
    .padding([spacing::XS, spacing::MD])
    .into()
}

/// Empty state when no messages
fn empty_state<'a>() -> Element<'a, Message> {
    container(
        column![
            text("💬").size(48.0),
            Space::with_height(spacing::MD),
            text("No messages yet")
                .size(text_size::LARGE)
                .color(colors::TEXT_PRIMARY),
            Space::with_height(spacing::XS),
            text("Start the conversation by sending a message below")
                .size(text_size::BODY)
                .color(colors::TEXT_SECONDARY),
        ]
        .align_x(Alignment::Center),
    )
    .center_x(Length::Fill)
    .center_y(Length::Fill)
    .into()
}

/// Scrollable ID for auto-scroll
pub fn chat_scroll_id() -> scrollable::Id {
    scrollable::Id::new("chat-messages")
}

pub fn view<'a>(
    room_id: &'a str,
    room_name: &'a str,
    messages: &'a [ChatMessage],
    input_text: &'a str,
    typing_users: &'a [String],
    loading: bool,
    connected: bool,
) -> Element<'a, Message> {
    // Connection status indicator
    let (status_dot_color, status_text) = if connected {
        (colors::STATUS_ONLINE, "Connected")
    } else {
        (colors::STATUS_AWAY, "Syncing...")
    };

    let connection_badge = container(
        row![
            container(Space::with_width(6).height(6))
                .style(move |_theme| container::Style {
                    background: Some(status_dot_color.into()),
                    border: Border {
                        radius: radius::FULL.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                }),
            Space::with_width(spacing::XS),
            text(status_text)
                .size(text_size::CAPTION)
                .color(colors::TEXT_TERTIARY),
        ]
        .align_y(Alignment::Center),
    )
    .padding([spacing::XXS, spacing::XS])
    .style(|_theme| container::Style {
        background: Some(colors::BG_ELEVATED.into()),
        border: Border {
            radius: radius::SM.into(),
            ..Default::default()
        },
        ..Default::default()
    });

    // Header with room info
    let header = container(
        row![
            column![
                text(room_name)
                    .size(text_size::TITLE)
                    .color(colors::TEXT_PRIMARY),
                text(format!("{} messages", messages.len()))
                    .size(text_size::CAPTION)
                    .color(colors::TEXT_TERTIARY),
            ]
            .spacing(spacing::XXXS),
            Space::with_width(Length::Fill),
            connection_badge,
            Space::with_width(spacing::SM),
            // Room ID badge (truncated)
            container(
                text(format!(
                    "{}...",
                    room_id.chars().take(20).collect::<String>()
                ))
                .size(text_size::CAPTION)
                .color(colors::TEXT_TERTIARY),
            )
            .padding([spacing::XXS, spacing::XS])
            .style(|_theme| container::Style {
                background: Some(colors::BG_ELEVATED.into()),
                border: Border {
                    radius: radius::SM.into(),
                    ..Default::default()
                },
                ..Default::default()
            }),
        ]
        .align_y(Alignment::Center)
        .padding([spacing::MD, spacing::LG]),
    )
    .style(header_style);

    // Message list
    let messages_view: Element<'a, Message> = if loading {
        common::loading_state("Loading messages...")
    } else if messages.is_empty() {
        empty_state()
    } else {
        let message_items: Vec<Element<'a, Message>> = messages
            .iter()
            .map(|msg| message_bubble(msg))
            .collect();

        scrollable(
            Column::with_children(message_items)
                .spacing(spacing::SM)
                .padding([spacing::MD, spacing::LG]),
        )
        .id(chat_scroll_id())
        .height(Length::Fill)
        .into()
    };

    // Typing indicator
    let typing = typing_indicator(typing_users);

    // Input area
    let input = text_input("Type a message...", input_text)
        .on_input(Message::ChatInputChanged)
        .on_submit(Message::SendMessage {
            room_id: room_id.to_string(),
        })
        .padding(spacing::SM)
        .size(text_size::BODY)
        .width(Length::Fill)
        .style(text_input_style);

    let send_btn = button(
        row![
            text("Send").size(text_size::BODY),
            Space::with_width(spacing::XS),
            text("↑").size(text_size::BODY),
        ]
        .align_y(Alignment::Center),
    )
    .on_press(Message::SendMessage {
        room_id: room_id.to_string(),
    })
    .style(button_primary)
    .padding([spacing::SM, spacing::LG]);

    let input_area = container(
        row![input, Space::with_width(spacing::SM), send_btn,]
            .align_y(Alignment::Center)
            .padding([spacing::SM, spacing::LG]),
    )
    .style(|_theme| container::Style {
        background: Some(colors::BG_SURFACE.into()),
        border: Border {
            color: colors::BORDER_SUBTLE,
            width: 0.0,
            radius: 0.0.into(),
        },
        ..Default::default()
    });

    // Combine all parts
    let content = column![header, messages_view, typing, input_area,]
        .width(Length::Fill)
        .height(Length::Fill);

    container(content)
        .style(content_style)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}
