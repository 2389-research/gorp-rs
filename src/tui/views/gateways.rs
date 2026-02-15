// ABOUTME: TUI Gateways view showing platform connection status
// ABOUTME: Displays gateway name, connection indicator, and state text with row selection

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table};

use crate::tui::app::TuiApp;
use crate::tui::theme;

/// Render the gateways view in the given area
pub fn render_gateways(frame: &mut Frame, area: Rect, app: &TuiApp) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Gateways ")
        .border_style(Style::default().fg(theme::BORDER_COLOR));

    if app.gateway_infos.is_empty() {
        let empty = Paragraph::new("  No gateways registered")
            .style(Style::default().fg(theme::DIM_TEXT))
            .block(block);
        frame.render_widget(empty, area);
        return;
    }

    let header = Row::new(vec![
        Cell::from(Span::styled(
            "  ",
            Style::default().fg(theme::DIM_TEXT),
        )),
        Cell::from(Span::styled(
            "Platform",
            Style::default()
                .fg(theme::DIM_TEXT)
                .add_modifier(Modifier::BOLD),
        )),
        Cell::from(Span::styled(
            "Status",
            Style::default()
                .fg(theme::DIM_TEXT)
                .add_modifier(Modifier::BOLD),
        )),
    ])
    .height(1);

    let rows: Vec<Row> = app
        .gateway_infos
        .iter()
        .enumerate()
        .map(|(i, gw)| {
            let indicator = if gw.connected { "  ●" } else { "  ○" };
            let indicator_color = if gw.connected {
                theme::CONNECTED_COLOR
            } else {
                theme::DISCONNECTED_COLOR
            };
            let platform_color = theme::platform_color(&gw.platform_id);

            let state_color = if gw.connected {
                theme::CONNECTED_COLOR
            } else {
                theme::DIM_TEXT
            };

            let row = Row::new(vec![
                Cell::from(Span::styled(
                    indicator,
                    Style::default().fg(indicator_color),
                )),
                Cell::from(Span::styled(
                    gw.platform_id.clone(),
                    Style::default()
                        .fg(platform_color)
                        .add_modifier(Modifier::BOLD),
                )),
                Cell::from(Span::styled(
                    gw.state_text.clone(),
                    Style::default().fg(state_color),
                )),
            ]);

            if i == app.gateway_selected {
                row.style(
                    Style::default()
                        .bg(theme::SELECTED_BG)
                        .fg(theme::SELECTED_FG),
                )
            } else {
                row
            }
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Length(4),
            Constraint::Length(14),
            Constraint::Min(16),
        ],
    )
    .header(header)
    .block(block);

    frame.render_widget(table, area);
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use crate::tui::app::GatewayInfo;

    #[test]
    fn test_gateway_info_construction() {
        let gw = GatewayInfo {
            platform_id: "matrix".to_string(),
            configured: true,
            connected: true,
            state_text: "Connected".to_string(),
        };
        assert_eq!(gw.platform_id, "matrix");
        assert!(gw.configured);
        assert!(gw.connected);
        assert_eq!(gw.state_text, "Connected");
    }

    #[test]
    fn test_gateway_info_disconnected() {
        let gw = GatewayInfo {
            platform_id: "telegram".to_string(),
            configured: true,
            connected: false,
            state_text: "Disconnected".to_string(),
        };
        assert!(!gw.connected);
        assert_eq!(gw.state_text, "Disconnected");
    }

    #[test]
    fn test_gateway_info_not_configured() {
        let gw = GatewayInfo {
            platform_id: "slack".to_string(),
            configured: false,
            connected: false,
            state_text: "Not configured".to_string(),
        };
        assert!(!gw.configured);
        assert!(!gw.connected);
    }
}
