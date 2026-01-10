// ABOUTME: System tray icon component for gorp desktop
// ABOUTME: Menu bar integration with status display and quick actions

use tray_icon::{
    menu::{accelerator::Accelerator, Menu, MenuEvent, MenuItem, PredefinedMenuItem},
    Icon, TrayIcon, TrayIconBuilder,
};

// Note: Accelerator is imported for type annotations but not used directly
// since tray menu accelerators are display-only on macOS

/// Connection state for tray icon display
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    Connected,
    Connecting,
    Disconnected,
}

/// Menu item IDs for event handling
pub mod menu_ids {
    pub const OPEN_DASHBOARD: &str = "open_dashboard";
    pub const QUICK_PROMPT: &str = "quick_prompt";
    pub const SETTINGS: &str = "settings";
    pub const QUIT: &str = "quit";
}

/// Create the tray icon with menu
pub fn create_tray_icon() -> Result<TrayIcon, tray_icon::Error> {
    let menu = build_menu();
    let icon = create_icon(ConnectionState::Connecting);

    TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("gorp - Matrix-Claude Bridge")
        .with_icon(icon)
        .build()
}

/// Build the tray context menu
fn build_menu() -> Menu {
    let menu = Menu::new();

    // Status section (will be updated dynamically)
    let status_item = MenuItem::with_id(
        "status",
        "Connecting...",
        false, // disabled - just for display
        None,
    );
    let _ = menu.append(&status_item);
    let _ = menu.append(&PredefinedMenuItem::separator());

    // Quick actions - no accelerators for tray menu (they're display-only hints anyway)
    let dashboard_item = MenuItem::with_id(
        menu_ids::OPEN_DASHBOARD,
        "Open Dashboard",
        true,
        None::<Accelerator>,
    );
    let _ = menu.append(&dashboard_item);

    let quick_prompt_item = MenuItem::with_id(
        menu_ids::QUICK_PROMPT,
        "Quick Prompt...",
        true,
        None::<Accelerator>,
    );
    let _ = menu.append(&quick_prompt_item);

    let _ = menu.append(&PredefinedMenuItem::separator());

    // Settings
    let settings_item = MenuItem::with_id(
        menu_ids::SETTINGS,
        "Settings...",
        true,
        None::<Accelerator>,
    );
    let _ = menu.append(&settings_item);

    let _ = menu.append(&PredefinedMenuItem::separator());

    // Quit
    let quit_item = MenuItem::with_id(
        menu_ids::QUIT,
        "Quit gorp",
        true,
        None::<Accelerator>,
    );
    let _ = menu.append(&quit_item);

    menu
}

/// Create an icon for the given connection state
/// For now, returns a simple colored icon. In production, use proper .icns/.png files.
fn create_icon(_state: ConnectionState) -> Icon {
    // Create a simple 22x22 icon (standard macOS menu bar size)
    // In production, load from Resources/gorp.icns
    let size = 22;
    let mut rgba = Vec::with_capacity(size * size * 4);

    // Simple circle icon - color based on state
    let (r, g, b) = match _state {
        ConnectionState::Connected => (0x89, 0xb4, 0xfa), // Blue (connected)
        ConnectionState::Connecting => (0xf9, 0xe2, 0xaf), // Yellow (connecting)
        ConnectionState::Disconnected => (0xf3, 0x8b, 0xa8), // Red (disconnected)
    };

    let center = size as f32 / 2.0;
    let radius = size as f32 / 2.0 - 2.0;

    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 - center;
            let dy = y as f32 - center;
            let dist = (dx * dx + dy * dy).sqrt();

            if dist <= radius {
                // Inside circle
                rgba.extend_from_slice(&[r, g, b, 255]);
            } else {
                // Outside circle (transparent)
                rgba.extend_from_slice(&[0, 0, 0, 0]);
            }
        }
    }

    Icon::from_rgba(rgba, size as u32, size as u32).expect("Failed to create icon")
}

/// Update the tray icon based on connection state
pub fn update_icon(tray: &TrayIcon, state: ConnectionState) {
    let icon = create_icon(state);
    if let Err(e) = tray.set_icon(Some(icon)) {
        tracing::warn!(error = %e, "Failed to update tray icon");
    }
}

/// Update the status text in the menu
pub fn update_status(tray: &TrayIcon, status: &str) {
    // Note: tray-icon/muda doesn't have a direct way to update menu item text
    // after creation. For now, we'll rebuild the menu or use tooltip.
    if let Err(e) = tray.set_tooltip(Some(status)) {
        tracing::warn!(error = %e, "Failed to update tray tooltip");
    }
}

/// Poll for menu events (non-blocking)
pub fn poll_menu_event() -> Option<MenuEvent> {
    MenuEvent::receiver().try_recv().ok()
}
