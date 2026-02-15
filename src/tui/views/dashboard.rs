// ABOUTME: TUI Dashboard view showing system overview and platform health
// ABOUTME: Displays version, uptime, platform status, session counts, and recent activity

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Cell, List, ListItem, Paragraph, Row, Table};

use crate::tui::app::TuiApp;
use crate::tui::theme;

/// Render the dashboard view in the given area
pub fn render_dashboard(frame: &mut Frame, area: Rect, app: &TuiApp) {
    // Split into three horizontal sections: header, stats, activity
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5),  // Header with version and uptime
            Constraint::Length(app.platform_statuses.len() as u16 + 4), // Platform status
            Constraint::Min(6),    // Recent activity / stats
        ])
        .split(area);

    render_header(frame, layout[0], app);
    render_platforms(frame, layout[1], app);
    render_stats(frame, layout[2], app);
}

/// Render the header section with gorp branding and uptime
fn render_header(frame: &mut Frame, area: Rect, app: &TuiApp) {
    let uptime = format_uptime(app.uptime_secs);

    let text = vec![
        Line::from(vec![
            Span::styled("gorp ", Style::default().fg(theme::NAV_HEADER).add_modifier(Modifier::BOLD)),
            Span::styled(format!("v{}", env!("CARGO_PKG_VERSION")), Style::default().fg(theme::DIM_TEXT)),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("Uptime: ", Style::default().fg(theme::DIM_TEXT)),
            Span::styled(uptime, Style::default().fg(theme::TEXT_COLOR)),
            Span::raw("  "),
            Span::styled("Messages: ", Style::default().fg(theme::DIM_TEXT)),
            Span::styled(
                format!("{}", app.feed_messages.len()),
                Style::default().fg(theme::TEXT_COLOR),
            ),
        ]),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Dashboard ")
        .border_style(Style::default().fg(theme::BORDER_COLOR));

    let paragraph = Paragraph::new(text).block(block);
    frame.render_widget(paragraph, area);
}

/// Render the platform connection status section
fn render_platforms(frame: &mut Frame, area: Rect, app: &TuiApp) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Platforms ")
        .border_style(Style::default().fg(theme::BORDER_COLOR));

    if app.platform_statuses.is_empty() {
        let empty = Paragraph::new("  No platforms connected")
            .style(Style::default().fg(theme::DIM_TEXT))
            .block(block);
        frame.render_widget(empty, area);
        return;
    }

    let rows: Vec<Row> = app
        .platform_statuses
        .iter()
        .map(|status| {
            let indicator = if status.connected { "  ●" } else { "  ○" };
            let indicator_color = if status.connected {
                theme::CONNECTED_COLOR
            } else {
                theme::DISCONNECTED_COLOR
            };
            let platform_color = theme::platform_color(&status.name);
            let state_text = if status.connected {
                "Connected"
            } else {
                "Disconnected"
            };

            Row::new(vec![
                Cell::from(Span::styled(indicator, Style::default().fg(indicator_color))),
                Cell::from(Span::styled(
                    status.name.clone(),
                    Style::default().fg(platform_color).add_modifier(Modifier::BOLD),
                )),
                Cell::from(Span::styled(
                    state_text,
                    Style::default().fg(if status.connected {
                        theme::DIM_TEXT
                    } else {
                        theme::DISCONNECTED_COLOR
                    }),
                )),
            ])
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Length(4),
            Constraint::Length(12),
            Constraint::Min(12),
        ],
    )
    .block(block);

    frame.render_widget(table, area);
}

/// Render the stats and recent activity section
fn render_stats(frame: &mut Frame, area: Rect, app: &TuiApp) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Recent Activity ")
        .border_style(Style::default().fg(theme::BORDER_COLOR));

    if app.feed_messages.is_empty() {
        let empty = Paragraph::new("  No messages yet. Waiting for platform events...")
            .style(Style::default().fg(theme::DIM_TEXT))
            .block(block);
        frame.render_widget(empty, area);
        return;
    }

    // Show last N messages as recent activity
    let max_items = (area.height.saturating_sub(2)) as usize;
    let items: Vec<ListItem> = app
        .feed_messages
        .iter()
        .rev()
        .take(max_items)
        .map(|msg| {
            let platform_color = theme::platform_color(&msg.platform_id);
            let line = Line::from(vec![
                Span::styled(
                    format!("[{}] ", msg.platform_id),
                    Style::default().fg(platform_color),
                ),
                Span::styled(
                    format!("{}: ", msg.sender),
                    Style::default().fg(theme::TEXT_COLOR).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    truncate_str(&msg.body, 60),
                    Style::default().fg(theme::TEXT_COLOR),
                ),
            ]);
            ListItem::new(line)
        })
        .collect();

    let list = List::new(items).block(block);
    frame.render_widget(list, area);
}

/// Format seconds into a human-readable uptime string
fn format_uptime(secs: u64) -> String {
    if secs < 60 {
        format!("{}s", secs)
    } else if secs < 3600 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else if secs < 86400 {
        format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
    } else {
        format!("{}d {}h", secs / 86400, (secs % 86400) / 3600)
    }
}

/// Truncate a string to a maximum length, adding "..." if truncated
fn truncate_str(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len.saturating_sub(3)])
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_uptime_seconds() {
        assert_eq!(format_uptime(0), "0s");
        assert_eq!(format_uptime(45), "45s");
    }

    #[test]
    fn test_format_uptime_minutes() {
        assert_eq!(format_uptime(60), "1m 0s");
        assert_eq!(format_uptime(125), "2m 5s");
    }

    #[test]
    fn test_format_uptime_hours() {
        assert_eq!(format_uptime(3600), "1h 0m");
        assert_eq!(format_uptime(7320), "2h 2m");
    }

    #[test]
    fn test_format_uptime_days() {
        assert_eq!(format_uptime(86400), "1d 0h");
        assert_eq!(format_uptime(90000), "1d 1h");
    }

    #[test]
    fn test_truncate_str_short() {
        assert_eq!(truncate_str("hello", 10), "hello");
    }

    #[test]
    fn test_truncate_str_exact() {
        assert_eq!(truncate_str("hello", 5), "hello");
    }

    #[test]
    fn test_truncate_str_long() {
        assert_eq!(truncate_str("hello world, this is a long message", 15), "hello world,...");
    }

    #[test]
    fn test_truncate_str_empty() {
        assert_eq!(truncate_str("", 10), "");
    }
}
