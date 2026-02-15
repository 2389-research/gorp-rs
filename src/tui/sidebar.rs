// ABOUTME: TUI sidebar component with navigation and platform status
// ABOUTME: Renders nav items with selection highlight and connection indicators

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};

use super::app::TuiApp;
use super::theme;

/// Render the sidebar containing navigation and platform status
pub fn render_sidebar(frame: &mut Frame, area: Rect, app: &TuiApp) {
    // Split sidebar into nav section and platform status section
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(10), Constraint::Length(app.platform_statuses.len() as u16 + 3)])
        .split(area);

    render_navigation(frame, layout[0], app);
    render_platform_status(frame, layout[1], app);
}

/// Render the navigation list
fn render_navigation(frame: &mut Frame, area: Rect, app: &TuiApp) {
    let nav_items: Vec<ListItem> = TuiApp::nav_items()
        .iter()
        .enumerate()
        .map(|(i, label)| {
            let prefix = if i == app.nav_selected { ">" } else { " " };
            let shortcut = label.chars().next().unwrap_or(' ');
            let text = format!("{} [{}] {}", prefix, shortcut, label);

            let style = if i == app.nav_selected {
                Style::default()
                    .fg(theme::SELECTED_FG)
                    .bg(theme::SELECTED_BG)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme::TEXT_COLOR)
            };

            ListItem::new(text).style(style)
        })
        .collect();

    let nav_block = Block::default()
        .borders(Borders::ALL)
        .title(" gorp ")
        .title_style(Style::default().fg(theme::NAV_HEADER).add_modifier(Modifier::BOLD))
        .border_style(Style::default().fg(theme::BORDER_COLOR));

    let nav_list = List::new(nav_items).block(nav_block);

    frame.render_widget(nav_list, area);
}

/// Render the platform connection status
fn render_platform_status(frame: &mut Frame, area: Rect, app: &TuiApp) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Platforms ")
        .border_style(Style::default().fg(theme::BORDER_COLOR));

    if app.platform_statuses.is_empty() {
        let empty = Paragraph::new("  No platforms")
            .style(Style::default().fg(theme::DIM_TEXT))
            .block(block);
        frame.render_widget(empty, area);
        return;
    }

    let items: Vec<ListItem> = app
        .platform_statuses
        .iter()
        .map(|status| {
            let indicator = if status.connected { "●" } else { "○" };
            let color = if status.connected {
                theme::CONNECTED_COLOR
            } else {
                theme::DISCONNECTED_COLOR
            };
            let platform_color = theme::platform_color(&status.name);

            let line = Line::from(vec![
                Span::styled(format!("  {} ", indicator), Style::default().fg(color)),
                Span::styled(&status.name, Style::default().fg(platform_color)),
            ]);

            ListItem::new(line)
        })
        .collect();

    let list = List::new(items).block(block);
    frame.render_widget(list, area);
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nav_items_have_unique_shortcuts() {
        let items = TuiApp::nav_items();
        let shortcuts: Vec<char> = items
            .iter()
            .map(|item| item.chars().next().unwrap().to_ascii_lowercase())
            .collect();

        // Check all shortcuts are unique
        let mut unique = shortcuts.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(shortcuts.len(), unique.len(), "Navigation shortcuts must be unique");
    }
}
