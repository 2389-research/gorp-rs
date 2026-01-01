// ABOUTME: Matrix-specific scheduler execution for gorp.
// ABOUTME: Re-exports core scheduler types from gorp-core, adds Matrix SDK integration.

// Re-export all core scheduler types and functions from gorp-core
// This ensures type consistency across the codebase
pub use gorp_core::scheduler::{
    compute_next_cron_execution, compute_next_cron_execution_in_tz, parse_time_expression,
    ParsedSchedule, ScheduleStatus, ScheduledPrompt, SchedulerCallback, SchedulerStore,
};

// Matrix-specific scheduler execution implementation
use anyhow::Result;
use chrono::Utc;
use gorp_agent::AgentEvent;
use matrix_sdk::{ruma::events::room::message::RoomMessageEventContent, Client};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration as StdDuration;
use tokio::time::interval;

use crate::{
    config::Config,
    metrics,
    session::{Channel, SessionStore},
    utils::{
        chunk_message, expand_slash_command, log_matrix_message, markdown_to_html, MAX_CHUNK_SIZE,
    },
    warm_session::{prepare_session_async, SharedWarmSessionManager},
};

/// Write context file for MCP tools (used by scheduler before Claude invocation)
async fn write_context_file(channel: &Channel) -> Result<()> {
    let gorp_dir = Path::new(&channel.directory).join(".gorp");
    tokio::fs::create_dir_all(&gorp_dir).await?;

    let context = serde_json::json!({
        "room_id": channel.room_id,
        "channel_name": channel.channel_name,
        "session_id": channel.session_id,
        "updated_at": Utc::now().to_rfc3339()
    });

    let context_path = gorp_dir.join("context.json");
    tokio::fs::write(&context_path, serde_json::to_string_pretty(&context)?).await?;

    tracing::debug!(path = %context_path.display(), "Wrote MCP context file for scheduled task");
    Ok(())
}

/// Start the background scheduler task that checks for and executes due schedules
pub async fn start_scheduler(
    scheduler_store: SchedulerStore,
    session_store: SessionStore,
    client: Client,
    config: Arc<Config>,
    check_interval: StdDuration,
    warm_manager: SharedWarmSessionManager,
) {
    tracing::info!(
        interval_secs = check_interval.as_secs(),
        "Starting scheduler background task"
    );

    let mut ticker = interval(check_interval);

    loop {
        ticker.tick().await;

        let now = Utc::now();
        // Use claim_due_schedules to atomically mark schedules as 'executing'
        // This prevents race conditions where a slow execution could cause duplicates
        match scheduler_store.claim_due_schedules(now) {
            Ok(schedules) => {
                if !schedules.is_empty() {
                    tracing::info!(
                        count = schedules.len(),
                        "Claimed due schedules for execution"
                    );
                }

                for schedule in schedules {
                    // Clone what we need for the spawned task
                    let store = scheduler_store.clone();
                    let sess_store = session_store.clone();
                    let cli = client.clone();
                    let cfg = Arc::clone(&config);
                    let warm_mgr = warm_manager.clone();

                    // Execute each due schedule concurrently
                    // Use spawn_local since ACP client futures are !Send
                    tokio::task::spawn_local(async move {
                        execute_schedule(schedule, store, sess_store, cli, cfg, warm_mgr).await;
                    });
                }
            }
            Err(e) => {
                tracing::error!(error = %e, "Failed to fetch due schedules");
            }
        }

        // Check for schedules that should be pre-warmed
        // Pre-warm if within pre_warm_secs of execution
        let pre_warm_duration = chrono::Duration::seconds(config.backend.pre_warm_secs as i64);
        let pre_warm_cutoff = now + pre_warm_duration;

        if let Ok(all_schedules) = scheduler_store.list_all() {
            for schedule in all_schedules {
                // Only pre-warm active schedules
                if schedule.status != ScheduleStatus::Active {
                    continue;
                }

                // Parse next execution time
                if let Ok(next_exec) =
                    chrono::DateTime::parse_from_rfc3339(&schedule.next_execution_at)
                {
                    let next_exec_utc = next_exec.with_timezone(&Utc);

                    // Pre-warm if within the window
                    if next_exec_utc > now && next_exec_utc <= pre_warm_cutoff {
                        // Get channel for this schedule
                        if let Ok(Some(channel)) = session_store.get_by_name(&schedule.channel_name)
                        {
                            let channel_name = schedule.channel_name.clone();

                            // Pre-warm directly (without spawning to avoid Send issues)
                            let mut mgr = warm_manager.write().await;
                            if let Err(e) = mgr.pre_warm(&channel).await {
                                tracing::warn!(
                                    channel = %channel_name,
                                    error = %e,
                                    "Pre-warm failed for upcoming schedule"
                                );
                            } else {
                                tracing::debug!(
                                    channel = %channel_name,
                                    "Pre-warmed session for upcoming schedule"
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Execute a single scheduled prompt
async fn execute_schedule(
    schedule: ScheduledPrompt,
    scheduler_store: SchedulerStore,
    session_store: SessionStore,
    client: Client,
    config: Arc<Config>,
    warm_manager: SharedWarmSessionManager,
) {
    let prompt_preview: String = schedule.prompt.chars().take(50).collect();
    tracing::info!(
        schedule_id = %schedule.id,
        channel = %schedule.channel_name,
        prompt_preview = %prompt_preview,
        "Executing scheduled prompt"
    );

    // Get channel info
    let channel = match session_store.get_by_name(&schedule.channel_name) {
        Ok(Some(c)) => c,
        Ok(None) => {
            tracing::error!(
                schedule_id = %schedule.id,
                channel = %schedule.channel_name,
                "Channel no longer exists"
            );
            if let Err(e) = scheduler_store.mark_failed(&schedule.id, "Channel no longer exists") {
                tracing::error!(error = %e, schedule_id = %schedule.id, "Failed to mark schedule failed");
            }
            return;
        }
        Err(e) => {
            tracing::error!(
                schedule_id = %schedule.id,
                error = %e,
                "Failed to get channel"
            );
            if let Err(e) = scheduler_store.mark_failed(&schedule.id, &e.to_string()) {
                tracing::error!(error = %e, schedule_id = %schedule.id, "Failed to mark schedule failed");
            }
            return;
        }
    };

    // Get Matrix room
    let room_id: matrix_sdk::ruma::OwnedRoomId = match schedule.room_id.parse() {
        Ok(id) => id,
        Err(e) => {
            tracing::error!(
                schedule_id = %schedule.id,
                room_id = %schedule.room_id,
                error = %e,
                "Invalid room ID"
            );
            if let Err(e) =
                scheduler_store.mark_failed(&schedule.id, &format!("Invalid room ID: {}", e))
            {
                tracing::error!(error = %e, schedule_id = %schedule.id, "Failed to mark schedule failed");
            }
            return;
        }
    };

    let Some(room) = client.get_room(&room_id) else {
        tracing::error!(
            schedule_id = %schedule.id,
            room_id = %schedule.room_id,
            "Room not found"
        );
        if let Err(e) = scheduler_store.mark_failed(&schedule.id, "Room not found") {
            tracing::error!(error = %e, schedule_id = %schedule.id, "Failed to mark schedule failed");
        }
        return;
    };

    // Send notification that scheduled prompt is executing
    let notification = format!(
        "⏰ **Scheduled Task**\n> {}",
        schedule.prompt.chars().take(200).collect::<String>()
    );
    let notification_html = markdown_to_html(&notification);
    if let Err(e) = room
        .send(RoomMessageEventContent::text_html(
            &notification,
            &notification_html,
        ))
        .await
    {
        tracing::warn!(error = %e, "Failed to send schedule notification");
    }

    // Write context file for MCP tools before invoking Claude
    if let Err(e) = write_context_file(&channel).await {
        tracing::warn!(error = %e, "Failed to write context file for scheduled task");
        // Non-fatal - continue without context file
    }

    // Expand slash commands at execution time (so updates to commands are picked up)
    let prompt = match expand_slash_command(&schedule.prompt, &channel.directory) {
        Ok(p) => p,
        Err(e) => {
            tracing::error!(
                schedule_id = %schedule.id,
                error = %e,
                "Failed to expand slash command"
            );
            let error_msg = format!("⚠️ Scheduled task failed: {}", e);
            if let Err(e) = room
                .send(RoomMessageEventContent::text_plain(&error_msg))
                .await
            {
                tracing::error!(error = %e, schedule_id = %schedule.id, "Failed to send error message to room");
            }
            if let Err(e) = scheduler_store.mark_failed(&schedule.id, &e.to_string()) {
                tracing::error!(error = %e, schedule_id = %schedule.id, "Failed to mark schedule failed");
            }
            return;
        }
    };

    // Start typing indicator
    if let Err(e) = room.typing_notice(true).await {
        tracing::debug!(error = %e, schedule_id = %schedule.id, "Failed to send typing indicator");
    }

    // Prepare session (creates session if needed)
    // Uses prepare_session_async which minimizes lock holding for concurrent access
    let (session_handle, session_id, is_new_session) = match prepare_session_async(
        &warm_manager,
        &channel,
    )
    .await
    {
        Ok((handle, sid, is_new)) => (handle, sid, is_new),
        Err(e) => {
            tracing::error!(error = %e, schedule_id = %schedule.id, "Failed to prepare session for scheduled task");
            let error_msg = format!("⚠️ Failed to prepare session: {}", e);
            if let Err(e) = room
                .send(RoomMessageEventContent::text_plain(&error_msg))
                .await
            {
                tracing::error!(error = %e, schedule_id = %schedule.id, "Failed to send error message to room");
            }
            if let Err(e) = scheduler_store.mark_failed(&schedule.id, &e.to_string()) {
                tracing::error!(error = %e, schedule_id = %schedule.id, "Failed to mark schedule failed");
            }
            return;
        }
    };

    // Update session store if a new session was created
    if is_new_session {
        if let Err(e) = session_store.update_session_id(&channel.room_id, &session_id) {
            tracing::warn!(error = %e, "Failed to update session ID in store");
        }
    }

    // Send prompt and get event receiver directly
    let mut rx = match crate::warm_session::send_prompt_with_handle(
        &session_handle,
        &session_id,
        &prompt,
    )
    .await
    {
        Ok(receiver) => receiver,
        Err(e) => {
            tracing::error!(error = %e, schedule_id = %schedule.id, "Failed to send prompt for scheduled task");
            let error_msg = format!("⚠️ Failed to send prompt: {}", e);
            if let Err(e) = room
                .send(RoomMessageEventContent::text_plain(&error_msg))
                .await
            {
                tracing::error!(error = %e, schedule_id = %schedule.id, "Failed to send error message to room");
            }
            if let Err(e) = scheduler_store.mark_failed(&schedule.id, &e.to_string()) {
                tracing::error!(error = %e, schedule_id = %schedule.id, "Failed to mark schedule failed");
            }
            return;
        }
    };

    // Collect response from stream
    let mut response = String::new();
    let mut had_error = false;
    let mut session_id_from_event: Option<String> = Some(session_id.clone());

    while let Some(event) = rx.recv().await {
        match event {
            AgentEvent::ToolStart { name, input, .. } => {
                // Extract input preview from JSON input
                let input_preview: String = input
                    .as_object()
                    .and_then(|o| o.get("command").or(o.get("file_path")).or(o.get("pattern")))
                    .and_then(|v| v.as_str())
                    .map(|s| s.chars().take(50).collect())
                    .unwrap_or_default();
                tracing::debug!(
                    tool = %name,
                    preview = %input_preview,
                    schedule_id = %schedule.id,
                    channel = %schedule.channel_name,
                    "Scheduled task tool use"
                );
            }
            AgentEvent::ToolEnd { .. } => {
                // Tool completion - just log for now
                tracing::debug!("Tool completed");
            }
            AgentEvent::Text(text) => {
                // Accumulate text chunks
                response.push_str(&text);
            }
            AgentEvent::Result { text, .. } => {
                // If we haven't accumulated text, use the result text
                if response.is_empty() {
                    response = text;
                }
                tracing::info!(
                    response_len = response.len(),
                    "Scheduled task agent completed"
                );
            }
            AgentEvent::Error { code, message, .. } => {
                // Check for session orphaned error
                if code == gorp_agent::ErrorCode::SessionOrphaned {
                    tracing::warn!("Scheduled task hit invalid session");
                    // Reset the session so future executions start fresh
                    if let Err(e) = session_store.reset_orphaned_session(&channel.room_id) {
                        tracing::error!(error = %e, "Failed to reset invalid session in scheduler");
                    }
                    // Mark session as invalidated FIRST so concurrent users see it
                    {
                        let mut session = session_handle.lock().await;
                        session.set_invalidated(true);
                    }
                    // Then evict from warm cache
                    let evicted = {
                        let mut mgr = warm_manager.write().await;
                        mgr.evict(&channel.channel_name)
                    };
                    tracing::info!(
                        channel = %channel.channel_name,
                        evicted = evicted,
                        "Evicted warm session after orphaned session in scheduler"
                    );
                    if let Err(e) = room
                        .send(RoomMessageEventContent::text_plain(
                            "🔄 Session was reset (conversation data was lost). Scheduled task will retry next time.",
                        ))
                        .await
                    {
                        tracing::error!(error = %e, schedule_id = %schedule.id, "Failed to send session reset message to room");
                    }
                    if let Err(e) = scheduler_store.mark_failed(&schedule.id, "Session was invalid")
                    {
                        tracing::error!(error = %e, schedule_id = %schedule.id, "Failed to mark schedule failed");
                    }
                    return;
                }
                tracing::warn!(error = %message, "Scheduled task agent error");
                had_error = true;
                // Don't return yet - we might have captured text before the error
            }
            AgentEvent::SessionInvalid { reason } => {
                tracing::warn!(reason = %reason, "Scheduled task hit invalid session");
                // Reset the session so future executions start fresh
                if let Err(e) = session_store.reset_orphaned_session(&channel.room_id) {
                    tracing::error!(error = %e, "Failed to reset invalid session in scheduler");
                }
                // Mark session as invalidated FIRST so concurrent users see it
                {
                    let mut session = session_handle.lock().await;
                    session.set_invalidated(true);
                }
                // Then evict from warm cache
                let evicted = {
                    let mut mgr = warm_manager.write().await;
                    mgr.evict(&channel.channel_name)
                };
                tracing::info!(
                    channel = %channel.channel_name,
                    evicted = evicted,
                    "Evicted warm session after invalid session in scheduler"
                );
                if let Err(e) = room
                    .send(RoomMessageEventContent::text_plain(
                        "🔄 Session was reset (conversation data was lost). Scheduled task will retry next time.",
                    ))
                    .await
                {
                    tracing::error!(error = %e, schedule_id = %schedule.id, "Failed to send session reset message to room");
                }
                if let Err(e) = scheduler_store.mark_failed(&schedule.id, "Session was invalid") {
                    tracing::error!(error = %e, schedule_id = %schedule.id, "Failed to mark schedule failed");
                }
                return;
            }
            AgentEvent::SessionChanged { new_session_id } => {
                // Track session ID changes during execution
                session_id_from_event = Some(new_session_id);
            }
            AgentEvent::ToolProgress { .. } => {
                tracing::debug!("Tool progress update");
            }
            AgentEvent::Custom { kind, .. } => {
                tracing::debug!(kind = %kind, "Received custom event");
            }
        }
    }

    // Stop typing
    if let Err(e) = room.typing_notice(false).await {
        tracing::debug!(error = %e, schedule_id = %schedule.id, "Failed to stop typing indicator");
    }

    // Check for empty response
    if response.trim().is_empty() {
        if had_error {
            tracing::error!(
                schedule_id = %schedule.id,
                prompt = %schedule.prompt,
                "ACP returned empty response with error for scheduled task"
            );
            let error_msg =
                "⚠️ Scheduled task failed: ACP encountered an error and returned no response.";
            if let Err(e) = room
                .send(RoomMessageEventContent::text_plain(error_msg))
                .await
            {
                tracing::error!(error = %e, schedule_id = %schedule.id, "Failed to send error message to room");
            }
            if let Err(e) =
                scheduler_store.mark_failed(&schedule.id, "ACP error with empty response")
            {
                tracing::error!(error = %e, schedule_id = %schedule.id, "Failed to mark schedule failed");
            }
        } else {
            tracing::error!(
                schedule_id = %schedule.id,
                prompt = %schedule.prompt,
                "ACP returned empty response for scheduled task"
            );
            let error_msg = "⚠️ Scheduled task failed: ACP returned an empty response. This may indicate a session issue or prompt problem.";
            if let Err(e) = room
                .send(RoomMessageEventContent::text_plain(error_msg))
                .await
            {
                tracing::error!(error = %e, schedule_id = %schedule.id, "Failed to send error message to room");
            }
            if let Err(e) = scheduler_store.mark_failed(&schedule.id, "Empty response from ACP") {
                tracing::error!(error = %e, schedule_id = %schedule.id, "Failed to mark schedule failed");
            }
        }
        return;
    }

    // Send response to room with chunking
    let chunks = chunk_message(&response, MAX_CHUNK_SIZE);
    let chunk_count = chunks.len();

    for (i, chunk) in chunks.into_iter().enumerate() {
        let html = markdown_to_html(&chunk);
        if let Err(e) = room
            .send(RoomMessageEventContent::text_html(&chunk, &html))
            .await
        {
            tracing::warn!(error = %e, chunk = i, "Failed to send response chunk");
        }

        // Log the Matrix message
        log_matrix_message(
            &channel.directory,
            room.room_id().as_str(),
            "scheduled_response",
            &chunk,
            Some(&html),
            if chunk_count > 1 { Some(i) } else { None },
            if chunk_count > 1 {
                Some(chunk_count)
            } else {
                None
            },
        )
        .await;

        // Small delay between chunks
        if i < chunk_count - 1 {
            tokio::time::sleep(StdDuration::from_millis(100)).await;
        }
    }

    // Update session ID if a new one was created
    let final_session_id = session_id_from_event;
    if let Some(ref sess_id) = final_session_id {
        if let Err(e) = session_store.update_session_id(&channel.room_id, sess_id) {
            tracing::error!(error = %e, "Failed to update session ID in scheduler");
            // Non-fatal - continue
        } else {
            // CRITICAL: Also update the warm session cache to match the database
            {
                let mut session = session_handle.lock().await;
                session.set_session_id(sess_id.clone());
            }
            tracing::debug!(
                channel = %channel.channel_name,
                new_session = %sess_id,
                "Updated session ID in warm cache (scheduler)"
            );
        }
    }

    // Calculate next execution for recurring schedules
    let next_execution = if let Some(ref cron_expr) = schedule.cron_expression {
        match compute_next_cron_execution_in_tz(cron_expr, &config.scheduler.timezone) {
            Ok(next) => Some(next),
            Err(e) => {
                // Log the error and mark schedule as failed instead of silently completing
                tracing::error!(
                    schedule_id = %schedule.id,
                    cron = %cron_expr,
                    timezone = %config.scheduler.timezone,
                    error = %e,
                    "Failed to compute next execution time for recurring schedule"
                );
                if let Err(e) = scheduler_store.mark_failed(
                    &schedule.id,
                    &format!("Failed to compute next execution: {}", e),
                ) {
                    tracing::error!(error = %e, schedule_id = %schedule.id, "Failed to mark schedule failed");
                }
                return; // Exit early - don't mark as executed
            }
        }
    } else {
        None // One-time schedule - will be marked completed
    };

    if let Err(e) = scheduler_store.mark_executed(&schedule.id, next_execution) {
        tracing::error!(
            schedule_id = %schedule.id,
            error = %e,
            "Failed to mark schedule as executed"
        );
    } else {
        // Record successful execution metric here (after we know it worked)
        metrics::record_schedule_executed();
        let status = if next_execution.is_some() {
            "rescheduled"
        } else {
            "completed"
        };
        tracing::info!(
            schedule_id = %schedule.id,
            status,
            "Schedule execution successful"
        );
    }

    // Log warning if there was an error but we still got a response
    if had_error {
        tracing::warn!(
            schedule_id = %schedule.id,
            "Scheduled task completed with warnings (ACP encountered non-fatal errors)"
        );
    }
}
