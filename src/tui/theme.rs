// ABOUTME: TUI color theme with platform-specific colors
// ABOUTME: Provides consistent styling for the terminal interface

use ratatui::prelude::Color;

// =============================================================================
// Global theme colors
// =============================================================================

/// Main text color
pub const TEXT_COLOR: Color = Color::White;

/// Dimmed/secondary text
pub const DIM_TEXT: Color = Color::DarkGray;

/// Border color for panels
pub const BORDER_COLOR: Color = Color::Gray;

/// Status bar background
pub const STATUS_BAR_BG: Color = Color::DarkGray;

/// Selected/highlighted item
pub const SELECTED_BG: Color = Color::Blue;

/// Selected item text
pub const SELECTED_FG: Color = Color::White;

/// Navigation header color
pub const NAV_HEADER: Color = Color::Yellow;

/// Connected status indicator
pub const CONNECTED_COLOR: Color = Color::Green;

/// Disconnected status indicator
pub const DISCONNECTED_COLOR: Color = Color::Red;

// =============================================================================
// Platform-specific colors
// =============================================================================

/// Matrix platform color (blue)
pub const MATRIX_COLOR: Color = Color::Blue;

/// Telegram platform color (cyan)
pub const TELEGRAM_COLOR: Color = Color::Cyan;

/// Slack platform color (magenta/purple)
pub const SLACK_COLOR: Color = Color::Magenta;

/// WhatsApp platform color (green)
pub const WHATSAPP_COLOR: Color = Color::Green;

/// Default platform color for unknown platforms
pub const DEFAULT_PLATFORM_COLOR: Color = Color::White;

/// Get the color for a platform by its ID
pub fn platform_color(platform_id: &str) -> Color {
    match platform_id {
        "matrix" => MATRIX_COLOR,
        "telegram" => TELEGRAM_COLOR,
        "slack" => SLACK_COLOR,
        "whatsapp" => WHATSAPP_COLOR,
        _ => DEFAULT_PLATFORM_COLOR,
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_platform_color_matrix() {
        assert_eq!(platform_color("matrix"), Color::Blue);
    }

    #[test]
    fn test_platform_color_telegram() {
        assert_eq!(platform_color("telegram"), Color::Cyan);
    }

    #[test]
    fn test_platform_color_slack() {
        assert_eq!(platform_color("slack"), Color::Magenta);
    }

    #[test]
    fn test_platform_color_whatsapp() {
        assert_eq!(platform_color("whatsapp"), Color::Green);
    }

    #[test]
    fn test_platform_color_unknown() {
        assert_eq!(platform_color("discord"), Color::White);
    }
}
