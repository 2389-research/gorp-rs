// ABOUTME: Global hotkey registration and handling for gorp desktop
// ABOUTME: Cmd+N triggers quick prompt from anywhere on the system

use global_hotkey::{
    hotkey::{Code, HotKey, Modifiers},
    GlobalHotKeyEvent, GlobalHotKeyManager,
};

/// Hotkey identifiers
pub mod hotkey_ids {
    pub const QUICK_PROMPT: u32 = 1;
}

/// Global hotkey manager wrapper
pub struct HotkeyManager {
    _manager: GlobalHotKeyManager,
    quick_prompt_id: u32,
}

impl HotkeyManager {
    /// Create and register global hotkeys
    pub fn new() -> Result<Self, global_hotkey::Error> {
        let manager = GlobalHotKeyManager::new()?;

        // Register Cmd+N for quick prompt (Meta = Cmd on macOS)
        let quick_prompt_hotkey = HotKey::new(Some(Modifiers::META), Code::KeyN);
        let quick_prompt_id = quick_prompt_hotkey.id();
        manager.register(quick_prompt_hotkey)?;

        tracing::info!("Global hotkey Cmd+N registered for quick prompt");

        Ok(Self {
            _manager: manager,
            quick_prompt_id,
        })
    }

    /// Get the quick prompt hotkey ID for event matching
    pub fn quick_prompt_id(&self) -> u32 {
        self.quick_prompt_id
    }
}

/// Poll for hotkey events (non-blocking)
pub fn poll_hotkey_event() -> Option<GlobalHotKeyEvent> {
    GlobalHotKeyEvent::receiver().try_recv().ok()
}

/// Check if an event matches the quick prompt hotkey
pub fn is_quick_prompt_event(event: &GlobalHotKeyEvent, manager: &HotkeyManager) -> bool {
    event.id == manager.quick_prompt_id()
}
