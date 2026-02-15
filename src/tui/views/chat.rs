// ABOUTME: TUI Chat view for platform channel interaction
// ABOUTME: Routes messages through platform ChatChannel instead of agent, with conversation display

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Wrap};

use crate::tui::app::TuiApp;
use crate::tui::theme;

/// Render the chat view (platform channel conversation) in the given area
pub fn render_chat(frame: &mut Frame, area: Rect, app: &TuiApp) {
    // Split into conversation and input
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(3),
            Constraint::Length(if app.input_mode { 3 } else { 1 }),
        ])
        .split(area);

    render_chat_messages(frame, layout[0], app);
    render_chat_input(frame, layout[1], app);
}

/// Render the chat message history
fn render_chat_messages(frame: &mut Frame, area: Rect, app: &TuiApp) {
    let title = match &app.chat_channel_name {
        Some(name) => format!(" #{} ", name),
        None => " Chat — select a channel from Feed ".to_string(),
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(Style::default().fg(theme::BORDER_COLOR));

    if app.chat_messages.is_empty() {
        let hint = if app.chat_channel_name.is_some() {
            "  No messages yet. Press i to start typing."
        } else {
            "  Select a message in Feed and press Enter to open its channel."
        };
        let paragraph = Paragraph::new(hint)
            .style(Style::default().fg(theme::DIM_TEXT))
            .block(block);
        frame.render_widget(paragraph, area);
        return;
    }

    let visible_height = area.height.saturating_sub(2) as usize;
    let total = app.chat_messages.len();
    let scroll = app
        .chat_scroll
        .min(total.saturating_sub(visible_height));

    let items: Vec<ListItem> = app
        .chat_messages
        .iter()
        .skip(scroll)
        .take(visible_height)
        .map(|msg| {
            let platform_color = theme::platform_color(&msg.platform_id);

            let line = Line::from(vec![
                Span::styled(
                    format!("[{}] ", msg.platform_id),
                    Style::default().fg(platform_color),
                ),
                Span::styled(
                    format!("{}: ", msg.sender),
                    Style::default()
                        .fg(theme::TEXT_COLOR)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    &msg.body,
                    Style::default().fg(theme::TEXT_COLOR),
                ),
            ]);

            ListItem::new(line)
        })
        .collect();

    let list = List::new(items).block(block);
    frame.render_widget(list, area);
}

/// Render the chat input area
fn render_chat_input(frame: &mut Frame, area: Rect, app: &TuiApp) {
    if app.input_mode {
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Send (Esc to cancel, Enter to send) ")
            .border_style(Style::default().fg(theme::NAV_HEADER));

        let input_text = format!("{}\u{2588}", app.input_buffer); // Block cursor
        let paragraph = Paragraph::new(input_text)
            .style(Style::default().fg(theme::TEXT_COLOR))
            .block(block)
            .wrap(Wrap { trim: false });

        frame.render_widget(paragraph, area);
    } else {
        let hint = " i: type message | PgUp/PgDn: scroll | Esc: back to Feed";
        let paragraph = Paragraph::new(hint).style(
            Style::default()
                .fg(Color::White)
                .bg(theme::STATUS_BAR_BG),
        );
        frame.render_widget(paragraph, area);
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chat_render_function_signature() {
        // Verify the render function signature compiles
        let _f: fn(&mut Frame, Rect, &TuiApp) = render_chat;
    }
}
