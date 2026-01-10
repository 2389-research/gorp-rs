// ABOUTME: View modules for different screens in the gorp desktop app
// ABOUTME: Each view is a function that returns an iced Element

pub mod chat;
pub mod dashboard;
pub mod logs;
pub mod schedules;
pub mod settings;

/// Current view/screen in the app
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum View {
    Dashboard,
    Chat { room_id: String },
    Settings,
    Schedules,
    Logs,
}
