// ABOUTME: TUI Logs view with level filtering and scrollable log entries
// ABOUTME: Displays timestamped log messages color-coded by level with workspace filtering

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};

use crate::tui::app::TuiApp;
use crate::tui::theme;

/// Render the logs view in the given area
pub fn render_logs(frame: &mut Frame, area: Rect, app: &TuiApp) {
    // Split into log list and filter bar
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(1)])
        .split(area);

    render_log_list(frame, layout[0], app);
    render_filter_bar(frame, layout[1], app);
}

/// Render the scrollable log list with level and workspace filtering
fn render_log_list(frame: &mut Frame, area: Rect, app: &TuiApp) {
    // Apply filters
    let filtered: Vec<_> = app
        .log_entries
        .iter()
        .filter(|entry| level_passes_filter(&entry.level, &app.log_level_filter))
        .filter(|entry| {
            app.log_workspace_filter
                .as_ref()
                .map_or(true, |f| entry.target.contains(f.as_str()))
        })
        .collect();

    let total = filtered.len();
    let visible_height = area.height.saturating_sub(2) as usize;
    let scroll = app.log_scroll.min(total.saturating_sub(visible_height));

    let title = if total > visible_height {
        format!(
            " Logs [level: {}] [{}/{}] ",
            app.log_level_filter, scroll + 1, total
        )
    } else {
        format!(" Logs [level: {}] [{}] ", app.log_level_filter, total)
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(Style::default().fg(theme::BORDER_COLOR));

    if filtered.is_empty() {
        let hint = if app.log_entries.is_empty() {
            "  Waiting for log output..."
        } else {
            "  No logs match the current filter."
        };
        let empty = Paragraph::new(hint)
            .style(Style::default().fg(theme::DIM_TEXT))
            .block(block);
        frame.render_widget(empty, area);
        return;
    }

    let items: Vec<ListItem> = filtered
        .iter()
        .skip(scroll)
        .take(visible_height)
        .map(|entry| {
            let level_color = level_color(&entry.level);
            let level_padded = format!("{:<5}", entry.level);

            let line = Line::from(vec![
                Span::styled(
                    format!("{} ", entry.timestamp),
                    Style::default().fg(theme::DIM_TEXT),
                ),
                Span::styled(
                    format!("{} ", level_padded),
                    Style::default()
                        .fg(level_color)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("{:<10} ", truncate_target(&entry.target, 10)),
                    Style::default().fg(theme::DIM_TEXT),
                ),
                Span::styled(
                    truncate_message(&entry.message, 80),
                    Style::default().fg(theme::TEXT_COLOR),
                ),
            ]);

            ListItem::new(line)
        })
        .collect();

    let list = List::new(items).block(block);
    frame.render_widget(list, area);
}

/// Render the filter bar at the bottom
fn render_filter_bar(frame: &mut Frame, area: Rect, app: &TuiApp) {
    let ws_filter = match &app.log_workspace_filter {
        Some(ws) => format!(" ws:{} |", ws),
        None => String::new(),
    };

    let text = format!(
        " [1]ERROR [2]WARN [3]INFO [4]DEBUG | current: {}{} j/k: scroll | f: filter ws",
        app.log_level_filter, ws_filter,
    );

    let bar = Paragraph::new(text).style(
        Style::default()
            .fg(Color::White)
            .bg(theme::STATUS_BAR_BG),
    );
    frame.render_widget(bar, area);
}

/// Check if a log level passes the current filter
fn level_passes_filter(entry_level: &str, filter_level: &str) -> bool {
    let entry_rank = level_rank(entry_level);
    let filter_rank = level_rank(filter_level);
    entry_rank <= filter_rank
}

/// Numeric rank for log levels (lower = more severe)
fn level_rank(level: &str) -> u8 {
    match level.to_uppercase().as_str() {
        "ERROR" => 1,
        "WARN" => 2,
        "INFO" => 3,
        "DEBUG" => 4,
        "TRACE" => 5,
        _ => 3, // default to INFO-level
    }
}

/// Get display color for a log level
fn level_color(level: &str) -> Color {
    match level.to_uppercase().as_str() {
        "ERROR" => Color::Red,
        "WARN" => Color::Yellow,
        "INFO" => Color::Green,
        "DEBUG" => Color::Cyan,
        "TRACE" => Color::DarkGray,
        _ => theme::TEXT_COLOR,
    }
}

/// Truncate a target string (e.g. module path) for column display
fn truncate_target(target: &str, max_len: usize) -> String {
    if target.len() <= max_len {
        target.to_string()
    } else {
        // Show the last segment of the module path
        if let Some(last_sep) = target.rfind("::") {
            let suffix = &target[last_sep + 2..];
            if suffix.len() <= max_len {
                return suffix.to_string();
            }
        }
        format!("{}...", &target[..max_len.saturating_sub(3)])
    }
}

/// Truncate a log message for single-line display
fn truncate_message(msg: &str, max_len: usize) -> String {
    // Collapse newlines into spaces
    let collapsed: String = msg
        .chars()
        .map(|c| if c == '\n' || c == '\r' { ' ' } else { c })
        .collect();

    if collapsed.len() <= max_len {
        collapsed
    } else {
        format!("{}...", &collapsed[..max_len.saturating_sub(3)])
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_level_rank_ordering() {
        assert!(level_rank("ERROR") < level_rank("WARN"));
        assert!(level_rank("WARN") < level_rank("INFO"));
        assert!(level_rank("INFO") < level_rank("DEBUG"));
        assert!(level_rank("DEBUG") < level_rank("TRACE"));
    }

    #[test]
    fn test_level_passes_filter_error_only() {
        assert!(level_passes_filter("ERROR", "ERROR"));
        assert!(!level_passes_filter("WARN", "ERROR"));
        assert!(!level_passes_filter("INFO", "ERROR"));
    }

    #[test]
    fn test_level_passes_filter_info() {
        assert!(level_passes_filter("ERROR", "INFO"));
        assert!(level_passes_filter("WARN", "INFO"));
        assert!(level_passes_filter("INFO", "INFO"));
        assert!(!level_passes_filter("DEBUG", "INFO"));
    }

    #[test]
    fn test_level_passes_filter_debug() {
        assert!(level_passes_filter("ERROR", "DEBUG"));
        assert!(level_passes_filter("WARN", "DEBUG"));
        assert!(level_passes_filter("INFO", "DEBUG"));
        assert!(level_passes_filter("DEBUG", "DEBUG"));
        assert!(!level_passes_filter("TRACE", "DEBUG"));
    }

    #[test]
    fn test_level_passes_filter_case_insensitive() {
        assert!(level_passes_filter("error", "INFO"));
        assert!(level_passes_filter("Error", "info"));
    }

    #[test]
    fn test_level_color() {
        assert_eq!(level_color("ERROR"), Color::Red);
        assert_eq!(level_color("WARN"), Color::Yellow);
        assert_eq!(level_color("INFO"), Color::Green);
        assert_eq!(level_color("DEBUG"), Color::Cyan);
        assert_eq!(level_color("TRACE"), Color::DarkGray);
    }

    #[test]
    fn test_truncate_target_short() {
        assert_eq!(truncate_target("gorp", 10), "gorp");
    }

    #[test]
    fn test_truncate_target_module_path() {
        assert_eq!(
            truncate_target("gorp::platform::matrix", 10),
            "matrix"
        );
    }

    #[test]
    fn test_truncate_target_long_segment() {
        assert_eq!(
            truncate_target("very_long_module_name", 10),
            "very_lo..."
        );
    }

    #[test]
    fn test_truncate_message_short() {
        assert_eq!(truncate_message("hello", 80), "hello");
    }

    #[test]
    fn test_truncate_message_newlines() {
        assert_eq!(
            truncate_message("line1\nline2\nline3", 80),
            "line1 line2 line3"
        );
    }

    #[test]
    fn test_truncate_message_long() {
        let long = "x".repeat(100);
        let result = truncate_message(&long, 20);
        assert!(result.len() <= 20);
        assert!(result.ends_with("..."));
    }
}
