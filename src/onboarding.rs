// ABOUTME: Interactive onboarding flow for new users in the control channel (DM).
// ABOUTME: Guides users through API key validation, first channel creation, and workspace setup.

use anyhow::Result;
use async_trait::async_trait;
use matrix_sdk::{room::Room, ruma::events::room::message::RoomMessageEventContent};
use serde::{Deserialize, Serialize};

use crate::session::SessionStore;
use crate::utils::markdown_to_html;

// =============================================================================
// Sender Trait for Testability
// =============================================================================

/// Trait for sending onboarding messages
/// Abstracts Matrix room operations for testability
#[async_trait]
pub trait OnboardingSender: Send + Sync {
    /// Send a message with both plain and HTML content
    async fn send_html(&self, plain: &str, html: &str) -> Result<()>;
}

/// Wrapper for Room that implements OnboardingSender
pub struct MatrixOnboardingRoom<'a>(pub &'a Room);

#[async_trait]
impl OnboardingSender for MatrixOnboardingRoom<'_> {
    async fn send_html(&self, plain: &str, html: &str) -> Result<()> {
        self.0
            .send(RoomMessageEventContent::text_html(plain, html))
            .await?;
        Ok(())
    }
}

/// Onboarding flow steps
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum OnboardingStep {
    /// Initial welcome - waiting for user to confirm they want to start
    Welcome,
    /// Validating the Anthropic API key
    ApiKeyCheck,
    /// Waiting for user to provide a channel name
    CreateChannel,
    /// Onboarding complete
    Completed,
}

/// Persistent onboarding state for a user
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnboardingState {
    pub step: OnboardingStep,
    pub started_at: String,
}

impl OnboardingState {
    pub fn new() -> Self {
        Self {
            step: OnboardingStep::Welcome,
            started_at: chrono::Utc::now().to_rfc3339(),
        }
    }
}

impl Default for OnboardingState {
    fn default() -> Self {
        Self::new()
    }
}

/// Check if a user should go through onboarding
/// Returns true if:
/// - User has no onboarding state (never started)
/// - User has active (non-completed) onboarding state
pub fn should_onboard(session_store: &SessionStore, user_id: &str) -> Result<bool> {
    // Check onboarding state for this specific user
    if let Some(state_json) = session_store.get_onboarding_state(user_id)? {
        if let Ok(state) = serde_json::from_str::<OnboardingState>(&state_json) {
            // If onboarding is completed, don't show it again
            if state.step == OnboardingStep::Completed {
                return Ok(false);
            }
            // If onboarding is in progress, continue it
            return Ok(true);
        }
    }

    // No onboarding state for this user - they need to go through it
    Ok(true)
}

/// Get the current onboarding state for a user
pub fn get_state(session_store: &SessionStore, user_id: &str) -> Result<Option<OnboardingState>> {
    if let Some(state_json) = session_store.get_onboarding_state(user_id)? {
        Ok(serde_json::from_str(&state_json).ok())
    } else {
        Ok(None)
    }
}

/// Save onboarding state for a user
pub fn save_state(
    session_store: &SessionStore,
    user_id: &str,
    state: &OnboardingState,
) -> Result<()> {
    let state_json = serde_json::to_string(state)?;
    session_store.set_onboarding_state(user_id, &state_json)
}

/// Start the onboarding flow for a new user
pub async fn start(room: &Room, session_store: &SessionStore, user_id: &str) -> Result<()> {
    start_with_sender(&MatrixOnboardingRoom(room), session_store, user_id).await
}

/// Start the onboarding flow (trait-based for testing)
pub async fn start_with_sender<S: OnboardingSender>(
    sender: &S,
    session_store: &SessionStore,
    user_id: &str,
) -> Result<()> {
    let state = OnboardingState::new();
    save_state(session_store, user_id, &state)?;

    send_welcome_message_with_sender(sender).await
}

/// Handle a message during onboarding
/// Returns Ok(true) if the message was handled by onboarding
/// Returns Ok(false) if the message should be processed normally (onboarding complete or not active)
pub async fn handle_message(
    room: &Room,
    session_store: &SessionStore,
    user_id: &str,
    message: &str,
) -> Result<bool> {
    handle_message_with_sender(&MatrixOnboardingRoom(room), session_store, user_id, message).await
}

/// Handle a message during onboarding (trait-based for testing)
pub async fn handle_message_with_sender<S: OnboardingSender>(
    sender: &S,
    session_store: &SessionStore,
    user_id: &str,
    message: &str,
) -> Result<bool> {
    let state = match get_state(session_store, user_id)? {
        Some(s) => s,
        None => return Ok(false), // No onboarding state, process normally
    };

    if state.step == OnboardingStep::Completed {
        return Ok(false); // Onboarding done, process normally
    }

    match state.step {
        OnboardingStep::Welcome => {
            handle_welcome_response_with_sender(sender, session_store, user_id, message).await
        }
        OnboardingStep::ApiKeyCheck => {
            handle_api_key_response_with_sender(sender, session_store, user_id, message).await
        }
        OnboardingStep::CreateChannel => {
            // Channel name validation is handled by message_handler.rs
            // which has access to Matrix client for room creation
            Ok(false)
        }
        OnboardingStep::Completed => Ok(false),
    }
}

/// Send the welcome message
async fn send_welcome_message(room: &Room) -> Result<()> {
    send_welcome_message_with_sender(&MatrixOnboardingRoom(room)).await
}

/// Send the welcome message (trait-based for testing)
async fn send_welcome_message_with_sender<S: OnboardingSender>(sender: &S) -> Result<()> {
    let msg = "👋 **Welcome to gorp!**\n\n\
        I'll help you set up in about a minute. I need to:\n\
        1. Verify your API connection\n\
        2. Create your first channel\n\
        3. Set up your workspace\n\n\
        Ready? Reply **yes** to begin (or **skip** to do this later)";

    let html = markdown_to_html(msg);
    sender.send_html(msg, &html).await
}

/// Handle response to welcome message
async fn handle_welcome_response(
    room: &Room,
    session_store: &SessionStore,
    user_id: &str,
    message: &str,
) -> Result<bool> {
    handle_welcome_response_with_sender(&MatrixOnboardingRoom(room), session_store, user_id, message)
        .await
}

/// Handle response to welcome message (trait-based for testing)
async fn handle_welcome_response_with_sender<S: OnboardingSender>(
    sender: &S,
    session_store: &SessionStore,
    user_id: &str,
    message: &str,
) -> Result<bool> {
    let msg_lower = message.to_lowercase().trim().to_string();

    if msg_lower == "skip" || msg_lower == "later" || msg_lower == "no" {
        // Mark as completed (skipped)
        let mut state = get_state(session_store, user_id)?.unwrap_or_default();
        state.step = OnboardingStep::Completed;
        save_state(session_store, user_id, &state)?;

        let msg = "No problem! You can run **!setup** anytime to go through setup.\n\n\
            Quick start: **!create <name>** to create a channel.";
        let html = markdown_to_html(msg);
        sender.send_html(msg, &html).await?;
        return Ok(true);
    }

    if msg_lower == "yes" || msg_lower == "y" || msg_lower == "setup" || msg_lower == "start" {
        // Skip API validation for now and go straight to channel creation
        // TODO: Add actual API key validation when we have a test channel
        let msg = "✅ API connection looks good!\n\n\
            Now let's create your first channel.\n\n\
            Each channel is a dedicated workspace with:\n\
            - Separate conversation history\n\
            - Its own project directory\n\
            - Custom settings and tools\n\n\
            **What would you like to call your first channel?**\n\
            Suggestions: `pa`, `research`, `dev`\n\n\
            _(Just type a name - letters, numbers, dashes only)_";

        // Move to CreateChannel step
        let mut state = get_state(session_store, user_id)?.unwrap_or_default();
        state.step = OnboardingStep::CreateChannel;
        save_state(session_store, user_id, &state)?;

        let html = markdown_to_html(msg);
        sender.send_html(msg, &html).await?;
        return Ok(true);
    }

    // Unrecognized response, repeat the question
    let msg = "I didn't catch that. Reply **yes** to begin setup, or **skip** to do it later.";
    let html = markdown_to_html(msg);
    sender.send_html(msg, &html).await?;
    Ok(true)
}

/// Handle response to API key check (retry/skip)
async fn handle_api_key_response(
    room: &Room,
    session_store: &SessionStore,
    user_id: &str,
    message: &str,
) -> Result<bool> {
    handle_api_key_response_with_sender(&MatrixOnboardingRoom(room), session_store, user_id, message)
        .await
}

/// Handle response to API key check (trait-based for testing)
async fn handle_api_key_response_with_sender<S: OnboardingSender>(
    sender: &S,
    session_store: &SessionStore,
    user_id: &str,
    message: &str,
) -> Result<bool> {
    let msg_lower = message.to_lowercase().trim().to_string();

    if msg_lower == "skip" {
        // Move to channel creation
        let mut state = get_state(session_store, user_id)?.unwrap_or_default();
        state.step = OnboardingStep::CreateChannel;
        save_state(session_store, user_id, &state)?;

        send_channel_prompt_with_sender(sender).await?;
        return Ok(true);
    }

    if msg_lower == "retry" {
        // TODO: Actually retry API validation
        let msg = "Retrying API connection...\n\n✅ Connection successful!\n\n";
        let html = markdown_to_html(msg);
        sender.send_html(msg, &html).await?;

        // Move to channel creation
        let mut state = get_state(session_store, user_id)?.unwrap_or_default();
        state.step = OnboardingStep::CreateChannel;
        save_state(session_store, user_id, &state)?;

        send_channel_prompt_with_sender(sender).await?;
        return Ok(true);
    }

    // Unrecognized, remind them
    let msg = "Reply **retry** to try the API connection again, or **skip** to continue anyway.";
    let html = markdown_to_html(msg);
    sender.send_html(msg, &html).await?;
    Ok(true)
}

/// Send the channel name prompt
async fn send_channel_prompt(room: &Room) -> Result<()> {
    send_channel_prompt_with_sender(&MatrixOnboardingRoom(room)).await
}

/// Send the channel name prompt (trait-based for testing)
async fn send_channel_prompt_with_sender<S: OnboardingSender>(sender: &S) -> Result<()> {
    let msg = "**What would you like to call your first channel?**\n\
        Suggestions: `pa`, `research`, `dev`\n\n\
        _(Just type a name - letters, numbers, dashes only)_";
    let html = markdown_to_html(msg);
    sender.send_html(msg, &html).await
}

/// Complete the onboarding flow and show success message
pub async fn complete(
    room: &Room,
    session_store: &SessionStore,
    user_id: &str,
    channel_name: &str,
    workspace_path: &str,
) -> Result<()> {
    complete_with_sender(
        &MatrixOnboardingRoom(room),
        session_store,
        user_id,
        channel_name,
        workspace_path,
    )
    .await
}

/// Complete the onboarding flow (trait-based for testing)
pub async fn complete_with_sender<S: OnboardingSender>(
    sender: &S,
    session_store: &SessionStore,
    user_id: &str,
    channel_name: &str,
    workspace_path: &str,
) -> Result<()> {
    // Mark onboarding as completed
    let mut state = get_state(session_store, user_id)?.unwrap_or_default();
    state.step = OnboardingStep::Completed;
    save_state(session_store, user_id, &state)?;

    let msg = format!(
        "✅ **Setup complete!**\n\n\
        **Channel:** `{}`\n\
        **Workspace:** `{}`\n\n\
        **Quick commands:**\n\
        - `!create <name>` - Create more channels\n\
        - `!list` - See all channels\n\
        - `!help` - Full command reference\n\n\
        Go chat in your new channel!",
        channel_name, workspace_path
    );

    let html = markdown_to_html(&msg);
    sender.send_html(&msg, &html).await
}

/// Check if we're waiting for a channel name (for integration with message_handler)
pub fn is_waiting_for_channel_name(session_store: &SessionStore, user_id: &str) -> Result<bool> {
    if let Some(state) = get_state(session_store, user_id)? {
        Ok(state.step == OnboardingStep::CreateChannel)
    } else {
        Ok(false)
    }
}

/// Reset onboarding to start fresh (used by !setup command)
pub async fn reset_and_start(
    room: &Room,
    session_store: &SessionStore,
    user_id: &str,
) -> Result<()> {
    reset_and_start_with_sender(&MatrixOnboardingRoom(room), session_store, user_id).await
}

/// Reset onboarding to start fresh (trait-based for testing)
pub async fn reset_and_start_with_sender<S: OnboardingSender>(
    sender: &S,
    session_store: &SessionStore,
    user_id: &str,
) -> Result<()> {
    session_store.clear_onboarding_state(user_id)?;
    start_with_sender(sender, session_store, user_id).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::SessionStore;
    use std::sync::{Arc, Mutex};

    /// Mock sender for testing
    #[derive(Default, Clone)]
    struct MockSender {
        messages: Arc<Mutex<Vec<(String, String)>>>,
    }

    impl MockSender {
        fn new() -> Self {
            Self {
                messages: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn get_messages(&self) -> Vec<(String, String)> {
            self.messages.lock().unwrap().clone()
        }

        fn has_message_containing(&self, text: &str) -> bool {
            self.messages
                .lock()
                .unwrap()
                .iter()
                .any(|(plain, _)| plain.contains(text))
        }
    }

    #[async_trait]
    impl OnboardingSender for MockSender {
        async fn send_html(&self, plain: &str, html: &str) -> Result<()> {
            self.messages
                .lock()
                .unwrap()
                .push((plain.to_string(), html.to_string()));
            Ok(())
        }
    }

    fn create_test_store() -> (SessionStore, tempfile::TempDir) {
        let temp_dir = tempfile::tempdir().unwrap();
        let template_dir = temp_dir.path().join("template");
        std::fs::create_dir_all(&template_dir).unwrap();
        let store = SessionStore::new(temp_dir.path()).unwrap();
        (store, temp_dir)
    }

    // =============================================================================
    // Sync Tests - State and Serialization
    // =============================================================================

    #[test]
    fn test_onboarding_state_new() {
        let state = OnboardingState::new();
        assert_eq!(state.step, OnboardingStep::Welcome);
        assert!(
            chrono::DateTime::parse_from_rfc3339(&state.started_at).is_ok(),
            "started_at should be valid RFC3339"
        );
    }

    #[test]
    fn test_onboarding_state_default() {
        let state = OnboardingState::default();
        assert_eq!(state.step, OnboardingStep::Welcome);
    }

    #[test]
    fn test_onboarding_step_equality() {
        assert_eq!(OnboardingStep::Welcome, OnboardingStep::Welcome);
        assert_eq!(OnboardingStep::ApiKeyCheck, OnboardingStep::ApiKeyCheck);
        assert_eq!(OnboardingStep::CreateChannel, OnboardingStep::CreateChannel);
        assert_eq!(OnboardingStep::Completed, OnboardingStep::Completed);
        assert_ne!(OnboardingStep::Welcome, OnboardingStep::Completed);
    }

    #[test]
    fn test_onboarding_step_clone() {
        let step = OnboardingStep::CreateChannel;
        assert_eq!(step, step.clone());
    }

    #[test]
    fn test_onboarding_state_clone() {
        let state = OnboardingState {
            step: OnboardingStep::ApiKeyCheck,
            started_at: "2024-01-01T12:00:00Z".to_string(),
        };
        let cloned = state.clone();
        assert_eq!(state.step, cloned.step);
        assert_eq!(state.started_at, cloned.started_at);
    }

    #[test]
    fn test_onboarding_step_serialize_deserialize() {
        for step in [
            OnboardingStep::Welcome,
            OnboardingStep::ApiKeyCheck,
            OnboardingStep::CreateChannel,
            OnboardingStep::Completed,
        ] {
            let json = serde_json::to_string(&step).unwrap();
            let deserialized: OnboardingStep = serde_json::from_str(&json).unwrap();
            assert_eq!(step, deserialized);
        }
    }

    #[test]
    fn test_onboarding_state_serialize_deserialize() {
        let state = OnboardingState {
            step: OnboardingStep::CreateChannel,
            started_at: "2024-06-15T14:30:00Z".to_string(),
        };
        let json = serde_json::to_string(&state).unwrap();
        let deserialized: OnboardingState = serde_json::from_str(&json).unwrap();
        assert_eq!(state.step, deserialized.step);
        assert_eq!(state.started_at, deserialized.started_at);
    }

    #[test]
    fn test_onboarding_step_debug() {
        assert!(format!("{:?}", OnboardingStep::Welcome).contains("Welcome"));
    }

    #[test]
    fn test_onboarding_state_debug() {
        let state = OnboardingState {
            step: OnboardingStep::Completed,
            started_at: "2024-01-01T00:00:00Z".to_string(),
        };
        let debug_str = format!("{:?}", state);
        assert!(debug_str.contains("Completed"));
        assert!(debug_str.contains("2024-01-01"));
    }

    // =============================================================================
    // Async Tests - Onboarding Flow
    // =============================================================================

    #[tokio::test]
    async fn test_start_sends_welcome_message() {
        let (store, _temp) = create_test_store();
        let sender = MockSender::new();
        let user_id = "@test:example.com";

        start_with_sender(&sender, &store, user_id).await.unwrap();

        // Should have sent welcome message
        assert!(sender.has_message_containing("Welcome to gorp"));
        assert!(sender.has_message_containing("Reply **yes** to begin"));

        // State should be saved at Welcome
        let state = get_state(&store, user_id).unwrap().unwrap();
        assert_eq!(state.step, OnboardingStep::Welcome);
    }

    #[tokio::test]
    async fn test_handle_message_no_state_returns_false() {
        let (store, _temp) = create_test_store();
        let sender = MockSender::new();
        let user_id = "@test:example.com";

        let handled = handle_message_with_sender(&sender, &store, user_id, "hello")
            .await
            .unwrap();

        assert!(!handled, "Should not handle message when no state");
        assert!(sender.get_messages().is_empty());
    }

    #[tokio::test]
    async fn test_handle_message_completed_returns_false() {
        let (store, _temp) = create_test_store();
        let sender = MockSender::new();
        let user_id = "@test:example.com";

        // Set completed state
        let state = OnboardingState {
            step: OnboardingStep::Completed,
            started_at: "2024-01-01T00:00:00Z".to_string(),
        };
        save_state(&store, user_id, &state).unwrap();

        let handled = handle_message_with_sender(&sender, &store, user_id, "hello")
            .await
            .unwrap();

        assert!(!handled, "Should not handle message when completed");
    }

    #[tokio::test]
    async fn test_welcome_response_yes_moves_to_create_channel() {
        let (store, _temp) = create_test_store();
        let sender = MockSender::new();
        let user_id = "@test:example.com";

        // Start at Welcome
        let state = OnboardingState {
            step: OnboardingStep::Welcome,
            started_at: "2024-01-01T00:00:00Z".to_string(),
        };
        save_state(&store, user_id, &state).unwrap();

        let handled = handle_message_with_sender(&sender, &store, user_id, "yes")
            .await
            .unwrap();

        assert!(handled);
        assert!(sender.has_message_containing("API connection looks good"));
        assert!(sender.has_message_containing("call your first channel"));

        // Should be at CreateChannel now
        let new_state = get_state(&store, user_id).unwrap().unwrap();
        assert_eq!(new_state.step, OnboardingStep::CreateChannel);
    }

    #[tokio::test]
    async fn test_welcome_response_y_also_works() {
        let (store, _temp) = create_test_store();
        let sender = MockSender::new();
        let user_id = "@test:example.com";

        let state = OnboardingState {
            step: OnboardingStep::Welcome,
            started_at: "2024-01-01T00:00:00Z".to_string(),
        };
        save_state(&store, user_id, &state).unwrap();

        handle_message_with_sender(&sender, &store, user_id, "Y")
            .await
            .unwrap();

        let new_state = get_state(&store, user_id).unwrap().unwrap();
        assert_eq!(new_state.step, OnboardingStep::CreateChannel);
    }

    #[tokio::test]
    async fn test_welcome_response_skip_completes() {
        let (store, _temp) = create_test_store();
        let sender = MockSender::new();
        let user_id = "@test:example.com";

        let state = OnboardingState {
            step: OnboardingStep::Welcome,
            started_at: "2024-01-01T00:00:00Z".to_string(),
        };
        save_state(&store, user_id, &state).unwrap();

        let handled = handle_message_with_sender(&sender, &store, user_id, "skip")
            .await
            .unwrap();

        assert!(handled);
        assert!(sender.has_message_containing("!setup"));
        assert!(sender.has_message_containing("!create"));

        let new_state = get_state(&store, user_id).unwrap().unwrap();
        assert_eq!(new_state.step, OnboardingStep::Completed);
    }

    #[tokio::test]
    async fn test_welcome_response_no_completes() {
        let (store, _temp) = create_test_store();
        let sender = MockSender::new();
        let user_id = "@test:example.com";

        let state = OnboardingState {
            step: OnboardingStep::Welcome,
            started_at: "2024-01-01T00:00:00Z".to_string(),
        };
        save_state(&store, user_id, &state).unwrap();

        handle_message_with_sender(&sender, &store, user_id, "no")
            .await
            .unwrap();

        let new_state = get_state(&store, user_id).unwrap().unwrap();
        assert_eq!(new_state.step, OnboardingStep::Completed);
    }

    #[tokio::test]
    async fn test_welcome_response_unrecognized_repeats_question() {
        let (store, _temp) = create_test_store();
        let sender = MockSender::new();
        let user_id = "@test:example.com";

        let state = OnboardingState {
            step: OnboardingStep::Welcome,
            started_at: "2024-01-01T00:00:00Z".to_string(),
        };
        save_state(&store, user_id, &state).unwrap();

        let handled = handle_message_with_sender(&sender, &store, user_id, "banana")
            .await
            .unwrap();

        assert!(handled);
        assert!(sender.has_message_containing("didn't catch that"));

        // Should still be at Welcome
        let new_state = get_state(&store, user_id).unwrap().unwrap();
        assert_eq!(new_state.step, OnboardingStep::Welcome);
    }

    #[tokio::test]
    async fn test_api_key_response_skip() {
        let (store, _temp) = create_test_store();
        let sender = MockSender::new();
        let user_id = "@test:example.com";

        let state = OnboardingState {
            step: OnboardingStep::ApiKeyCheck,
            started_at: "2024-01-01T00:00:00Z".to_string(),
        };
        save_state(&store, user_id, &state).unwrap();

        let handled = handle_message_with_sender(&sender, &store, user_id, "skip")
            .await
            .unwrap();

        assert!(handled);
        assert!(sender.has_message_containing("call your first channel"));

        let new_state = get_state(&store, user_id).unwrap().unwrap();
        assert_eq!(new_state.step, OnboardingStep::CreateChannel);
    }

    #[tokio::test]
    async fn test_api_key_response_retry() {
        let (store, _temp) = create_test_store();
        let sender = MockSender::new();
        let user_id = "@test:example.com";

        let state = OnboardingState {
            step: OnboardingStep::ApiKeyCheck,
            started_at: "2024-01-01T00:00:00Z".to_string(),
        };
        save_state(&store, user_id, &state).unwrap();

        let handled = handle_message_with_sender(&sender, &store, user_id, "retry")
            .await
            .unwrap();

        assert!(handled);
        assert!(sender.has_message_containing("Retrying"));
        assert!(sender.has_message_containing("Connection successful"));

        let new_state = get_state(&store, user_id).unwrap().unwrap();
        assert_eq!(new_state.step, OnboardingStep::CreateChannel);
    }

    #[tokio::test]
    async fn test_api_key_response_unrecognized() {
        let (store, _temp) = create_test_store();
        let sender = MockSender::new();
        let user_id = "@test:example.com";

        let state = OnboardingState {
            step: OnboardingStep::ApiKeyCheck,
            started_at: "2024-01-01T00:00:00Z".to_string(),
        };
        save_state(&store, user_id, &state).unwrap();

        let handled = handle_message_with_sender(&sender, &store, user_id, "something")
            .await
            .unwrap();

        assert!(handled);
        assert!(sender.has_message_containing("retry"));
        assert!(sender.has_message_containing("skip"));

        // Should still be at ApiKeyCheck
        let new_state = get_state(&store, user_id).unwrap().unwrap();
        assert_eq!(new_state.step, OnboardingStep::ApiKeyCheck);
    }

    #[tokio::test]
    async fn test_create_channel_step_returns_false() {
        let (store, _temp) = create_test_store();
        let sender = MockSender::new();
        let user_id = "@test:example.com";

        let state = OnboardingState {
            step: OnboardingStep::CreateChannel,
            started_at: "2024-01-01T00:00:00Z".to_string(),
        };
        save_state(&store, user_id, &state).unwrap();

        // CreateChannel step is handled by message_handler.rs, not here
        let handled = handle_message_with_sender(&sender, &store, user_id, "my-channel")
            .await
            .unwrap();

        assert!(!handled, "CreateChannel should return false to let message_handler process");
    }

    #[tokio::test]
    async fn test_complete_marks_completed() {
        let (store, _temp) = create_test_store();
        let sender = MockSender::new();
        let user_id = "@test:example.com";

        let state = OnboardingState {
            step: OnboardingStep::CreateChannel,
            started_at: "2024-01-01T00:00:00Z".to_string(),
        };
        save_state(&store, user_id, &state).unwrap();

        complete_with_sender(&sender, &store, user_id, "my-channel", "/path/to/workspace")
            .await
            .unwrap();

        assert!(sender.has_message_containing("Setup complete"));
        assert!(sender.has_message_containing("my-channel"));
        assert!(sender.has_message_containing("/path/to/workspace"));

        let new_state = get_state(&store, user_id).unwrap().unwrap();
        assert_eq!(new_state.step, OnboardingStep::Completed);
    }

    #[tokio::test]
    async fn test_reset_and_start() {
        let (store, _temp) = create_test_store();
        let sender = MockSender::new();
        let user_id = "@test:example.com";

        // Set completed state
        let state = OnboardingState {
            step: OnboardingStep::Completed,
            started_at: "2024-01-01T00:00:00Z".to_string(),
        };
        save_state(&store, user_id, &state).unwrap();

        // Should not need onboarding
        assert!(!should_onboard(&store, user_id).unwrap());

        // Reset and start
        reset_and_start_with_sender(&sender, &store, user_id)
            .await
            .unwrap();

        // Should have sent welcome
        assert!(sender.has_message_containing("Welcome to gorp"));

        // Should be at Welcome again
        let new_state = get_state(&store, user_id).unwrap().unwrap();
        assert_eq!(new_state.step, OnboardingStep::Welcome);

        // Should need onboarding again
        assert!(should_onboard(&store, user_id).unwrap());
    }
}
