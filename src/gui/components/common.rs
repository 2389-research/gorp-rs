// ABOUTME: Shared UI components used across multiple views
// ABOUTME: Loading states, empty states, and other reusable elements

use crate::gui::app::Message;
use crate::gui::theme::{backdrop_style, colors, spacing, text_size};
use iced::widget::{column, container, text, Space};
use iced::{Alignment, Element, Length};

/// Centered loading spinner with message
pub fn loading_state<'a>(message: &'a str) -> Element<'a, Message> {
    container(
        column![
            text("◌").size(48.0).color(colors::ACCENT_PRIMARY),
            Space::with_height(spacing::MD),
            text(message)
                .size(text_size::LARGE)
                .color(colors::TEXT_PRIMARY),
        ]
        .align_x(Alignment::Center),
    )
    .center_x(Length::Fill)
    .center_y(Length::Fill)
    .into()
}

/// Wraps modal content with semi-transparent backdrop and centers it on screen.
pub fn modal_frame<'a>(modal_content: Element<'a, Message>) -> Element<'a, Message> {
    let backdrop = container(Space::new(0, 0))
        .width(Length::Fill)
        .height(Length::Fill)
        .style(backdrop_style);

    let centered = container(modal_content)
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill);

    iced::widget::stack![backdrop, centered].into()
}
