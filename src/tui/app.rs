// ABOUTME: TUI application state and main render/event loop
// ABOUTME: Manages views, navigation, and delegates rendering to view-specific modules

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::prelude::*;
use ratatui::widgets::Paragraph;
use std::collections::VecDeque;

use super::event::TuiEvent;
use super::sidebar;
use super::theme;
use super::views;

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
// Conversation message for workspace chat
// =============================================================================

#[derive(Debug, Clone)]
pub struct ConversationMessage {
    pub role: String,
    pub content: String,
}

// =============================================================================
// Workspace info for workspace list
// =============================================================================

#[derive(Debug, Clone)]
pub struct WorkspaceInfo {
    pub name: String,
    pub path: String,
    pub active: bool,
}

// =============================================================================
// Schedule info for schedules view
// =============================================================================

#[derive(Debug, Clone)]
pub struct ScheduleInfo {
    pub id: String,
    pub channel_name: String,
    pub prompt: String,
    pub next_run: String,
    pub status: String,
}

// =============================================================================
// Log entry for logs view
// =============================================================================

#[derive(Debug, Clone)]
pub struct LogEntry {
    pub timestamp: String,
    pub level: String,
    pub target: String,
    pub message: String,
}

// =============================================================================
// Chat message for platform channel chat view
// =============================================================================

#[derive(Debug, Clone)]
pub struct ChatMessage {
    pub platform_id: String,
    pub sender: String,
    pub body: String,
    pub timestamp: i64,
}

// =============================================================================
// TuiApp — main application state
// =============================================================================

pub struct TuiApp {
    pub view: View,
    pub should_quit: bool,
    pub feed_messages: VecDeque<FeedMessage>,
    pub feed_scroll: usize,
    pub feed_filter: Option<String>,
    pub feed_selected: usize,
    pub platform_statuses: Vec<PlatformStatus>,
    pub nav_selected: usize,
    pub input_buffer: String,
    pub input_mode: bool,
    pub uptime_secs: u64,
    /// Tick counter for uptime tracking (10 ticks = 1 second at 100ms interval)
    pub tick_count: u32,
    pub workspace_sidebar_open: bool,
    pub workspaces: Vec<WorkspaceInfo>,
    pub workspace_selected: usize,
    pub conversation_messages: Vec<ConversationMessage>,
    pub conversation_scroll: usize,
    pub is_streaming: bool,
    pub schedules: Vec<ScheduleInfo>,
    pub schedule_selected: usize,
    pub log_entries: VecDeque<LogEntry>,
    pub log_scroll: usize,
    pub log_level_filter: String,
    pub log_workspace_filter: Option<String>,
    pub chat_messages: Vec<ChatMessage>,
    pub chat_scroll: usize,
    pub chat_channel_name: Option<String>,
}

/// Maximum number of feed messages to keep in memory
const MAX_FEED_MESSAGES: usize = 500;

/// Maximum number of log entries to keep in memory
const MAX_LOG_ENTRIES: usize = 1000;

impl TuiApp {
    pub fn new() -> Self {
        Self {
            view: View::Dashboard,
            should_quit: false,
            feed_messages: VecDeque::with_capacity(MAX_FEED_MESSAGES),
            feed_scroll: 0,
            feed_filter: None,
            feed_selected: 0,
            platform_statuses: Vec::new(),
            nav_selected: 0,
            input_buffer: String::new(),
            input_mode: false,
            uptime_secs: 0,
            tick_count: 0,
            workspace_sidebar_open: true,
            workspaces: Vec::new(),
            workspace_selected: 0,
            conversation_messages: Vec::new(),
            conversation_scroll: 0,
            is_streaming: false,
            schedules: Vec::new(),
            schedule_selected: 0,
            log_entries: VecDeque::with_capacity(MAX_LOG_ENTRIES),
            log_scroll: 0,
            log_level_filter: "INFO".to_string(),
            log_workspace_filter: None,
            chat_messages: Vec::new(),
            chat_scroll: 0,
            chat_channel_name: None,
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
            TuiEvent::Tick => {
                self.tick_count += 1;
                // Tick interval is 100ms, so 10 ticks = 1 second
                if self.tick_count >= 10 {
                    self.tick_count = 0;
                    self.uptime_secs += 1;
                }
                EventResult::Continue
            }
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
                if matches!(self.view, View::Workspace { .. } | View::Channels) {
                    self.input_mode = true;
                }
            }
            KeyCode::Char('1') => {
                if matches!(self.view, View::Logs) {
                    self.log_level_filter = "ERROR".to_string();
                }
            }
            KeyCode::Char('2') => {
                if matches!(self.view, View::Logs) {
                    self.log_level_filter = "WARN".to_string();
                }
            }
            KeyCode::Char('3') => {
                if matches!(self.view, View::Logs) {
                    self.log_level_filter = "INFO".to_string();
                }
            }
            KeyCode::Char('4') => {
                if matches!(self.view, View::Logs) {
                    self.log_level_filter = "DEBUG".to_string();
                }
            }
            KeyCode::Tab => {
                // Toggle workspace sidebar
                if matches!(self.view, View::Workspace { .. }) {
                    self.workspace_sidebar_open = !self.workspace_sidebar_open;
                }
            }
            KeyCode::Char('g') => {
                // Scroll to top
                match &self.view {
                    View::Feed => self.feed_scroll = 0,
                    View::Workspace { .. } => self.conversation_scroll = 0,
                    View::Channels => self.chat_scroll = 0,
                    View::Logs => self.log_scroll = 0,
                    _ => {}
                }
            }
            KeyCode::Char('G') => {
                // Scroll to bottom
                match &self.view {
                    View::Feed => {
                        self.feed_scroll = self.feed_messages.len().saturating_sub(1);
                    }
                    View::Workspace { .. } => {
                        self.conversation_scroll =
                            self.conversation_messages.len().saturating_sub(1);
                    }
                    View::Channels => {
                        self.chat_scroll = self.chat_messages.len().saturating_sub(1);
                    }
                    View::Logs => {
                        self.log_scroll = self.log_entries.len().saturating_sub(1);
                    }
                    _ => {}
                }
            }
            KeyCode::PageUp => {
                match &self.view {
                    View::Feed => {
                        self.feed_scroll = self.feed_scroll.saturating_sub(10);
                    }
                    View::Workspace { .. } => {
                        self.conversation_scroll = self.conversation_scroll.saturating_sub(10);
                    }
                    View::Channels => {
                        self.chat_scroll = self.chat_scroll.saturating_sub(10);
                    }
                    View::Logs => {
                        self.log_scroll = self.log_scroll.saturating_sub(10);
                    }
                    _ => {}
                }
            }
            KeyCode::PageDown => {
                match &self.view {
                    View::Feed => {
                        self.feed_scroll = self
                            .feed_scroll
                            .saturating_add(10)
                            .min(self.feed_messages.len().saturating_sub(1));
                    }
                    View::Workspace { .. } => {
                        self.conversation_scroll = self
                            .conversation_scroll
                            .saturating_add(10)
                            .min(self.conversation_messages.len().saturating_sub(1));
                    }
                    View::Channels => {
                        self.chat_scroll = self
                            .chat_scroll
                            .saturating_add(10)
                            .min(self.chat_messages.len().saturating_sub(1));
                    }
                    View::Logs => {
                        self.log_scroll = self
                            .log_scroll
                            .saturating_add(10)
                            .min(self.log_entries.len().saturating_sub(1));
                    }
                    _ => {}
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

    /// Get the name of the currently active workspace, if any
    pub fn active_workspace_name(&self) -> Option<&str> {
        self.workspaces
            .iter()
            .find(|ws| ws.active)
            .map(|ws| ws.name.as_str())
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
        match &self.view {
            View::Dashboard => views::dashboard::render_dashboard(frame, area, self),
            View::Feed => views::feed::render_feed(frame, area, self),
            View::Workspace { .. } => views::workspace::render_workspace(frame, area, self),
            View::Channels => views::chat::render_chat(frame, area, self),
            View::Schedules => views::schedules::render_schedules(frame, area, self),
            View::Logs => views::logs::render_logs(frame, area, self),
        }
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
