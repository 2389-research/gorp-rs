// ABOUTME: TUI application state and main render/event loop
// ABOUTME: Manages views, navigation, and delegates rendering to view-specific modules

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph};
use std::collections::VecDeque;

use super::event::TuiEvent;
use super::sidebar;
use super::theme;

// =============================================================================
// View enum — which screen is active
// =============================================================================

#[derive(Debug, Clone, PartialEq)]
pub enum View {
    Dashboard,
    Feed,
    Workspace { name: String },
    Channels,
    Schedules,
    Logs,
}

impl View {
    pub fn label(&self) -> &str {
        match self {
            View::Dashboard => "Dashboard",
            View::Feed => "Feed",
            View::Workspace { .. } => "Workspace",
            View::Channels => "Channels",
            View::Schedules => "Schedules",
            View::Logs => "Logs",
        }
    }
}

// =============================================================================
// Event handling result
// =============================================================================

pub enum EventResult {
    Continue,
    Quit,
}

// =============================================================================
// Feed message for the cross-platform feed view
// =============================================================================

#[derive(Debug, Clone)]
pub struct FeedMessage {
    pub platform_id: String,
    pub channel_name: String,
    pub sender: String,
    pub body: String,
    pub timestamp: i64,
    pub channel_id: String,
    pub is_bot: bool,
}

// =============================================================================
// Platform status for sidebar display
// =============================================================================

#[derive(Debug, Clone)]
pub struct PlatformStatus {
    pub name: String,
    pub connected: bool,
}

// =============================================================================
// TuiApp — main application state
// =============================================================================

pub struct TuiApp {
    pub view: View,
    pub should_quit: bool,
    pub feed_messages: VecDeque<FeedMessage>,
    pub feed_scroll: usize,
    pub platform_statuses: Vec<PlatformStatus>,
    pub nav_selected: usize,
    pub input_buffer: String,
    pub input_mode: bool,
}

/// Maximum number of feed messages to keep in memory
const MAX_FEED_MESSAGES: usize = 500;

impl TuiApp {
    pub fn new() -> Self {
        Self {
            view: View::Dashboard,
            should_quit: false,
            feed_messages: VecDeque::with_capacity(MAX_FEED_MESSAGES),
            feed_scroll: 0,
            platform_statuses: Vec::new(),
            nav_selected: 0,
            input_buffer: String::new(),
            input_mode: false,
        }
    }

    /// Navigation items in order
    pub fn nav_items() -> &'static [&'static str] {
        &["Dashboard", "Feed", "Workspace", "Channels", "Schedules", "Logs"]
    }

    /// Handle a TUI event and return whether to continue
    pub fn handle_event(&mut self, event: TuiEvent) -> EventResult {
        match event {
            TuiEvent::Key(key) => self.handle_key(key),
            TuiEvent::Tick => EventResult::Continue,
            TuiEvent::PlatformMessage(msg) => {
                self.add_feed_message(msg);
                EventResult::Continue
            }
            TuiEvent::PlatformStatus { name, connected } => {
                self.update_platform_status(name, connected);
                EventResult::Continue
            }
        }
    }

    /// Handle a key event
    fn handle_key(&mut self, key: KeyEvent) -> EventResult {
        // Global keybindings
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            return EventResult::Quit;
        }

        if key.code == KeyCode::Char('q') && !self.input_mode {
            return EventResult::Quit;
        }

        // Input mode keybindings
        if self.input_mode {
            match key.code {
                KeyCode::Esc => {
                    self.input_mode = false;
                }
                KeyCode::Enter => {
                    // Submit input (handled by workspace/chat views later)
                    self.input_buffer.clear();
                }
                KeyCode::Backspace => {
                    self.input_buffer.pop();
                }
                KeyCode::Char(c) => {
                    self.input_buffer.push(c);
                }
                _ => {}
            }
            return EventResult::Continue;
        }

        // Navigation keybindings (not in input mode)
        match key.code {
            KeyCode::Char('d') | KeyCode::Char('D') => {
                self.view = View::Dashboard;
                self.nav_selected = 0;
            }
            KeyCode::Char('f') | KeyCode::Char('F') => {
                self.view = View::Feed;
                self.nav_selected = 1;
            }
            KeyCode::Char('w') | KeyCode::Char('W') => {
                self.view = View::Workspace {
                    name: String::new(),
                };
                self.nav_selected = 2;
            }
            KeyCode::Char('c') | KeyCode::Char('C') => {
                self.view = View::Channels;
                self.nav_selected = 3;
            }
            KeyCode::Char('s') | KeyCode::Char('S') => {
                self.view = View::Schedules;
                self.nav_selected = 4;
            }
            KeyCode::Char('l') | KeyCode::Char('L') => {
                self.view = View::Logs;
                self.nav_selected = 5;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if self.nav_selected > 0 {
                    self.nav_selected -= 1;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let max = Self::nav_items().len() - 1;
                if self.nav_selected < max {
                    self.nav_selected += 1;
                }
            }
            KeyCode::Enter => {
                self.navigate_to_selected();
            }
            KeyCode::Char('i') => {
                // Enter input mode (for workspace/chat views)
                if matches!(self.view, View::Workspace { .. }) {
                    self.input_mode = true;
                }
            }
            _ => {}
        }

        EventResult::Continue
    }

    /// Navigate to the currently selected nav item
    fn navigate_to_selected(&mut self) {
        self.view = match self.nav_selected {
            0 => View::Dashboard,
            1 => View::Feed,
            2 => View::Workspace {
                name: String::new(),
            },
            3 => View::Channels,
            4 => View::Schedules,
            5 => View::Logs,
            _ => return,
        };
    }

    /// Add a message to the feed
    fn add_feed_message(&mut self, msg: FeedMessage) {
        if self.feed_messages.len() >= MAX_FEED_MESSAGES {
            self.feed_messages.pop_front();
        }
        self.feed_messages.push_back(msg);
    }

    /// Update platform connection status
    fn update_platform_status(&mut self, name: String, connected: bool) {
        if let Some(status) = self.platform_statuses.iter_mut().find(|s| s.name == name) {
            status.connected = connected;
        } else {
            self.platform_statuses.push(PlatformStatus { name, connected });
        }
    }

    /// Main render function — delegates to layout components
    pub fn render(&self, frame: &mut Frame) {
        let area = frame.area();

        // Split into sidebar and main content
        let layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(22), Constraint::Min(40)])
            .split(area);

        // Render sidebar
        sidebar::render_sidebar(frame, layout[0], self);

        // Split main area into content and status bar
        let main_layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(3), Constraint::Length(1)])
            .split(layout[1]);

        // Render main content based on view
        self.render_main_content(frame, main_layout[0]);

        // Render status bar
        self.render_status_bar(frame, main_layout[1]);
    }

    /// Render the main content area based on current view
    fn render_main_content(&self, frame: &mut Frame, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .title(format!(" {} ", self.view.label()))
            .border_style(Style::default().fg(theme::BORDER_COLOR));

        let content = match &self.view {
            View::Dashboard => "Dashboard view - press D\n\nPlatform connections and statistics will appear here.",
            View::Feed => "Feed view - press F\n\nCross-platform message feed will appear here.\nj/k to scroll, f to filter.",
            View::Workspace { name } => {
                if name.is_empty() {
                    "Workspace view - press W\n\nSelect a workspace to start chatting with Claude.\nPress i to enter input mode."
                } else {
                    "Workspace: active\n\nConversation will appear here."
                }
            }
            View::Channels => "Channels view - press C\n\nChannel list and management will appear here.",
            View::Schedules => "Schedules view - press S\n\nSchedule list and management will appear here.",
            View::Logs => "Logs view - press L\n\nLog viewer with filtering will appear here.",
        };

        let paragraph = Paragraph::new(content)
            .block(block)
            .style(Style::default().fg(theme::TEXT_COLOR));

        frame.render_widget(paragraph, area);
    }

    /// Render the bottom status bar
    fn render_status_bar(&self, frame: &mut Frame, area: Rect) {
        let mode_indicator = if self.input_mode { "INSERT" } else { "NORMAL" };
        let help_text = if self.input_mode {
            "ESC: exit input | Enter: send"
        } else {
            "q: quit | D/F/W/C/S/L: navigate | i: input mode"
        };

        let status = Paragraph::new(format!(" {} | {} ", mode_indicator, help_text))
            .style(
                Style::default()
                    .fg(Color::White)
                    .bg(theme::STATUS_BAR_BG),
            );

        frame.render_widget(status, area);
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tui_app_new() {
        let app = TuiApp::new();
        assert_eq!(app.view, View::Dashboard);
        assert!(!app.should_quit);
        assert!(app.feed_messages.is_empty());
        assert_eq!(app.nav_selected, 0);
        assert!(!app.input_mode);
    }

    #[test]
    fn test_view_labels() {
        assert_eq!(View::Dashboard.label(), "Dashboard");
        assert_eq!(View::Feed.label(), "Feed");
        assert_eq!(
            View::Workspace {
                name: "test".to_string()
            }
            .label(),
            "Workspace"
        );
        assert_eq!(View::Channels.label(), "Channels");
        assert_eq!(View::Schedules.label(), "Schedules");
        assert_eq!(View::Logs.label(), "Logs");
    }

    #[test]
    fn test_quit_on_q() {
        let mut app = TuiApp::new();
        let result = app.handle_event(TuiEvent::Key(KeyEvent::new(
            KeyCode::Char('q'),
            KeyModifiers::NONE,
        )));
        assert!(matches!(result, EventResult::Quit));
    }

    #[test]
    fn test_quit_on_ctrl_c() {
        let mut app = TuiApp::new();
        let result = app.handle_event(TuiEvent::Key(KeyEvent::new(
            KeyCode::Char('c'),
            KeyModifiers::CONTROL,
        )));
        assert!(matches!(result, EventResult::Quit));
    }

    #[test]
    fn test_navigate_to_feed() {
        let mut app = TuiApp::new();
        app.handle_event(TuiEvent::Key(KeyEvent::new(
            KeyCode::Char('f'),
            KeyModifiers::NONE,
        )));
        assert_eq!(app.view, View::Feed);
        assert_eq!(app.nav_selected, 1);
    }

    #[test]
    fn test_navigate_to_workspace() {
        let mut app = TuiApp::new();
        app.handle_event(TuiEvent::Key(KeyEvent::new(
            KeyCode::Char('w'),
            KeyModifiers::NONE,
        )));
        assert!(matches!(app.view, View::Workspace { .. }));
        assert_eq!(app.nav_selected, 2);
    }

    #[test]
    fn test_add_feed_message() {
        let mut app = TuiApp::new();
        app.add_feed_message(FeedMessage {
            platform_id: "matrix".to_string(),
            channel_name: "#test".to_string(),
            sender: "user".to_string(),
            body: "hello".to_string(),
            timestamp: 12345,
            channel_id: "!abc:matrix.org".to_string(),
            is_bot: false,
        });
        assert_eq!(app.feed_messages.len(), 1);
    }

    #[test]
    fn test_feed_message_ring_buffer() {
        let mut app = TuiApp::new();
        for i in 0..MAX_FEED_MESSAGES + 10 {
            app.add_feed_message(FeedMessage {
                platform_id: "test".to_string(),
                channel_name: format!("#{}", i),
                sender: "user".to_string(),
                body: format!("message {}", i),
                timestamp: i as i64,
                channel_id: format!("ch_{}", i),
                is_bot: false,
            });
        }
        assert_eq!(app.feed_messages.len(), MAX_FEED_MESSAGES);
    }

    #[test]
    fn test_platform_status_update() {
        let mut app = TuiApp::new();
        app.update_platform_status("matrix".to_string(), true);
        assert_eq!(app.platform_statuses.len(), 1);
        assert!(app.platform_statuses[0].connected);

        app.update_platform_status("matrix".to_string(), false);
        assert_eq!(app.platform_statuses.len(), 1);
        assert!(!app.platform_statuses[0].connected);
    }

    #[test]
    fn test_input_mode_toggle() {
        let mut app = TuiApp::new();
        // Navigate to workspace first
        app.handle_event(TuiEvent::Key(KeyEvent::new(
            KeyCode::Char('w'),
            KeyModifiers::NONE,
        )));
        // Enter input mode
        app.handle_event(TuiEvent::Key(KeyEvent::new(
            KeyCode::Char('i'),
            KeyModifiers::NONE,
        )));
        assert!(app.input_mode);

        // Typing in input mode
        app.handle_event(TuiEvent::Key(KeyEvent::new(
            KeyCode::Char('h'),
            KeyModifiers::NONE,
        )));
        assert_eq!(app.input_buffer, "h");

        // Backspace
        app.handle_event(TuiEvent::Key(KeyEvent::new(
            KeyCode::Backspace,
            KeyModifiers::NONE,
        )));
        assert!(app.input_buffer.is_empty());

        // Exit input mode
        app.handle_event(TuiEvent::Key(KeyEvent::new(
            KeyCode::Esc,
            KeyModifiers::NONE,
        )));
        assert!(!app.input_mode);
    }

    #[test]
    fn test_q_does_not_quit_in_input_mode() {
        let mut app = TuiApp::new();
        app.view = View::Workspace {
            name: String::new(),
        };
        app.input_mode = true;
        let result = app.handle_event(TuiEvent::Key(KeyEvent::new(
            KeyCode::Char('q'),
            KeyModifiers::NONE,
        )));
        assert!(matches!(result, EventResult::Continue));
        assert_eq!(app.input_buffer, "q");
    }

    #[test]
    fn test_nav_items_count() {
        assert_eq!(TuiApp::nav_items().len(), 6);
    }

    #[test]
    fn test_tick_event() {
        let mut app = TuiApp::new();
        let result = app.handle_event(TuiEvent::Tick);
        assert!(matches!(result, EventResult::Continue));
    }

    #[test]
    fn test_nav_arrow_keys() {
        let mut app = TuiApp::new();
        assert_eq!(app.nav_selected, 0);

        // Down
        app.handle_event(TuiEvent::Key(KeyEvent::new(
            KeyCode::Down,
            KeyModifiers::NONE,
        )));
        assert_eq!(app.nav_selected, 1);

        // Up
        app.handle_event(TuiEvent::Key(KeyEvent::new(
            KeyCode::Up,
            KeyModifiers::NONE,
        )));
        assert_eq!(app.nav_selected, 0);

        // Up at top stays at top
        app.handle_event(TuiEvent::Key(KeyEvent::new(
            KeyCode::Up,
            KeyModifiers::NONE,
        )));
        assert_eq!(app.nav_selected, 0);
    }

    #[test]
    fn test_enter_navigates() {
        let mut app = TuiApp::new();
        app.nav_selected = 1; // Feed
        app.handle_event(TuiEvent::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        )));
        assert_eq!(app.view, View::Feed);
    }
}
