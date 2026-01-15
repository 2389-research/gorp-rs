// ABOUTME: Main iced Application implementation for gorp desktop
// ABOUTME: Manages app state, message routing, and view rendering

use super::components::common;
use super::components::hotkey::{self, HotkeyManager};
use super::components::tray::{self, ConnectionState};
use super::sync::{self, MatrixEvent};
use super::theme::{
    self, button_primary, button_secondary, colors, content_style, modal_style, radius, spacing,
    text_input_style, text_size,
};
use super::views::chat::{chat_scroll_id, ChatMessage};
use super::views::{self, View};
use crate::config::Config;
use crate::scheduler::ScheduledPrompt;
use crate::server::{RoomInfo, ServerState};
use global_hotkey::HotKeyState;
use iced::widget::{button, column, container, row, scrollable, text, text_input, Space};
use iced::{Alignment, Border, Element, Length, Subscription, Task, Theme};
use std::sync::Arc;
use tokio::sync::mpsc;
use tray_icon::TrayIcon;

/// Main application state
pub struct GorpApp {
    /// Server state (None while initializing)
    server: Option<Arc<ServerState>>,

    /// Current view
    view: View,

    /// Initialization error (if any)
    init_error: Option<String>,

    /// Status message
    status: String,

    /// System tray icon handle
    tray_icon: Option<TrayIcon>,

    /// Global hotkey manager
    hotkey_manager: Option<HotkeyManager>,

    /// Current connection state for tray icon
    connection_state: ConnectionState,

    /// Quick prompt modal state
    quick_prompt_visible: bool,

    /// Quick prompt input text
    quick_prompt_input: String,

    /// Cached room list
    rooms: Vec<RoomInfo>,

    /// Chat input text (per-room would be better, but simple for now)
    chat_input: String,

    /// Cached chat messages for current room
    chat_messages: Vec<ChatMessage>,

    /// Current room name (for display)
    current_room_name: String,

    /// Current room ID (for filtering sync events)
    current_room_id: Option<String>,

    /// Matrix sync event receiver
    sync_rx: Option<mpsc::UnboundedReceiver<MatrixEvent>>,

    /// Typing users in current room
    typing_users: Vec<String>,

    /// Last typing update time (for timeout)
    typing_last_update: Option<std::time::Instant>,

    /// Chat loading state (true while fetching messages)
    chat_loading: bool,

    /// Room ID for which messages are being loaded (to handle race conditions)
    messages_loading_for_room: Option<String>,

    // === Schedule state ===
    /// Cached list of schedules
    schedules: Vec<ScheduledPrompt>,

    /// Schedules loading state
    schedules_loading: bool,

    /// Show create schedule modal
    show_create_schedule: bool,

    /// Create schedule form state
    schedule_form_channel: String,
    schedule_form_prompt: String,
    schedule_form_time: String,
    schedule_form_error: Option<String>,

    // === Logs state ===
    /// Cached log entries
    log_entries: Vec<LogEntry>,

    /// Logs loading state
    logs_loading: bool,

    /// Log level filter (None = show all)
    log_level_filter: Option<String>,

    // === Room management state ===
    /// Show create room modal
    show_create_room: bool,

    /// New room name input
    new_room_name: String,

    /// Room creation error message
    room_creation_error: Option<String>,
}

/// A parsed log entry
#[derive(Debug, Clone)]
pub struct LogEntry {
    pub timestamp: String,
    pub level: String,
    pub target: String,
    pub message: String,
}

/// Parse a NDJSON log line into a LogEntry
fn parse_log_line(line: &str) -> Option<LogEntry> {
    let json: serde_json::Value = serde_json::from_str(line).ok()?;
    Some(LogEntry {
        timestamp: json["timestamp"].as_str()?.to_string(),
        level: json["level"].as_str()?.to_string(),
        target: json["target"].as_str()?.to_string(),
        message: json["fields"]["message"]
            .as_str()
            .or_else(|| json["message"].as_str())
            .unwrap_or("")
            .to_string(),
    })
}

/// Messages the app can receive
#[derive(Debug, Clone)]
pub enum Message {
    /// Server initialization completed
    ServerReady(Result<Arc<ServerState>, String>),

    /// Navigation
    Navigate(View),

    /// Tray menu event
    TrayEvent(TrayMenuAction),

    /// Poll tick for tray and hotkey events
    Poll,

    /// Quick prompt toggled (show/hide)
    ToggleQuickPrompt,

    /// Quick prompt input changed
    QuickPromptInputChanged(String),

    /// Quick prompt submitted
    QuickPromptSubmit,

    /// Chat input changed
    ChatInputChanged(String),

    /// Send message to a room
    SendMessage { room_id: String },

    /// Message sent successfully
    MessageSent,

    /// Failed to send message
    MessageFailed(String),

    /// Refresh room list
    RefreshRooms,

    /// Matrix sync event received
    MatrixEvent(MatrixEvent),

    /// Room messages loaded
    RoomMessagesLoaded(Vec<ChatMessage>),

    // === Schedule messages ===
    /// Load schedules from store
    LoadSchedules,

    /// Schedules loaded
    SchedulesLoaded(Vec<ScheduledPrompt>),

    /// Pause a schedule
    PauseSchedule(String),

    /// Resume a schedule
    ResumeSchedule(String),

    /// Delete a schedule
    DeleteSchedule(String),

    /// Schedule action completed
    ScheduleActionDone(Result<(), String>),

    /// Show create schedule form
    ShowCreateSchedule,

    /// Hide create schedule form
    HideCreateSchedule,

    /// Schedule form: channel changed
    ScheduleFormChannelChanged(String),

    /// Schedule form: prompt changed
    ScheduleFormPromptChanged(String),

    /// Schedule form: time expression changed
    ScheduleFormTimeChanged(String),

    /// Create the schedule
    CreateSchedule,

    /// Schedule created result
    ScheduleCreated(Result<(), String>),

    // === Log messages ===
    /// Load logs from file
    LoadLogs,

    /// Logs loaded
    LogsLoaded(Vec<LogEntry>),

    /// Change log level filter
    LogLevelFilterChanged(Option<String>),

    // === Room management messages ===
    /// Show create room modal
    ShowCreateRoom,

    /// Hide create room modal
    HideCreateRoom,

    /// Room name input changed
    RoomNameChanged(String),

    /// Create the room
    CreateRoom,

    /// Room created result
    RoomCreated(Result<String, String>),

    /// Request to quit the application
    Quit,
}

/// Actions from tray menu
#[derive(Debug, Clone)]
pub enum TrayMenuAction {
    OpenDashboard,
    QuickPrompt,
    Settings,
    Quit,
}

impl GorpApp {
    fn new(config: Config) -> (Self, Task<Message>) {
        // Create tray icon
        let tray_icon = match tray::create_tray_icon() {
            Ok(tray) => {
                tracing::info!("Tray icon created");
                Some(tray)
            }
            Err(e) => {
                tracing::error!(error = %e, "Failed to create tray icon");
                None
            }
        };

        // Create hotkey manager
        let hotkey_manager = match HotkeyManager::new() {
            Ok(manager) => {
                tracing::info!("Hotkey manager created");
                Some(manager)
            }
            Err(e) => {
                tracing::error!(error = %e, "Failed to create hotkey manager");
                None
            }
        };

        let app = Self {
            server: None,
            view: View::Dashboard,
            init_error: None,
            status: "Initializing...".to_string(),
            tray_icon,
            hotkey_manager,
            connection_state: ConnectionState::Connecting,
            quick_prompt_visible: false,
            quick_prompt_input: String::new(),
            rooms: Vec::new(),
            chat_input: String::new(),
            chat_messages: Vec::new(),
            current_room_name: String::new(),
            current_room_id: None,
            sync_rx: None,
            typing_users: Vec::new(),
            typing_last_update: None,
            chat_loading: false,
            messages_loading_for_room: None,
            schedules: Vec::new(),
            schedules_loading: false,
            show_create_schedule: false,
            schedule_form_channel: String::new(),
            schedule_form_prompt: String::new(),
            schedule_form_time: String::new(),
            schedule_form_error: None,
            log_entries: Vec::new(),
            logs_loading: false,
            log_level_filter: None,
            show_create_room: false,
            new_room_name: String::new(),
            room_creation_error: None,
        };

        // Spawn server initialization
        let init_task = Task::perform(
            async move {
                match ServerState::initialize(config).await {
                    Ok(state) => Ok(Arc::new(state)),
                    Err(e) => Err(e.to_string()),
                }
            },
            Message::ServerReady,
        );

        (app, init_task)
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::ServerReady(result) => {
                match result {
                    Ok(server) => {
                        // Get initial room list
                        self.rooms = server.get_rooms();
                        tracing::info!(room_count = self.rooms.len(), "Fetched room list");

                        // Start Matrix sync loop
                        let sync_rx = sync::start_sync(
                            server.matrix_client.clone(),
                            server.sync_token.clone(),
                        );
                        self.sync_rx = Some(sync_rx);
                        tracing::info!("Started Matrix sync for GUI");

                        self.server = Some(server);
                        self.status = "Connected".to_string();
                        self.connection_state = ConnectionState::Connected;

                        // Update tray icon to connected state
                        if let Some(ref tray) = self.tray_icon {
                            tray::update_icon(tray, ConnectionState::Connected);
                            tray::update_status(tray, "gorp - Connected to Matrix");
                        }

                        tracing::info!("Server initialized successfully");
                    }
                    Err(e) => {
                        self.init_error = Some(e.clone());
                        self.status = format!("Error: {}", e);
                        self.connection_state = ConnectionState::Disconnected;

                        // Update tray icon to error state
                        if let Some(ref tray) = self.tray_icon {
                            tray::update_icon(tray, ConnectionState::Disconnected);
                            tray::update_status(tray, &format!("gorp - Error: {}", e));
                        }

                        tracing::error!(error = %e, "Server initialization failed");
                    }
                }
                Task::none()
            }
            Message::Navigate(view) => {
                // If navigating to a chat room, update the current room name and load messages
                if let View::Chat { ref room_id } = view {
                    self.current_room_name = self
                        .rooms
                        .iter()
                        .find(|r| r.id == *room_id)
                        .map(|r| r.name.clone())
                        .unwrap_or_else(|| room_id.clone());

                    self.current_room_id = Some(room_id.clone());
                    self.messages_loading_for_room = Some(room_id.clone());
                    self.chat_messages.clear();
                    self.chat_input.clear();
                    self.typing_users.clear();
                    self.chat_loading = true;

                    tracing::info!(room_id = %room_id, room_name = %self.current_room_name, "Navigated to room");

                    // Load room messages
                    if let Some(ref server) = self.server {
                        let client = server.matrix_client.clone();
                        let room_id = room_id.clone();
                        self.view = view;
                        return Task::perform(
                            async move {
                                let messages =
                                    sync::load_room_messages(&client, &room_id, 50).await;
                                messages
                                    .into_iter()
                                    .map(|(sender, content, timestamp, is_own)| {
                                        // UTF-8 safe truncation for dedup key
                                        let content_prefix: String =
                                            content.chars().take(50).collect();
                                        let dedup_key = Some(format!(
                                            "{}:{}:{}",
                                            sender, timestamp, content_prefix
                                        ));
                                        ChatMessage {
                                            sender,
                                            content,
                                            timestamp,
                                            is_own,
                                            dedup_key,
                                        }
                                    })
                                    .collect()
                            },
                            Message::RoomMessagesLoaded,
                        );
                    }
                } else if matches!(view, View::Schedules) {
                    // Load schedules when navigating to schedules view
                    self.current_room_id = None;
                    self.view = view;
                    return Task::done(Message::LoadSchedules);
                } else if matches!(view, View::Logs) {
                    // Load logs when navigating to logs view
                    self.current_room_id = None;
                    self.view = view;
                    return Task::done(Message::LoadLogs);
                } else {
                    self.current_room_id = None;
                }
                self.view = view;
                Task::none()
            }
            Message::TrayEvent(action) => {
                match action {
                    TrayMenuAction::OpenDashboard => {
                        self.view = View::Dashboard;
                    }
                    TrayMenuAction::QuickPrompt => {
                        self.quick_prompt_visible = true;
                    }
                    TrayMenuAction::Settings => {
                        self.view = View::Settings;
                    }
                    TrayMenuAction::Quit => {
                        return Task::done(Message::Quit);
                    }
                }
                Task::none()
            }
            Message::Poll => {
                // Clear stale typing indicators (5 second timeout)
                if let Some(last_update) = self.typing_last_update {
                    if last_update.elapsed() > std::time::Duration::from_secs(5) {
                        self.typing_users.clear();
                        self.typing_last_update = None;
                    }
                }

                // Poll for tray menu events
                if let Some(event) = tray::poll_menu_event() {
                    let action = match event.id.0.as_str() {
                        tray::menu_ids::OPEN_DASHBOARD => Some(TrayMenuAction::OpenDashboard),
                        tray::menu_ids::QUICK_PROMPT => Some(TrayMenuAction::QuickPrompt),
                        tray::menu_ids::SETTINGS => Some(TrayMenuAction::Settings),
                        tray::menu_ids::QUIT => Some(TrayMenuAction::Quit),
                        _ => None,
                    };

                    if let Some(action) = action {
                        return Task::done(Message::TrayEvent(action));
                    }
                }

                // Poll for hotkey events
                if let Some(ref manager) = self.hotkey_manager {
                    if let Some(event) = hotkey::poll_hotkey_event() {
                        if event.state == HotKeyState::Pressed
                            && hotkey::is_quick_prompt_event(&event, manager)
                        {
                            tracing::info!("Quick prompt hotkey pressed (Cmd+N)");
                            return Task::done(Message::ToggleQuickPrompt);
                        }
                    }
                }

                // Poll for Matrix sync events - drain all available events
                if let Some(ref mut rx) = self.sync_rx {
                    let mut tasks = Vec::new();
                    loop {
                        match rx.try_recv() {
                            Ok(event) => tasks.push(Task::done(Message::MatrixEvent(event))),
                            Err(mpsc::error::TryRecvError::Empty) => break,
                            Err(mpsc::error::TryRecvError::Disconnected) => {
                                // Channel closed - sync task died
                                tracing::error!("Sync channel disconnected");
                                self.connection_state = ConnectionState::Disconnected;
                                self.status = "Sync disconnected".to_string();
                                if let Some(ref tray) = self.tray_icon {
                                    tray::update_icon(tray, ConnectionState::Disconnected);
                                }
                                self.sync_rx = None;
                                break;
                            }
                        }
                    }
                    if !tasks.is_empty() {
                        return Task::batch(tasks);
                    }
                }

                Task::none()
            }
            Message::ToggleQuickPrompt => {
                self.quick_prompt_visible = !self.quick_prompt_visible;
                if self.quick_prompt_visible {
                    self.quick_prompt_input.clear();
                }
                Task::none()
            }
            Message::QuickPromptInputChanged(input) => {
                self.quick_prompt_input = input;
                Task::none()
            }
            Message::QuickPromptSubmit => {
                if !self.quick_prompt_input.trim().is_empty() {
                    // If we're in a chat view, send to that room
                    if let View::Chat { ref room_id } = self.view {
                        let room_id = room_id.clone();
                        self.quick_prompt_visible = false;
                        // Don't clear input yet - SendMessage will use it
                        return Task::done(Message::SendMessage { room_id });
                    }

                    // Otherwise, find DISPATCH room (the DM) and send there
                    if let Some(dispatch_room) = self.rooms.iter().find(|r| r.is_direct) {
                        let room_id = dispatch_room.id.clone();
                        tracing::info!(room_id = %room_id, "Routing quick prompt to DISPATCH");
                        self.quick_prompt_visible = false;
                        return Task::done(Message::SendMessage { room_id });
                    }

                    tracing::warn!("Quick prompt submitted but no DISPATCH room found");
                }
                self.quick_prompt_visible = false;
                self.quick_prompt_input.clear();
                Task::none()
            }
            Message::ChatInputChanged(input) => {
                self.chat_input = input;
                Task::none()
            }
            Message::SendMessage { room_id } => {
                // Get message from quick prompt or chat input
                let message_text = if !self.quick_prompt_input.is_empty() {
                    self.quick_prompt_input.clone()
                } else {
                    self.chat_input.clone()
                };

                if message_text.trim().is_empty() {
                    return Task::none();
                }

                // Clear inputs
                self.chat_input.clear();
                self.quick_prompt_input.clear();

                // Add message to local chat (optimistic update)
                let timestamp = chrono::Local::now().format("%H:%M").to_string();
                // UTF-8 safe truncation for dedup key
                let content_prefix: String = message_text.chars().take(50).collect();
                let dedup_key = Some(format!("You:{}:{}", timestamp, content_prefix));
                self.chat_messages.push(ChatMessage {
                    sender: "You".to_string(),
                    content: message_text.clone(),
                    timestamp,
                    is_own: true,
                    dedup_key,
                });

                // Create scroll task for after optimistic update
                let scroll_task =
                    scrollable::snap_to(chat_scroll_id(), scrollable::RelativeOffset::END);

                // Send message asynchronously
                if let Some(ref server) = self.server {
                    let client = server.matrix_client.clone();
                    let room_id_owned = room_id.clone();
                    let msg = message_text;

                    let send_task = Task::perform(
                        async move {
                            use matrix_sdk::ruma::events::room::message::RoomMessageEventContent;
                            use matrix_sdk::ruma::OwnedRoomId;

                            let room_id: OwnedRoomId = match room_id_owned.parse() {
                                Ok(id) => id,
                                Err(e) => {
                                    return Err(format!("Invalid room ID: {}", e));
                                }
                            };

                            let room = match client.get_room(&room_id) {
                                Some(r) => r,
                                None => {
                                    return Err("Room not found".to_string());
                                }
                            };

                            match room.send(RoomMessageEventContent::text_plain(&msg)).await {
                                Ok(_) => Ok(()),
                                Err(e) => Err(format!("Failed to send: {}", e)),
                            }
                        },
                        |result| match result {
                            Ok(()) => Message::MessageSent,
                            Err(e) => Message::MessageFailed(e),
                        },
                    );

                    // Return both scroll and send tasks
                    return Task::batch([scroll_task, send_task]);
                }

                scroll_task
            }
            Message::MessageSent => {
                tracing::info!("Message sent successfully");
                Task::none()
            }
            Message::MessageFailed(error) => {
                tracing::error!(error = %error, "Failed to send message");
                // Could show error in UI
                Task::none()
            }
            Message::RefreshRooms => {
                if let Some(ref server) = self.server {
                    self.rooms = server.get_rooms();
                    tracing::info!(room_count = self.rooms.len(), "Refreshed room list");
                }
                Task::none()
            }
            Message::MatrixEvent(event) => {
                match event {
                    MatrixEvent::Message {
                        room_id,
                        sender,
                        content,
                        timestamp,
                        is_own,
                    } => {
                        // Only add message if we're viewing that room
                        if self.current_room_id.as_ref() == Some(&room_id) {
                            // UTF-8 safe truncation for dedup key
                            let content_prefix: String = content.chars().take(50).collect();
                            let dedup_key = format!("{}:{}:{}", sender, timestamp, content_prefix);

                            // Check for duplicates using dedup_key
                            let is_duplicate = self
                                .chat_messages
                                .iter()
                                .any(|m| m.dedup_key.as_ref() == Some(&dedup_key));

                            // Also check for optimistic update duplicates:
                            // If this is our own message, check if there's an optimistic "You:..." entry
                            let is_optimistic_duplicate = is_own
                                && self.chat_messages.iter().any(|m| {
                                    if let Some(ref key) = m.dedup_key {
                                        // Optimistic updates use "You:timestamp:content"
                                        let optimistic_key =
                                            format!("You:{}:{}", timestamp, content_prefix);
                                        key == &optimistic_key
                                    } else {
                                        false
                                    }
                                });

                            if !is_duplicate && !is_optimistic_duplicate {
                                self.chat_messages.push(ChatMessage {
                                    sender,
                                    content,
                                    timestamp,
                                    is_own,
                                    dedup_key: Some(dedup_key),
                                });

                                // Cap message history to prevent unbounded growth
                                const MAX_MESSAGES: usize = 200;
                                if self.chat_messages.len() > MAX_MESSAGES {
                                    self.chat_messages
                                        .drain(0..self.chat_messages.len() - MAX_MESSAGES);
                                }

                                // Auto-scroll to bottom on new message
                                return scrollable::snap_to(
                                    chat_scroll_id(),
                                    scrollable::RelativeOffset::END,
                                );
                            }
                        }
                    }
                    MatrixEvent::RoomListChanged => {
                        // Refresh room list
                        if let Some(ref server) = self.server {
                            self.rooms = server.get_rooms();
                        }
                    }
                    MatrixEvent::Typing { room_id, users } => {
                        if self.current_room_id.as_ref() == Some(&room_id) {
                            self.typing_users = users;
                            self.typing_last_update = if self.typing_users.is_empty() {
                                None
                            } else {
                                Some(std::time::Instant::now())
                            };
                        }
                    }
                    MatrixEvent::SyncError(error) => {
                        tracing::error!(error = %error, "Matrix sync error");
                        self.status = format!("Sync error: {}", error);
                    }
                    MatrixEvent::ConnectionState(status) => {
                        use sync::ConnectionStatus;
                        match status {
                            ConnectionStatus::Connected => {
                                self.connection_state = ConnectionState::Connected;
                                self.status = "Connected".to_string();
                            }
                            ConnectionStatus::Syncing => {
                                self.connection_state = ConnectionState::Connecting;
                                self.status = "Syncing...".to_string();
                            }
                            ConnectionStatus::Disconnected => {
                                self.connection_state = ConnectionState::Disconnected;
                                self.status = "Disconnected".to_string();
                            }
                        }
                        if let Some(ref tray) = self.tray_icon {
                            tray::update_icon(tray, self.connection_state);
                        }
                    }
                }
                Task::none()
            }
            Message::RoomMessagesLoaded(mut messages) => {
                // Only accept if we're still viewing the same room we loaded for
                // This prevents race conditions when navigating quickly between rooms
                if self.messages_loading_for_room == self.current_room_id {
                    // Merge any live messages that arrived during loading
                    // (they have dedup_keys and won't be duplicated)
                    let live_messages: Vec<_> = self.chat_messages.drain(..).collect();

                    for live_msg in live_messages {
                        // Only add if not already in loaded messages (by dedup_key)
                        let is_duplicate = messages
                            .iter()
                            .any(|m| m.dedup_key.is_some() && m.dedup_key == live_msg.dedup_key);
                        if !is_duplicate {
                            messages.push(live_msg);
                        }
                    }

                    self.chat_messages = messages;
                    tracing::info!(count = self.chat_messages.len(), "Loaded room messages");
                } else {
                    tracing::debug!("Discarding stale room messages load");
                }

                self.chat_loading = false;
                self.messages_loading_for_room = None;

                // Scroll to bottom after messages load
                scrollable::snap_to(chat_scroll_id(), scrollable::RelativeOffset::END)
            }

            // === Schedule handlers ===
            Message::LoadSchedules => {
                let Some(ref server) = self.server else {
                    tracing::warn!("LoadSchedules called before server initialized");
                    return Task::none();
                };
                self.schedules_loading = true;
                let store = server.scheduler_store.clone();
                Task::perform(
                    async move {
                        // Run blocking DB call off the main thread
                        tokio::task::spawn_blocking(move || store.list_all())
                            .await
                            .unwrap_or_else(|_| Ok(Vec::new()))
                    },
                    |result| match result {
                        Ok(schedules) => Message::SchedulesLoaded(schedules),
                        Err(e) => {
                            tracing::error!(error = %e, "Failed to load schedules");
                            Message::SchedulesLoaded(Vec::new())
                        }
                    },
                )
            }
            Message::SchedulesLoaded(schedules) => {
                self.schedules = schedules;
                self.schedules_loading = false;
                Task::none()
            }
            Message::PauseSchedule(id) => {
                let Some(ref server) = self.server else {
                    tracing::warn!("PauseSchedule called before server initialized");
                    return Task::none();
                };
                let store = server.scheduler_store.clone();
                Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || store.pause_schedule(&id))
                            .await
                            .unwrap_or_else(|e| Err(anyhow::anyhow!("Task join error: {}", e)))
                    },
                    |result| {
                        if let Err(e) = &result {
                            tracing::error!(error = %e, "Failed to pause schedule");
                        }
                        Message::ScheduleActionDone(result.map(|_| ()).map_err(|e| e.to_string()))
                    },
                )
            }
            Message::ResumeSchedule(id) => {
                let Some(ref server) = self.server else {
                    tracing::warn!("ResumeSchedule called before server initialized");
                    return Task::none();
                };
                let store = server.scheduler_store.clone();
                Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || store.resume_schedule(&id))
                            .await
                            .unwrap_or_else(|e| Err(anyhow::anyhow!("Task join error: {}", e)))
                    },
                    |result| {
                        if let Err(e) = &result {
                            tracing::error!(error = %e, "Failed to resume schedule");
                        }
                        Message::ScheduleActionDone(result.map(|_| ()).map_err(|e| e.to_string()))
                    },
                )
            }
            Message::DeleteSchedule(id) => {
                let Some(ref server) = self.server else {
                    tracing::warn!("DeleteSchedule called before server initialized");
                    return Task::none();
                };
                let store = server.scheduler_store.clone();
                Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || store.delete_schedule(&id))
                            .await
                            .unwrap_or_else(|e| Err(anyhow::anyhow!("Task join error: {}", e)))
                    },
                    |result| {
                        if let Err(e) = &result {
                            tracing::error!(error = %e, "Failed to delete schedule");
                        }
                        Message::ScheduleActionDone(result.map(|_| ()).map_err(|e| e.to_string()))
                    },
                )
            }
            Message::ScheduleActionDone(result) => {
                match result {
                    Ok(_) => {
                        self.status = "Schedule updated".to_string();
                    }
                    Err(e) => {
                        self.status = format!("Schedule action failed: {}", e);
                        tracing::error!(error = %e, "Schedule action failed");
                    }
                }
                Task::done(Message::LoadSchedules)
            }
            Message::ShowCreateSchedule => {
                self.show_create_schedule = true;
                self.schedule_form_channel = String::new();
                self.schedule_form_prompt = String::new();
                self.schedule_form_time = String::new();
                self.schedule_form_error = None;
                Task::none()
            }
            Message::HideCreateSchedule => {
                self.show_create_schedule = false;
                self.schedule_form_error = None;
                Task::none()
            }
            Message::ScheduleFormChannelChanged(channel) => {
                self.schedule_form_channel = channel;
                Task::none()
            }
            Message::ScheduleFormPromptChanged(prompt) => {
                self.schedule_form_prompt = prompt;
                Task::none()
            }
            Message::ScheduleFormTimeChanged(time) => {
                self.schedule_form_time = time;
                Task::none()
            }
            Message::CreateSchedule => {
                // Clear previous error
                self.schedule_form_error = None;

                if let Some(ref server) = self.server {
                    // Validate form
                    if self.schedule_form_channel.is_empty() {
                        self.schedule_form_error = Some("Please select a room".to_string());
                        return Task::none();
                    }
                    if self.schedule_form_prompt.is_empty() {
                        self.schedule_form_error = Some("Please enter a prompt".to_string());
                        return Task::none();
                    }
                    if self.schedule_form_time.is_empty() {
                        self.schedule_form_error = Some("Please enter a schedule time".to_string());
                        return Task::none();
                    }

                    // Find room_id for channel
                    let room_id = self
                        .rooms
                        .iter()
                        .find(|r| r.name == self.schedule_form_channel)
                        .map(|r| r.id.clone());

                    let Some(room_id) = room_id else {
                        self.schedule_form_error =
                            Some(format!("Room '{}' not found", self.schedule_form_channel));
                        return Task::none();
                    };

                    // Parse time expression (use local timezone)
                    use crate::scheduler::{parse_time_expression, ParsedSchedule};
                    let local_tz = chrono::Local::now().format("%z").to_string();
                    let parsed = match parse_time_expression(&self.schedule_form_time, &local_tz) {
                        Ok(p) => p,
                        Err(e) => {
                            self.schedule_form_error = Some(format!("Invalid time: {}", e));
                            return Task::none();
                        }
                    };

                    // Extract schedule details from parsed enum
                    let (execute_at, cron_expression, next_execution_at) = match parsed {
                        ParsedSchedule::OneTime(dt) => {
                            (Some(dt.to_rfc3339()), None, dt.to_rfc3339())
                        }
                        ParsedSchedule::Recurring { cron, next } => {
                            (None, Some(cron), next.to_rfc3339())
                        }
                    };

                    let schedule = ScheduledPrompt {
                        id: uuid::Uuid::new_v4().to_string(),
                        channel_name: self.schedule_form_channel.clone(),
                        room_id,
                        prompt: self.schedule_form_prompt.clone(),
                        created_by: "gui".to_string(),
                        created_at: chrono::Utc::now().to_rfc3339(),
                        execute_at,
                        cron_expression,
                        last_executed_at: None,
                        next_execution_at,
                        status: crate::scheduler::ScheduleStatus::Active,
                        error_message: None,
                        execution_count: 0,
                    };

                    let store = server.scheduler_store.clone();
                    return Task::perform(
                        async move {
                            tokio::task::spawn_blocking(move || store.create_schedule(&schedule))
                                .await
                                .unwrap_or_else(|e| Err(anyhow::anyhow!("Task join error: {}", e)))
                        },
                        |result| Message::ScheduleCreated(result.map_err(|e| e.to_string())),
                    );
                }
                Task::none()
            }
            Message::ScheduleCreated(result) => {
                match result {
                    Ok(_) => {
                        self.show_create_schedule = false;
                        return Task::done(Message::LoadSchedules);
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "Failed to create schedule");
                    }
                }
                Task::none()
            }

            // === Log handlers ===
            Message::LoadLogs => {
                self.logs_loading = true;

                // Load logs asynchronously to avoid blocking the UI thread
                return Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || {
                            // Load logs from today's log file
                            // Use ~/.local/share/gorp/logs on Unix, or fallback to current dir
                            let log_dir = std::env::var("HOME")
                                .map(|h| std::path::PathBuf::from(h).join(".local/share/gorp/logs"))
                                .unwrap_or_else(|_| std::path::PathBuf::from("."));

                            let today = chrono::Local::now().format("%Y-%m-%d").to_string();
                            let log_file = log_dir.join(format!("debug.log.{}", today));

                            if log_file.exists() {
                                match std::fs::read_to_string(&log_file) {
                                    Ok(content) => {
                                        content
                                            .lines()
                                            .rev() // Most recent first
                                            .take(500) // Limit entries
                                            .filter_map(|line| parse_log_line(line))
                                            .collect()
                                    }
                                    Err(e) => {
                                        tracing::error!(error = %e, "Failed to read log file");
                                        Vec::new()
                                    }
                                }
                            } else {
                                tracing::info!(path = %log_file.display(), "Log file not found");
                                Vec::new()
                            }
                        })
                        .await
                        .unwrap_or_else(|_| Vec::new())
                    },
                    Message::LogsLoaded,
                );
            }
            Message::LogsLoaded(entries) => {
                self.log_entries = entries;
                self.logs_loading = false;
                Task::none()
            }
            Message::LogLevelFilterChanged(filter) => {
                self.log_level_filter = filter;
                Task::none()
            }

            // === Room management handlers ===
            Message::ShowCreateRoom => {
                self.show_create_room = true;
                self.new_room_name.clear();
                self.room_creation_error = None;
                Task::none()
            }
            Message::HideCreateRoom => {
                self.show_create_room = false;
                Task::none()
            }
            Message::RoomNameChanged(name) => {
                self.new_room_name = name;
                self.room_creation_error = None;
                Task::none()
            }
            Message::CreateRoom => {
                // Validate room name
                let name = self.new_room_name.trim().to_lowercase();
                if name.is_empty() {
                    self.room_creation_error = Some("Room name cannot be empty".to_string());
                    return Task::none();
                }
                if !name
                    .chars()
                    .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
                {
                    self.room_creation_error = Some(
                        "Room name can only contain letters, numbers, hyphens and underscores"
                            .to_string(),
                    );
                    return Task::none();
                }

                if let Some(ref server) = self.server {
                    let client = server.matrix_client.clone();
                    let session_store = server.session_store.clone();
                    let room_name = name.clone();

                    return Task::perform(
                        async move {
                            use matrix_sdk::ruma::api::client::room::create_room::v3::Request as CreateRoomRequest;

                            // Create room request
                            let mut request = CreateRoomRequest::new();
                            request.name = Some(room_name.clone());
                            request.is_direct = false;

                            match client.create_room(request).await {
                                Ok(response) => {
                                    let room_id = response.room_id().to_string();
                                    // Create channel in session store
                                    match session_store.create_channel(&room_name, &room_id) {
                                        Ok(_) => Ok(room_id),
                                        Err(e) => Err(format!("Failed to create channel: {}", e)),
                                    }
                                }
                                Err(e) => Err(format!("Failed to create room: {}", e)),
                            }
                        },
                        Message::RoomCreated,
                    );
                }
                Task::none()
            }
            Message::RoomCreated(result) => {
                match result {
                    Ok(room_id) => {
                        tracing::info!(room_id = %room_id, "Room created successfully");
                        self.show_create_room = false;
                        // Refresh room list
                        return Task::done(Message::RefreshRooms);
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "Failed to create room");
                        self.room_creation_error = Some(e);
                    }
                }
                Task::none()
            }

            Message::Quit => {
                tracing::info!("Quit requested");
                std::process::exit(0);
            }
        }
    }

    fn view(&self) -> Element<'_, Message> {
        let main_content: Element<Message> = if let Some(ref error) = self.init_error {
            // Error state - styled
            container(
                container(
                    column![
                        text("⚠").size(48.0).color(colors::ACCENT_DANGER),
                        Space::with_height(spacing::MD),
                        text("Failed to start gorp")
                            .size(text_size::TITLE)
                            .color(colors::TEXT_PRIMARY),
                        Space::with_height(spacing::XS),
                        text(error.clone())
                            .size(text_size::BODY)
                            .color(colors::TEXT_SECONDARY),
                        Space::with_height(spacing::LG),
                        text("Check your config.toml and try again")
                            .size(text_size::SMALL)
                            .color(colors::TEXT_TERTIARY),
                    ]
                    .align_x(Alignment::Center)
                    .spacing(spacing::XXS),
                )
                .padding(spacing::XL)
                .style(theme::surface_style),
            )
            .style(content_style)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
        } else if self.server.is_none() {
            // Loading state - styled with animation hint
            container(
                column![
                    // Logo container
                    container(
                        text("G")
                            .size(text_size::DISPLAY)
                            .color(colors::TEXT_INVERSE),
                    )
                    .padding([spacing::MD, spacing::LG])
                    .style(|_theme| container::Style {
                        background: Some(colors::ACCENT_PRIMARY.into()),
                        border: Border {
                            radius: radius::LG.into(),
                            ..Default::default()
                        },
                        ..Default::default()
                    }),
                    Space::with_height(spacing::LG),
                    text("gorp")
                        .size(text_size::HEADING)
                        .color(colors::TEXT_PRIMARY),
                    Space::with_height(spacing::XS),
                    text("Connecting to Matrix...")
                        .size(text_size::BODY)
                        .color(colors::TEXT_SECONDARY),
                    Space::with_height(spacing::XXS),
                    text("Establishing secure connection")
                        .size(text_size::SMALL)
                        .color(colors::TEXT_TERTIARY),
                ]
                .align_x(Alignment::Center),
            )
            .style(content_style)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
        } else {
            // Main app layout with sidebar
            let sidebar = super::components::sidebar::view(&self.view, &self.rooms);
            let view_content: Element<Message> = match &self.view {
                View::Dashboard => views::dashboard::view(self.server.as_ref()),
                View::Chat { room_id } => views::chat::view(
                    room_id,
                    &self.current_room_name,
                    &self.chat_messages,
                    &self.chat_input,
                    &self.typing_users,
                    self.chat_loading,
                    self.connection_state == ConnectionState::Connected,
                ),
                View::Settings => views::settings::view(self.server.as_ref()),
                View::Schedules => views::schedules::view(
                    &self.schedules,
                    self.schedules_loading,
                    self.show_create_schedule,
                    &self.schedule_form_channel,
                    &self.schedule_form_prompt,
                    &self.schedule_form_time,
                    self.schedule_form_error.as_deref(),
                    &self.rooms,
                ),
                View::Logs => views::logs::view(
                    &self.log_entries,
                    self.logs_loading,
                    self.log_level_filter.as_deref(),
                ),
            };

            row![sidebar, view_content]
                .spacing(0)
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        };

        // Wrap with modals if visible
        if self.quick_prompt_visible {
            let modal = self.quick_prompt_modal();
            iced::widget::stack![main_content, modal].into()
        } else if self.show_create_room {
            let modal = self.create_room_modal();
            iced::widget::stack![main_content, modal].into()
        } else {
            main_content
        }
    }

    /// Render the quick prompt modal overlay
    fn quick_prompt_modal(&self) -> Element<'_, Message> {
        let input = text_input("Ask Claude anything...", &self.quick_prompt_input)
            .on_input(Message::QuickPromptInputChanged)
            .on_submit(Message::QuickPromptSubmit)
            .padding(spacing::SM)
            .size(text_size::LARGE)
            .width(Length::Fill)
            .style(text_input_style);

        let submit_btn = button(
            row![
                text("Send").size(text_size::BODY),
                Space::with_width(spacing::XS),
                text("↑").size(text_size::BODY),
            ]
            .align_y(Alignment::Center),
        )
        .on_press(Message::QuickPromptSubmit)
        .style(button_primary)
        .padding([spacing::SM, spacing::LG]);

        let cancel_btn = button(text("Cancel").size(text_size::BODY))
            .on_press(Message::ToggleQuickPrompt)
            .style(button_secondary)
            .padding([spacing::SM, spacing::LG]);

        let modal_content = container(
            column![
                // Header
                row![
                    text("⚡").size(text_size::TITLE).color(colors::ACCENT_WARM),
                    Space::with_width(spacing::SM),
                    column![
                        text("Quick Prompt")
                            .size(text_size::TITLE)
                            .color(colors::TEXT_PRIMARY),
                        text("Press Cmd+N to toggle")
                            .size(text_size::CAPTION)
                            .color(colors::TEXT_TERTIARY),
                    ]
                    .spacing(spacing::XXXS),
                ]
                .align_y(Alignment::Center),
                Space::with_height(spacing::LG),
                // Input
                input,
                Space::with_height(spacing::MD),
                // Buttons
                row![
                    Space::with_width(Length::Fill),
                    cancel_btn,
                    Space::with_width(spacing::SM),
                    submit_btn,
                ]
                .align_y(Alignment::Center),
            ]
            .padding(spacing::LG)
            .width(Length::Fixed(520.0)),
        )
        .style(modal_style);

        common::modal_frame(modal_content.into())
    }

    /// Render the create room modal overlay
    fn create_room_modal(&self) -> Element<'_, Message> {
        let input = text_input("Room name (e.g., my-project)", &self.new_room_name)
            .on_input(Message::RoomNameChanged)
            .on_submit(Message::CreateRoom)
            .padding(spacing::SM)
            .size(text_size::BODY)
            .width(Length::Fill)
            .style(text_input_style);

        let error_text: Element<'_, Message> = if let Some(ref error) = self.room_creation_error {
            text(error)
                .size(text_size::SMALL)
                .color(colors::ACCENT_DANGER)
                .into()
        } else {
            Space::with_height(text_size::SMALL).into()
        };

        let create_btn = button(text("Create Room").size(text_size::BODY))
            .on_press(Message::CreateRoom)
            .style(button_primary)
            .padding([spacing::SM, spacing::LG]);

        let cancel_btn = button(text("Cancel").size(text_size::BODY))
            .on_press(Message::HideCreateRoom)
            .style(button_secondary)
            .padding([spacing::SM, spacing::LG]);

        let modal_content = container(
            column![
                // Header
                row![
                    text("#")
                        .size(text_size::TITLE)
                        .color(colors::ACCENT_PRIMARY),
                    Space::with_width(spacing::SM),
                    column![
                        text("New Room")
                            .size(text_size::TITLE)
                            .color(colors::TEXT_PRIMARY),
                        text("Create a workspace room")
                            .size(text_size::CAPTION)
                            .color(colors::TEXT_TERTIARY),
                    ]
                    .spacing(spacing::XXXS),
                ]
                .align_y(Alignment::Center),
                Space::with_height(spacing::LG),
                // Name field
                text("Room Name")
                    .size(text_size::SMALL)
                    .color(colors::TEXT_SECONDARY),
                Space::with_height(spacing::XXS),
                input,
                Space::with_height(spacing::XS),
                text("Use lowercase letters, numbers, hyphens")
                    .size(text_size::CAPTION)
                    .color(colors::TEXT_TERTIARY),
                Space::with_height(spacing::XS),
                error_text,
                Space::with_height(spacing::MD),
                // Buttons
                row![
                    Space::with_width(Length::Fill),
                    cancel_btn,
                    Space::with_width(spacing::SM),
                    create_btn,
                ]
                .align_y(Alignment::Center),
            ]
            .padding(spacing::LG)
            .width(Length::Fixed(420.0)),
        )
        .style(modal_style);

        common::modal_frame(modal_content.into())
    }

    fn theme(&self) -> Theme {
        Theme::Dark
    }

    fn subscription(&self) -> Subscription<Message> {
        iced::time::every(std::time::Duration::from_millis(50)).map(|_| Message::Poll)
    }
}

/// Run the iced application
pub fn run(config: Config) -> anyhow::Result<()> {
    iced::application("gorp", GorpApp::update, GorpApp::view)
        .theme(GorpApp::theme)
        .subscription(GorpApp::subscription)
        .window_size((1200.0, 800.0))
        .run_with(move || GorpApp::new(config))?;

    Ok(())
}
