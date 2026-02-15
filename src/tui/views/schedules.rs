// ABOUTME: TUI Schedules view showing scheduled tasks in a table layout
// ABOUTME: Displays workspace, prompt preview, next run time, and status with row selection

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table};

use crate::tui::app::TuiApp;
use crate::tui::theme;

/// Render the schedules view in the given area
pub fn render_schedules(frame: &mut Frame, area: Rect, app: &TuiApp) {
    // Split into table and action bar
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(1)])
        .split(area);

    render_schedule_table(frame, layout[0], app);
    render_action_bar(frame, layout[1]);
}

/// Render the schedule table with row selection
fn render_schedule_table(frame: &mut Frame, area: Rect, app: &TuiApp) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" Schedules [{}] ", app.schedules.len()))
        .border_style(Style::default().fg(theme::BORDER_COLOR));

    if app.schedules.is_empty() {
        let empty = Paragraph::new("  No schedules configured. Press n to create one.")
            .style(Style::default().fg(theme::DIM_TEXT))
            .block(block);
        frame.render_widget(empty, area);
        return;
    }

    let header = Row::new(vec![
        Cell::from(Span::styled(
            "  Workspace",
            Style::default()
                .fg(theme::NAV_HEADER)
                .add_modifier(Modifier::BOLD),
        )),
        Cell::from(Span::styled(
            "Schedule",
            Style::default()
                .fg(theme::NAV_HEADER)
                .add_modifier(Modifier::BOLD),
        )),
        Cell::from(Span::styled(
            "Next Run",
            Style::default()
                .fg(theme::NAV_HEADER)
                .add_modifier(Modifier::BOLD),
        )),
        Cell::from(Span::styled(
            "Status",
            Style::default()
                .fg(theme::NAV_HEADER)
                .add_modifier(Modifier::BOLD),
        )),
    ])
    .height(1);

    let rows: Vec<Row> = app
        .schedules
        .iter()
        .enumerate()
        .map(|(i, sched)| {
            let is_selected = i == app.schedule_selected;
            let active = sched.status == "Active" || sched.status == "Executing";

            let indicator = if active { "●" } else { "○" };
            let indicator_color = if active {
                theme::CONNECTED_COLOR
            } else {
                theme::DIM_TEXT
            };

            let status_text = format!("{} {}", indicator, short_status(&sched.status));

            let row_style = if is_selected {
                Style::default()
                    .fg(theme::SELECTED_FG)
                    .bg(theme::SELECTED_BG)
            } else {
                Style::default()
            };

            Row::new(vec![
                Cell::from(Span::styled(
                    format!("  {}", sched.channel_name),
                    Style::default().fg(theme::TEXT_COLOR),
                )),
                Cell::from(Span::styled(
                    truncate_prompt(&sched.prompt, 30),
                    Style::default().fg(theme::TEXT_COLOR),
                )),
                Cell::from(Span::styled(
                    format_next_run(&sched.next_run),
                    Style::default().fg(theme::DIM_TEXT),
                )),
                Cell::from(Span::styled(status_text, Style::default().fg(indicator_color))),
            ])
            .style(row_style)
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Length(14),
            Constraint::Min(20),
            Constraint::Length(12),
            Constraint::Length(10),
        ],
    )
    .header(header)
    .block(block);

    frame.render_widget(table, area);
}

/// Render the action bar at the bottom
fn render_action_bar(frame: &mut Frame, area: Rect) {
    let actions =
        Paragraph::new(" Enter: edit | d: toggle | n: new | x: delete | j/k: navigate").style(
            Style::default()
                .fg(Color::White)
                .bg(theme::STATUS_BAR_BG),
        );
    frame.render_widget(actions, area);
}

/// Shorten status for display
fn short_status(status: &str) -> &str {
    match status {
        "Active" => "on",
        "Paused" => "off",
        "Completed" => "done",
        "Failed" => "err",
        "Executing" => "run",
        "Cancelled" => "off",
        _ => status,
    }
}

/// Truncate a prompt to a max length for table display.
/// Uses char boundaries to avoid panicking on multi-byte UTF-8.
fn truncate_prompt(prompt: &str, max_len: usize) -> String {
    // Collapse whitespace for single-line display
    let collapsed: String = prompt
        .chars()
        .map(|c| if c.is_whitespace() { ' ' } else { c })
        .collect();
    let trimmed = collapsed.trim();

    if trimmed.len() <= max_len {
        trimmed.to_string()
    } else {
        let target = max_len.saturating_sub(3);
        let boundary = trimmed.floor_char_boundary(target);
        format!("{}...", &trimmed[..boundary])
    }
}

/// Format the next run time for display
fn format_next_run(next_run: &str) -> String {
    // Try to extract just the time portion (HH:MM) from an RFC3339 timestamp
    if let Some(time_start) = next_run.find('T') {
        let time_part = &next_run[time_start + 1..];
        if time_part.len() >= 5 {
            return time_part[..5].to_string();
        }
    }
    // If it looks like a cron expression (contains */), show as-is
    if next_run.contains("*/") || next_run.contains(' ') {
        return truncate_prompt(next_run, 10);
    }
    truncate_prompt(next_run, 10)
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_short_status() {
        assert_eq!(short_status("Active"), "on");
        assert_eq!(short_status("Paused"), "off");
        assert_eq!(short_status("Completed"), "done");
        assert_eq!(short_status("Failed"), "err");
        assert_eq!(short_status("Executing"), "run");
        assert_eq!(short_status("Cancelled"), "off");
        assert_eq!(short_status("Unknown"), "Unknown");
    }

    #[test]
    fn test_truncate_prompt_short() {
        assert_eq!(truncate_prompt("daily digest", 30), "daily digest");
    }

    #[test]
    fn test_truncate_prompt_long() {
        let long = "a".repeat(50);
        let result = truncate_prompt(&long, 20);
        assert!(result.len() <= 20);
        assert!(result.ends_with("..."));
    }

    #[test]
    fn test_truncate_prompt_whitespace() {
        assert_eq!(
            truncate_prompt("line1\nline2\ttab", 30),
            "line1 line2 tab"
        );
    }

    #[test]
    fn test_truncate_prompt_empty() {
        assert_eq!(truncate_prompt("", 10), "");
    }

    #[test]
    fn test_format_next_run_rfc3339() {
        assert_eq!(
            format_next_run("2026-02-15T18:00:00Z"),
            "18:00"
        );
    }

    #[test]
    fn test_format_next_run_with_offset() {
        assert_eq!(
            format_next_run("2026-02-15T06:30:00+05:00"),
            "06:30"
        );
    }

    #[test]
    fn test_format_next_run_cron() {
        assert_eq!(format_next_run("*/30m"), "*/30m");
    }

    #[test]
    fn test_format_next_run_plain() {
        assert_eq!(format_next_run("tomorrow"), "tomorrow");
    }
}
