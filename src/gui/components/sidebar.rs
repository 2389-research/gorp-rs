// ABOUTME: Sidebar navigation component with refined styling
// ABOUTME: Shows branding, nav items, room list with unread badges, and connection status

use crate::gui::app::Message;
use crate::gui::theme::{
    self, colors, radius, spacing, text_size, button_nav, button_nav_active,
    button_room, button_room_active, sidebar_style,
};
use crate::gui::views::View;
use crate::server::RoomInfo;
use iced::widget::{button, column, container, horizontal_rule, row, scrollable, text, Column, Space};
use iced::{Alignment, Border, Element, Length, Padding};

/// Section label for sidebar groups
fn section_label<'a>(label: &'a str) -> Element<'a, Message> {
    container(
        text(label)
            .size(text_size::CAPTION)
            .color(colors::TEXT_TERTIARY),
    )
    .padding(Padding {
        top: spacing::SM,
        right: spacing::MD,
        bottom: spacing::XS,
        left: spacing::MD,
    })
    .into()
}

/// Navigation button with icon placeholder
fn nav_button<'a>(
    label: &'a str,
    icon: &'a str,
    target: View,
    current: &View,
) -> Element<'a, Message> {
    let is_active = match (current, &target) {
        (View::Dashboard, View::Dashboard) => true,
        (View::Settings, View::Settings) => true,
        (View::Schedules, View::Schedules) => true,
        (View::Logs, View::Logs) => true,
        _ => false,
    };

    let style = if is_active {
        button_nav_active
    } else {
        button_nav
    };

    let text_color = if is_active {
        colors::ACCENT_PRIMARY
    } else {
        colors::TEXT_SECONDARY
    };

    button(
        row![
            text(icon).size(text_size::LARGE).color(text_color),
            Space::with_width(spacing::SM),
            text(label).size(text_size::BODY).color(text_color),
        ]
        .align_y(Alignment::Center),
    )
    .on_press(Message::Navigate(target))
    .style(style)
    .width(Length::Fill)
    .padding([spacing::SM, spacing::MD])
    .into()
}

/// Room item with name, unread badge, and direct message indicator
fn room_item<'a>(room: &'a RoomInfo, is_active: bool) -> Element<'a, Message> {
    let style = if is_active {
        button_room_active
    } else {
        button_room
    };

    let text_color = if is_active {
        colors::TEXT_PRIMARY
    } else {
        colors::TEXT_SECONDARY
    };

    // Truncate long names (UTF-8 safe)
    let display_name = if room.name.chars().count() > 22 {
        format!("{}...", room.name.chars().take(19).collect::<String>())
    } else {
        room.name.clone()
    };

    // Room type indicator
    let type_indicator = if room.is_direct {
        text("@").size(text_size::BODY).color(colors::ACCENT_PRIMARY) // DM
    } else {
        text("#").size(text_size::BODY).color(colors::TEXT_TERTIARY) // Channel
    };

    // Unread badge
    let unread_badge: Element<'a, Message> = if room.unread_count > 0 {
        container(
            text(format!("{}", room.unread_count.min(99)))
                .size(text_size::CAPTION)
                .color(colors::TEXT_INVERSE),
        )
        .padding([spacing::XXXS, spacing::XS])
        .style(theme::badge_style)
        .into()
    } else {
        Space::with_width(0).into()
    };

    button(
        row![
            type_indicator,
            Space::with_width(spacing::XS),
            text(display_name).size(text_size::BODY).color(text_color),
            Space::with_width(Length::Fill),
            unread_badge,
        ]
        .align_y(Alignment::Center)
        .width(Length::Fill),
    )
    .on_press(Message::Navigate(View::Chat {
        room_id: room.id.clone(),
    }))
    .style(style)
    .width(Length::Fill)
    .padding([spacing::XS, spacing::SM])
    .into()
}

/// Divider line
fn divider<'a>() -> Element<'a, Message> {
    container(
        horizontal_rule(1).style(|_theme| iced::widget::rule::Style {
            color: colors::BORDER_SUBTLE,
            width: 1,
            radius: 0.0.into(),
            fill_mode: iced::widget::rule::FillMode::Full,
        }),
    )
    .padding([spacing::SM, spacing::MD])
    .into()
}

pub fn view<'a>(current_view: &View, rooms: &'a [RoomInfo]) -> Element<'a, Message> {
    // Brand header
    let brand = container(
        row![
            container(
                text("G").size(text_size::LARGE).color(colors::TEXT_INVERSE),
            )
            .padding([spacing::XS, spacing::SM])
            .style(|_theme| container::Style {
                background: Some(colors::ACCENT_PRIMARY.into()),
                border: Border {
                    radius: radius::MD.into(),
                    ..Default::default()
                },
                ..Default::default()
            }),
            Space::with_width(spacing::SM),
            column![
                text("gorp").size(text_size::LARGE).color(colors::TEXT_PRIMARY),
                text("Matrix-Claude Bridge")
                    .size(text_size::CAPTION)
                    .color(colors::TEXT_TERTIARY),
            ]
            .spacing(spacing::XXXS),
        ]
        .align_y(Alignment::Center),
    )
    .padding(Padding {
        top: spacing::LG,
        right: spacing::MD,
        bottom: spacing::MD,
        left: spacing::MD,
    });

    // Primary navigation
    let primary_nav = column![
        nav_button("Dashboard", "◉", View::Dashboard, current_view),
        nav_button("Schedules", "◷", View::Schedules, current_view),
        nav_button("Logs", "☰", View::Logs, current_view),
    ]
    .spacing(spacing::XXS)
    .padding([0.0, spacing::XS]);

    // Room list
    let room_items: Vec<Element<'a, Message>> = rooms
        .iter()
        .map(|room| {
            let is_active = matches!(current_view, View::Chat { room_id } if room_id == &room.id);
            room_item(room, is_active)
        })
        .collect();

    let rooms_section: Element<'a, Message> = if rooms.is_empty() {
        container(
            text("No rooms joined yet")
                .size(text_size::SMALL)
                .color(colors::TEXT_TERTIARY),
        )
        .padding([spacing::SM, spacing::MD])
        .into()
    } else {
        Column::with_children(room_items)
            .spacing(spacing::XXXS)
            .padding([0.0, spacing::XS])
            .into()
    };

    let scrollable_rooms = scrollable(rooms_section)
        .height(Length::Fill);

    // Settings at bottom
    let bottom_nav = column![
        divider(),
        container(
            nav_button("Settings", "⚙", View::Settings, current_view),
        )
        .padding([0.0, spacing::XS]),
    ];

    // ROOMS section header with add button
    let rooms_header = container(
        row![
            text("ROOMS")
                .size(text_size::CAPTION)
                .color(colors::TEXT_TERTIARY),
            Space::with_width(Length::Fill),
            button(text("+").size(text_size::BODY).color(colors::TEXT_SECONDARY))
                .on_press(Message::ShowCreateRoom)
                .style(button_nav)
                .padding([spacing::XXXS, spacing::XS]),
        ]
        .align_y(Alignment::Center),
    )
    .padding(Padding {
        top: spacing::SM,
        right: spacing::MD,
        bottom: spacing::XS,
        left: spacing::MD,
    });

    // Assemble sidebar
    let sidebar_content = column![
        brand,
        divider(),
        section_label("NAVIGATION"),
        primary_nav,
        divider(),
        rooms_header,
        scrollable_rooms,
        bottom_nav,
    ]
    .width(Length::Fixed(theme::SIDEBAR_WIDTH));

    container(sidebar_content)
        .style(sidebar_style)
        .height(Length::Fill)
        .into()
}
