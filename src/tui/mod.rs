// ABOUTME: TUI module entry point with terminal setup, teardown, and panic hook
// ABOUTME: Provides run_tui() for starting the terminal interface via `gorp tui`

pub mod app;
pub mod event;
pub mod sidebar;
pub mod theme;

use anyhow::Result;
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::prelude::*;
use std::io;

/// Run the TUI application. Entry point for `gorp tui` command.
pub async fn run_tui() -> Result<()> {
    // Setup terminal
    let terminal = setup_terminal()?;

    // Run the app
    let result = run_app(terminal).await;

    // Restore terminal regardless of result
    restore_terminal()?;

    result
}

/// Initialize the terminal for TUI rendering
fn setup_terminal() -> Result<Terminal<CrosstermBackend<io::Stdout>>> {
    // Install panic hook to restore terminal on panic
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        let _ = restore_terminal();
        original_hook(panic_info);
    }));

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let terminal = Terminal::new(backend)?;

    Ok(terminal)
}

/// Restore the terminal to its original state
fn restore_terminal() -> Result<()> {
    disable_raw_mode()?;
    execute!(
        io::stdout(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    Ok(())
}

/// Main TUI application loop
async fn run_app(mut terminal: Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
    let (event_tx, mut event_rx) = tokio::sync::mpsc::channel(256);
    let mut app = app::TuiApp::new();

    // Start event collection tasks
    event::spawn_event_tasks(event_tx);

    loop {
        // Render
        terminal.draw(|frame| app.render(frame))?;

        // Handle events
        if let Some(event) = event_rx.recv().await {
            match app.handle_event(event) {
                app::EventResult::Continue => {}
                app::EventResult::Quit => break,
            }
        } else {
            // All event senders dropped
            break;
        }
    }

    Ok(())
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tui_module_exports() {
        // Verify all submodules are accessible
        let _view = app::View::Dashboard;
        let _event = event::TuiEvent::Tick;
        let _color = theme::platform_color("matrix");
    }
}
