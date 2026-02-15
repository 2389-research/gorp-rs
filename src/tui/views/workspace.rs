// ABOUTME: TUI Workspace view for direct Claude interaction
// ABOUTME: Workspace selection, conversation display, and input handling with streaming support

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Wrap};

use crate::tui::app::TuiApp;
use crate::tui::theme;

/// Render the workspace view in the given area
pub fn render_workspace(frame: &mut Frame, area: Rect, app: &TuiApp) {
    // Split into workspace sidebar, conversation, and input
    let layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(if app.workspace_sidebar_open { 20 } else { 0 }),
            Constraint::Min(30),
        ])
        .split(area);

    if app.workspace_sidebar_open {
        render_workspace_list(frame, layout[0], app);
    }

    // Split main area into conversation and input
    let main_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(3),
            Constraint::Length(if app.input_mode { 3 } else { 1 }),
        ])
        .split(layout[1]);

    render_conversation(frame, main_layout[0], app);
    render_input(frame, main_layout[1], app);
}

/// Render the workspace list sidebar
fn render_workspace_list(frame: &mut Frame, area: Rect, app: &TuiApp) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Workspaces ")
        .border_style(Style::default().fg(theme::BORDER_COLOR));

    if app.workspaces.is_empty() {
        let empty = Paragraph::new(" No workspaces")
            .style(Style::default().fg(theme::DIM_TEXT))
            .block(block);
        frame.render_widget(empty, area);
        return;
    }

    let items: Vec<ListItem> = app
        .workspaces
        .iter()
        .enumerate()
        .map(|(i, ws)| {
            let prefix = if i == app.workspace_selected { ">" } else { " " };
            let active = if ws.active { "*" } else { " " };
            let text = format!("{}{} {}", prefix, active, ws.name);

            let style = if i == app.workspace_selected {
                Style::default()
                    .fg(theme::SELECTED_FG)
                    .bg(theme::SELECTED_BG)
                    .add_modifier(Modifier::BOLD)
            } else if ws.active {
                Style::default()
                    .fg(theme::CONNECTED_COLOR)
            } else {
                Style::default().fg(theme::TEXT_COLOR)
            };

            ListItem::new(text).style(style)
        })
        .collect();

    let list = List::new(items).block(block);
    frame.render_widget(list, area);
}

/// Render the conversation area
fn render_conversation(frame: &mut Frame, area: Rect, app: &TuiApp) {
    let workspace_name = app.active_workspace_name();

    let title = match workspace_name {
        Some(name) => format!(" {} ", name),
        None => " Workspace — select a workspace to begin ".to_string(),
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(Style::default().fg(theme::BORDER_COLOR));

    if app.conversation_messages.is_empty() {
        let hint = if workspace_name.is_some() {
            "  Press i to enter input mode and start chatting with Claude."
        } else {
            "  Press Tab to open workspace list, then select a workspace."
        };

        let paragraph = Paragraph::new(hint)
            .style(Style::default().fg(theme::DIM_TEXT))
            .block(block);
        frame.render_widget(paragraph, area);
        return;
    }

    // Render conversation messages
    let visible_height = area.height.saturating_sub(2) as usize;
    let total = app.conversation_messages.len();
    let scroll = app
        .conversation_scroll
        .min(total.saturating_sub(visible_height));

    let items: Vec<ListItem> = app
        .conversation_messages
        .iter()
        .skip(scroll)
        .take(visible_height)
        .map(|msg| {
            let (role_style, role_label) = match msg.role.as_str() {
                "user" => (
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                    "You",
                ),
                "assistant" => (
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                    "Claude",
                ),
                _ => (
                    Style::default().fg(theme::DIM_TEXT),
                    msg.role.as_str(),
                ),
            };

            let line = Line::from(vec![
                Span::styled(format!("{}: ", role_label), role_style),
                Span::styled(&msg.content, Style::default().fg(theme::TEXT_COLOR)),
            ]);

            ListItem::new(line)
        })
        .collect();

    let list = List::new(items).block(block);
    frame.render_widget(list, area);
}

/// Render the input area
fn render_input(frame: &mut Frame, area: Rect, app: &TuiApp) {
    if app.input_mode {
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Input (Esc to cancel, Enter to send) ")
            .border_style(Style::default().fg(theme::NAV_HEADER));

        // Show cursor in input
        let input_text = format!("{}\u{2588}", app.input_buffer); // Block cursor
        let paragraph = Paragraph::new(input_text)
            .style(Style::default().fg(theme::TEXT_COLOR))
            .block(block)
            .wrap(Wrap { trim: false });

        frame.render_widget(paragraph, area);
    } else {
        let hint = if app.is_streaming {
            " Streaming... (Ctrl+C to cancel)"
        } else {
            " i: input mode | Tab: workspaces | PgUp/PgDn: scroll"
        };

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
    use crate::tui::app::ConversationMessage;

    #[test]
    fn test_workspace_view_requires_app() {
        // Verify the render function signature compiles
        let _f: fn(&mut Frame, Rect, &TuiApp) = render_workspace;
    }

    #[test]
    fn test_conversation_message_roles() {
        let user_msg = ConversationMessage {
            role: "user".to_string(),
            content: "Hello".to_string(),
        };
        assert_eq!(user_msg.role, "user");

        let assistant_msg = ConversationMessage {
            role: "assistant".to_string(),
            content: "Hi there!".to_string(),
        };
        assert_eq!(assistant_msg.role, "assistant");
    }
}
