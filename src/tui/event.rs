// ABOUTME: TUI event system merging keyboard, tick, and platform events
// ABOUTME: Three async event sources feed into a single mpsc channel

use crossterm::event::{self, Event, KeyEvent, KeyEventKind};
use std::time::Duration;
use tokio::sync::mpsc;

use super::app::FeedMessage;

// =============================================================================
// TuiEvent — unified event type for the TUI event loop
// =============================================================================

#[derive(Debug)]
pub enum TuiEvent {
    /// Keyboard input from crossterm
    Key(KeyEvent),
    /// Periodic render tick (100ms)
    Tick,
    /// Incoming message from any platform
    PlatformMessage(FeedMessage),
    /// Platform connection status change
    PlatformStatus { name: String, connected: bool },
}

// =============================================================================
// Event source spawning
// =============================================================================

/// Spawn all event source tasks. Each task sends TuiEvents to the provided channel.
pub fn spawn_event_tasks(tx: mpsc::Sender<TuiEvent>) {
    // Keyboard input task
    spawn_keyboard_task(tx.clone());

    // Tick task (100ms render interval)
    spawn_tick_task(tx);

    // Platform event bridging is handled externally when PlatformRegistry is wired
}

/// Spawn keyboard input polling task
fn spawn_keyboard_task(tx: mpsc::Sender<TuiEvent>) {
    tokio::spawn(async move {
        loop {
            // crossterm event polling is blocking, run in spawn_blocking
            let event = tokio::task::spawn_blocking(|| {
                if event::poll(Duration::from_millis(50)).unwrap_or(false) {
                    event::read().ok()
                } else {
                    None
                }
            })
            .await;

            match event {
                Ok(Some(Event::Key(key))) if key.kind == KeyEventKind::Press => {
                    if tx.send(TuiEvent::Key(key)).await.is_err() {
                        break;
                    }
                }
                Ok(_) => {} // Mouse events, resize, etc — ignore for now
                Err(_) => break,
            }
        }
    });
}

/// Spawn tick task for periodic re-renders
fn spawn_tick_task(tx: mpsc::Sender<TuiEvent>) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(100));
        loop {
            interval.tick().await;
            if tx.send(TuiEvent::Tick).await.is_err() {
                break;
            }
        }
    });
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tui_event_debug() {
        let event = TuiEvent::Tick;
        assert_eq!(format!("{:?}", event), "Tick");
    }

    #[test]
    fn test_tui_event_platform_message() {
        let msg = FeedMessage {
            platform_id: "matrix".to_string(),
            channel_name: "#test".to_string(),
            sender: "user".to_string(),
            body: "hello".to_string(),
            timestamp: 12345,
            channel_id: "!abc:matrix.org".to_string(),
            is_bot: false,
        };
        let event = TuiEvent::PlatformMessage(msg);
        assert!(format!("{:?}", event).contains("PlatformMessage"));
    }

    #[test]
    fn test_tui_event_platform_status() {
        let event = TuiEvent::PlatformStatus {
            name: "matrix".to_string(),
            connected: true,
        };
        assert!(format!("{:?}", event).contains("PlatformStatus"));
    }
}
