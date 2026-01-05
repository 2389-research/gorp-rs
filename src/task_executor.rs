// ABOUTME: Background task executor for dispatched work from DISPATCH control plane.
// ABOUTME: Polls pending tasks and sends prompts to target workspace rooms.

use anyhow::Result;
use gorp_agent::AgentEvent;
use gorp_core::session::{DispatchEvent, DispatchTaskStatus, SessionStore};
use matrix_sdk::{ruma::events::room::message::RoomMessageEventContent, Client};
use std::sync::Arc;
use tokio::time::{interval, Duration};

use crate::{
    config::Config,
    utils::{chunk_message, markdown_to_html, MAX_CHUNK_SIZE},
    warm_session::{prepare_session_async, send_prompt_with_handle, SharedWarmSessionManager},
};

/// Start the background task executor
///
/// This runs in a loop, polling for pending dispatch tasks and executing them
/// by sending prompts to target workspace rooms.
pub fn start_task_executor(
    client: Client,
    session_store: SessionStore,
    config: Arc<Config>,
    warm_manager: SharedWarmSessionManager,
) {
    tokio::spawn(async move {
        tracing::info!("Task executor starting");
        run_executor_loop(client, session_store, config, warm_manager).await;
    });
}

/// Main executor loop - polls for pending tasks every 5 seconds
async fn run_executor_loop(
    client: Client,
    session_store: SessionStore,
    config: Arc<Config>,
    warm_manager: SharedWarmSessionManager,
) {
    let mut ticker = interval(Duration::from_secs(5));

    loop {
        ticker.tick().await;

        // Get pending tasks
        let pending_tasks = match session_store.list_dispatch_tasks(Some(DispatchTaskStatus::Pending)) {
            Ok(tasks) => tasks,
            Err(e) => {
                tracing::warn!(error = %e, "Failed to list pending dispatch tasks");
                continue;
            }
        };

        if pending_tasks.is_empty() {
            continue;
        }

        tracing::info!(count = pending_tasks.len(), "Found pending dispatch tasks");

        // Process each pending task
        for task in pending_tasks {
            // Atomically claim the task using compare-and-swap
            // Only one executor can successfully claim a task, preventing duplicate execution
            let claimed = match session_store.claim_dispatch_task(
                &task.id,
                DispatchTaskStatus::Pending,
                DispatchTaskStatus::InProgress,
            ) {
                Ok(true) => true,
                Ok(false) => {
                    tracing::debug!(task_id = %task.id, "Task already claimed by another executor");
                    continue;
                }
                Err(e) => {
                    tracing::error!(task_id = %task.id, error = %e, "Failed to claim task");
                    continue;
                }
            };

            if !claimed {
                continue;
            }

            tracing::info!(
                task_id = %task.id,
                target_room = %task.target_room_id,
                prompt_preview = %task.prompt.chars().take(50).collect::<String>(),
                "Executing dispatch task"
            );

            // Execute the task
            let result = execute_task(
                &task.id,
                &task.target_room_id,
                &task.prompt,
                &client,
                &session_store,
                Arc::clone(&config),
                warm_manager.clone(),
            )
            .await;

            // Update status based on result
            match result {
                Ok(summary) => {
                    if let Err(e) = session_store.update_dispatch_task_status(
                        &task.id,
                        DispatchTaskStatus::Completed,
                        Some(&summary),
                    ) {
                        tracing::error!(task_id = %task.id, error = %e, "Failed to mark task completed");
                    } else {
                        tracing::info!(task_id = %task.id, "Dispatch task completed");

                        // Create event for DISPATCH to see the completion
                        let event = DispatchEvent {
                            id: uuid::Uuid::new_v4().to_string(),
                            source_room_id: task.target_room_id.clone(),
                            event_type: "task_completed".to_string(),
                            payload: serde_json::json!({
                                "task_id": task.id,
                                "summary": summary.chars().take(100).collect::<String>(),
                            }),
                            created_at: chrono::Utc::now().to_rfc3339(),
                            acknowledged_at: None,
                        };
                        if let Err(e) = session_store.insert_dispatch_event(&event) {
                            tracing::warn!(error = %e, "Failed to create task completion event");
                        }

                        // Notify DISPATCH rooms in real-time
                        notify_dispatch_rooms(
                            &client,
                            &session_store,
                            &task.target_room_id,
                            &task.id,
                            true,
                            &summary,
                        )
                        .await;
                    }
                }
                Err(e) => {
                    let error_msg = e.to_string();
                    if let Err(update_err) = session_store.update_dispatch_task_status(
                        &task.id,
                        DispatchTaskStatus::Failed,
                        Some(&error_msg),
                    ) {
                        tracing::error!(task_id = %task.id, error = %update_err, "Failed to mark task failed");
                    } else {
                        tracing::error!(task_id = %task.id, error = %error_msg, "Dispatch task failed");

                        // Create error event for DISPATCH
                        let event = DispatchEvent {
                            id: uuid::Uuid::new_v4().to_string(),
                            source_room_id: task.target_room_id.clone(),
                            event_type: "task_failed".to_string(),
                            payload: serde_json::json!({
                                "task_id": task.id,
                                "error": error_msg.chars().take(100).collect::<String>(),
                            }),
                            created_at: chrono::Utc::now().to_rfc3339(),
                            acknowledged_at: None,
                        };
                        if let Err(e) = session_store.insert_dispatch_event(&event) {
                            tracing::warn!(error = %e, "Failed to create task failure event");
                        }

                        // Notify DISPATCH rooms in real-time
                        notify_dispatch_rooms(
                            &client,
                            &session_store,
                            &task.target_room_id,
                            &task.id,
                            false,
                            &error_msg,
                        )
                        .await;
                    }
                }
            }
        }
    }
}

/// Notify all DISPATCH rooms about task completion/failure
async fn notify_dispatch_rooms(
    client: &Client,
    session_store: &SessionStore,
    target_room_id: &str,
    task_id: &str,
    success: bool,
    message: &str,
) {
    // Get channel name for the target room
    let channel_name = session_store
        .get_by_room(target_room_id)
        .ok()
        .flatten()
        .map(|c| c.channel_name)
        .unwrap_or_else(|| "unknown".to_string());

    // Get all DISPATCH channels
    let dispatch_channels = match session_store.list_dispatch_channels() {
        Ok(channels) => channels,
        Err(e) => {
            tracing::warn!(error = %e, "Failed to list DISPATCH channels for notification");
            return;
        }
    };

    // Build notification message
    let task_short_id: String = task_id.chars().take(8).collect();
    let notification = if success {
        format!(
            "✅ **Task Completed** in **{}**\n`{}`\n> {}",
            channel_name,
            task_short_id,
            message.chars().take(150).collect::<String>()
        )
    } else {
        format!(
            "❌ **Task Failed** in **{}**\n`{}`\n> {}",
            channel_name,
            task_short_id,
            message.chars().take(150).collect::<String>()
        )
    };

    let notification_html = markdown_to_html(&notification);

    // Send to each DISPATCH room
    for dispatch in dispatch_channels {
        let room_id: matrix_sdk::ruma::OwnedRoomId = match dispatch.room_id.parse() {
            Ok(id) => id,
            Err(_) => continue,
        };

        let Some(room) = client.get_room(&room_id) else {
            continue;
        };

        if let Err(e) = room
            .send(RoomMessageEventContent::text_html(
                &notification,
                &notification_html,
            ))
            .await
        {
            tracing::warn!(
                dispatch_room = %dispatch.room_id,
                error = %e,
                "Failed to send task notification to DISPATCH"
            );
        } else {
            tracing::debug!(
                dispatch_room = %dispatch.room_id,
                task_id = %task_id,
                "Sent task completion notification to DISPATCH"
            );
        }
    }
}

/// Execute a single dispatch task
async fn execute_task(
    task_id: &str,
    target_room_id: &str,
    prompt: &str,
    client: &Client,
    session_store: &SessionStore,
    _config: Arc<Config>,
    warm_manager: SharedWarmSessionManager,
) -> Result<String> {
    // Get channel info for the target room
    let channel = session_store
        .get_by_room(target_room_id)?
        .ok_or_else(|| anyhow::anyhow!("Target room not found: {}", target_room_id))?;

    // Get the Matrix room
    let room_id: matrix_sdk::ruma::OwnedRoomId = target_room_id
        .parse()
        .map_err(|e| anyhow::anyhow!("Invalid room ID: {}", e))?;

    let room = client
        .get_room(&room_id)
        .ok_or_else(|| anyhow::anyhow!("Room not accessible: {}", target_room_id))?;

    // Send notification that a dispatched task is starting
    let notification = format!(
        "📋 **Dispatched Task**\n> {}",
        prompt.chars().take(200).collect::<String>()
    );
    let notification_html = markdown_to_html(&notification);
    if let Err(e) = room
        .send(RoomMessageEventContent::text_html(
            &notification,
            &notification_html,
        ))
        .await
    {
        tracing::warn!(task_id = %task_id, error = %e, "Failed to send task notification");
    }

    // Start typing indicator
    if let Err(e) = room.typing_notice(true).await {
        tracing::debug!(task_id = %task_id, error = %e, "Failed to send typing indicator");
    }

    // Prepare session (creates session if needed)
    let (session_handle, session_id, is_new_session) =
        prepare_session_async(&warm_manager, &channel).await?;

    // Update session store if a new session was created
    if is_new_session {
        if let Err(e) = session_store.update_session_id(&channel.room_id, &session_id) {
            tracing::warn!(error = %e, "Failed to update session ID in store");
        }
    }

    // Send prompt and get event receiver
    let mut rx = send_prompt_with_handle(&session_handle, &session_id, prompt).await?;

    // Collect response from stream
    let mut response = String::new();
    let mut had_error = false;
    let mut error_message = String::new();

    while let Some(event) = rx.recv().await {
        match event {
            AgentEvent::Text(text) => {
                response.push_str(&text);
            }
            AgentEvent::Result { text, .. } => {
                if response.is_empty() {
                    response = text;
                }
                break;
            }
            AgentEvent::Error { message, .. } => {
                had_error = true;
                error_message = message;
                break;
            }
            AgentEvent::ToolStart { name, .. } => {
                tracing::debug!(task_id = %task_id, tool = %name, "Tool started");
            }
            AgentEvent::ToolEnd { name, success, .. } => {
                tracing::debug!(task_id = %task_id, tool = %name, success = success, "Tool completed");
            }
            _ => {}
        }
    }

    // Stop typing indicator
    let _ = room.typing_notice(false).await;

    if had_error {
        // Send error to room
        let error_msg = format!("⚠️ **Task Error**\n{}", error_message);
        if let Err(e) = room
            .send(RoomMessageEventContent::text_plain(&error_msg))
            .await
        {
            tracing::error!(task_id = %task_id, error = %e, "Failed to send error message to room");
        }
        return Err(anyhow::anyhow!("{}", error_message));
    }

    // Send response to room (chunk if needed)
    if !response.is_empty() {
        let chunks = chunk_message(&response, MAX_CHUNK_SIZE);
        for chunk in chunks {
            let html = markdown_to_html(&chunk);
            if let Err(e) = room
                .send(RoomMessageEventContent::text_html(&chunk, &html))
                .await
            {
                tracing::error!(task_id = %task_id, error = %e, "Failed to send response chunk");
            }
        }
    }

    // Generate summary for the result
    let summary = if response.len() > 100 {
        format!("{}...", response.chars().take(100).collect::<String>())
    } else {
        response.clone()
    };

    Ok(summary)
}
