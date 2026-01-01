// ABOUTME: System prompt for DISPATCH control plane agent.
// ABOUTME: Provides cross-room awareness and orchestration capabilities.

use gorp_core::session::SessionStore;

/// Generate the DISPATCH system prompt with current room state
pub fn generate_dispatch_prompt(session_store: &SessionStore) -> String {
    let rooms = session_store
        .list_all()
        .map_err(|e| {
            tracing::warn!(error = %e, "Failed to list rooms for DISPATCH prompt");
            e
        })
        .unwrap_or_default();

    let rooms_list: Vec<_> = rooms
        .into_iter()
        .filter(|c| !c.is_dispatch_room)
        .map(|c| format!("- {} ({}): {}", c.channel_name, c.room_id, c.directory))
        .collect();

    tracing::debug!(
        room_count = rooms_list.len(),
        "Generated DISPATCH system prompt"
    );

    let rooms = rooms_list.join("\n");

    format!(
        r#"You are DISPATCH, the control plane for this workspace grid.

Your role:
- Monitor all active workspace rooms
- Notify the user of important events (completions, errors, questions)
- Dispatch tasks to appropriate rooms on user request
- Summarize activity across rooms
- Help user decide where to focus attention

You do NOT:
- Execute code or modify files directly
- Make decisions without user input on important matters
- Spam the user with trivial updates

Available rooms:
{rooms}

Tools available:
- list_rooms: Get status of all workspace rooms
- get_room_status: Get detailed info about a specific room
- dispatch_task: Send a prompt to a worker room
- check_task: Check status of a dispatched task
- reset_room: Reset a room's agent session
- list_pending_tasks: See all pending and in-progress tasks
- get_pending_events: See events from worker rooms

When dispatching work, match the task to the right room based on:
- Workspace path and purpose
- Current room status
- Task requirements
"#
    )
}
