// ABOUTME: TUI Feed view showing cross-platform message stream
// ABOUTME: Color-coded messages by platform with scrolling, filtering, and reply support

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};

use crate::tui::app::{FeedMessage, TuiApp};
use crate::tui::theme;

/// Render the feed view in the given area
pub fn render_feed(frame: &mut Frame, area: Rect, app: &TuiApp) {
    // Split into message list and filter bar
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(1)])
        .split(area);

    render_message_list(frame, layout[0], app);
    render_filter_bar(frame, layout[1], app);
}

/// Render the scrollable message list
fn render_message_list(frame: &mut Frame, area: Rect, app: &TuiApp) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Feed ")
        .border_style(Style::default().fg(theme::BORDER_COLOR));

    if app.feed_messages.is_empty() {
        let empty = Paragraph::new("  Waiting for messages from connected platforms...")
            .style(Style::default().fg(theme::DIM_TEXT))
            .block(block);
        frame.render_widget(empty, area);
        return;
    }

    // Filter messages by platform if filter is active
    let filtered_messages: Vec<&FeedMessage> = if let Some(ref filter) = app.feed_filter {
        app.feed_messages
            .iter()
            .filter(|m| m.platform_id == *filter)
            .collect()
    } else {
        app.feed_messages.iter().collect()
    };

    let visible_height = area.height.saturating_sub(2) as usize;
    let total = filtered_messages.len();

    // Clamp scroll to valid range
    let scroll = app.feed_scroll.min(total.saturating_sub(visible_height));

    let items: Vec<ListItem> = filtered_messages
        .iter()
        .skip(scroll)
        .take(visible_height)
        .enumerate()
        .map(|(i, msg)| {
            let is_selected = i + scroll == app.feed_selected;
            render_message_item(msg, is_selected)
        })
        .collect();

    // Show scroll position in title
    let title = if total > visible_height {
        format!(" Feed [{}/{}] ", scroll + 1, total)
    } else {
        format!(" Feed [{}] ", total)
    };

    let block = block.title(title);
    let list = List::new(items).block(block);
    frame.render_widget(list, area);
}

/// Render a single message as a ListItem
fn render_message_item(msg: &FeedMessage, selected: bool) -> ListItem<'static> {
    let platform_color = theme::platform_color(&msg.platform_id);

    let bot_marker = if msg.is_bot { " [bot]" } else { "" };

    let line = Line::from(vec![
        Span::styled(
            format!("[{}] ", msg.platform_id),
            Style::default().fg(platform_color),
        ),
        Span::styled(
            format!("#{} ", msg.channel_name),
            Style::default().fg(theme::DIM_TEXT),
        ),
        Span::styled(
            format!("{}{}: ", msg.sender, bot_marker),
            Style::default()
                .fg(theme::TEXT_COLOR)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            truncate_body(&msg.body, 80),
            Style::default().fg(theme::TEXT_COLOR),
        ),
    ]);

    let style = if selected {
        Style::default()
            .fg(theme::SELECTED_FG)
            .bg(theme::SELECTED_BG)
    } else {
        Style::default()
    };

    ListItem::new(line).style(style)
}

/// Render the filter bar at the bottom of the feed
fn render_filter_bar(frame: &mut Frame, area: Rect, app: &TuiApp) {
    let filter_text = match &app.feed_filter {
        Some(platform) => format!(" Filter: {} | f: clear | ", platform),
        None => " f: filter | ".to_string(),
    };

    let status = Paragraph::new(format!(
        "{}j/k: scroll | g/G: top/bottom | r: reply | Enter: open",
        filter_text,
    ))
    .style(
        Style::default()
            .fg(Color::White)
            .bg(theme::STATUS_BAR_BG),
    );

    frame.render_widget(status, area);
}

/// Truncate message body for display, collapsing newlines
fn truncate_body(body: &str, max_len: usize) -> String {
    // Collapse newlines into spaces for single-line display
    let collapsed: String = body
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
    fn test_truncate_body_short() {
        assert_eq!(truncate_body("hello", 10), "hello");
    }

    #[test]
    fn test_truncate_body_long() {
        let long = "a".repeat(100);
        let result = truncate_body(&long, 20);
        assert!(result.len() <= 20);
        assert!(result.ends_with("..."));
    }

    #[test]
    fn test_truncate_body_newlines() {
        assert_eq!(truncate_body("line1\nline2\nline3", 50), "line1 line2 line3");
    }

    #[test]
    fn test_truncate_body_empty() {
        assert_eq!(truncate_body("", 10), "");
    }

    #[test]
    fn test_truncate_body_exact() {
        assert_eq!(truncate_body("hello", 5), "hello");
    }

    #[test]
    fn test_truncate_body_carriage_return() {
        assert_eq!(truncate_body("line1\r\nline2", 20), "line1  line2");
    }
}
