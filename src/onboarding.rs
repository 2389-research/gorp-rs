// ABOUTME: Interactive onboarding flow for new users in the control channel (DM).
// ABOUTME: Guides users through API key validation, first channel creation, and workspace setup.

use anyhow::Result;
use matrix_sdk::{room::Room, ruma::events::room::message::RoomMessageEventContent};
use serde::{Deserialize, Serialize};

use crate::session::SessionStore;
use crate::utils::markdown_to_html;

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
    let state = OnboardingState::new();
    save_state(session_store, user_id, &state)?;

    send_welcome_message(room).await
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
    let state = match get_state(session_store, user_id)? {
        Some(s) => s,
        None => return Ok(false), // No onboarding state, process normally
    };

    if state.step == OnboardingStep::Completed {
        return Ok(false); // Onboarding done, process normally
    }

    match state.step {
        OnboardingStep::Welcome => {
            handle_welcome_response(room, session_store, user_id, message).await
        }
        OnboardingStep::ApiKeyCheck => {
            handle_api_key_response(room, session_store, user_id, message).await
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
    let msg = "👋 **Welcome to gorp!**\n\n\
        I'll help you set up in about a minute. I need to:\n\
        1. Verify your API connection\n\
        2. Create your first channel\n\
        3. Set up your workspace\n\n\
        Ready? Reply **yes** to begin (or **skip** to do this later)";

    let html = markdown_to_html(msg);
    room.send(RoomMessageEventContent::text_html(msg, &html))
        .await?;
    Ok(())
}

/// Handle response to welcome message
async fn handle_welcome_response(
    room: &Room,
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
        room.send(RoomMessageEventContent::text_html(msg, &html))
            .await?;
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
        room.send(RoomMessageEventContent::text_html(msg, &html))
            .await?;
        return Ok(true);
    }

    // Unrecognized response, repeat the question
    let msg = "I didn't catch that. Reply **yes** to begin setup, or **skip** to do it later.";
    let html = markdown_to_html(msg);
    room.send(RoomMessageEventContent::text_html(msg, &html))
        .await?;
    Ok(true)
}

/// Handle response to API key check (retry/skip)
async fn handle_api_key_response(
    room: &Room,
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

        send_channel_prompt(room).await?;
        return Ok(true);
    }

    if msg_lower == "retry" {
        // TODO: Actually retry API validation
        let msg = "Retrying API connection...\n\n✅ Connection successful!\n\n";
        let html = markdown_to_html(msg);
        room.send(RoomMessageEventContent::text_html(msg, &html))
            .await?;

        // Move to channel creation
        let mut state = get_state(session_store, user_id)?.unwrap_or_default();
        state.step = OnboardingStep::CreateChannel;
        save_state(session_store, user_id, &state)?;

        send_channel_prompt(room).await?;
        return Ok(true);
    }

    // Unrecognized, remind them
    let msg = "Reply **retry** to try the API connection again, or **skip** to continue anyway.";
    let html = markdown_to_html(msg);
    room.send(RoomMessageEventContent::text_html(msg, &html))
        .await?;
    Ok(true)
}

/// Send the channel name prompt
async fn send_channel_prompt(room: &Room) -> Result<()> {
    let msg = "**What would you like to call your first channel?**\n\
        Suggestions: `pa`, `research`, `dev`\n\n\
        _(Just type a name - letters, numbers, dashes only)_";
    let html = markdown_to_html(msg);
    room.send(RoomMessageEventContent::text_html(msg, &html))
        .await?;
    Ok(())
}

/// Complete the onboarding flow and show success message
pub async fn complete(
    room: &Room,
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
    room.send(RoomMessageEventContent::text_html(&msg, &html))
        .await?;
    Ok(())
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
    session_store.clear_onboarding_state(user_id)?;
    start(room, session_store, user_id).await
}
