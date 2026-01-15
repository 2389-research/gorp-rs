// ABOUTME: Chat message processing module for Claude invocation and response streaming.
// ABOUTME: Handles attachments, typing indicators, session management, and Matrix response chunking.

use anyhow::Result;
use matrix_sdk::{
    room::Room,
    ruma::events::room::message::{
        MessageType, OriginalSyncRoomMessageEvent, RoomMessageEventContent,
    },
    Client,
};

use crate::{
    metrics,
    session::{Channel, SessionStore},
    utils::{
        chunk_message, log_matrix_message, markdown_to_html, strip_function_calls, MAX_CHUNK_SIZE,
    },
    warm_session::{prepare_session_async, SharedWarmSessionManager},
};
use gorp_agent::AgentEvent;

use super::{download_attachment, is_debug_enabled, route_to_dispatch, write_context_file};

/// Process a regular (non-command) chat message by invoking Claude and streaming the response.
///
/// This function handles:
/// - Building the prompt from image/file attachments
/// - Writing context file for MCP tools
/// - Managing typing indicators
/// - Preparing and using warm sessions
/// - Processing the agent event stream
/// - Chunking and sending responses to Matrix
pub async fn process_chat_message(
    room: Room,
    event: OriginalSyncRoomMessageEvent,
    client: Client,
    channel: Channel,
    session_store: SessionStore,
    warm_manager: SharedWarmSessionManager,
) -> Result<()> {
    let start_time = std::time::Instant::now();
    let body = event.content.body();

    // Check for attachments (images, files) and build the prompt
    let prompt = match &event.content.msgtype {
        MessageType::Image(image_content) => {
            // Download the image
            let filename = image_content.body.clone();
            match download_attachment(
                &client,
                &image_content.source,
                &filename,
                &channel.directory,
            )
            .await
            {
                Ok(rel_path) => {
                    let abs_path = format!("{}/{}", channel.directory, rel_path);
                    tracing::info!(path = %abs_path, "Image downloaded");
                    // Include image path in prompt for Claude to read
                    format!("[Attached image: {}]\n\n{}", abs_path, image_content.body)
                }
                Err(e) => {
                    tracing::error!(error = %e, "Failed to download image");
                    room.send(RoomMessageEventContent::text_plain(format!(
                        "⚠️ Failed to download image: {}",
                        e
                    )))
                    .await?;
                    return Ok(());
                }
            }
        }
        MessageType::File(file_content) => {
            // Download the file
            let filename = file_content.body.clone();
            match download_attachment(&client, &file_content.source, &filename, &channel.directory)
                .await
            {
                Ok(rel_path) => {
                    let abs_path = format!("{}/{}", channel.directory, rel_path);
                    tracing::info!(path = %abs_path, "File downloaded");
                    format!("[Attached file: {}]\n\n{}", abs_path, file_content.body)
                }
                Err(e) => {
                    tracing::error!(error = %e, "Failed to download file");
                    room.send(RoomMessageEventContent::text_plain(format!(
                        "⚠️ Failed to download file: {}",
                        e
                    )))
                    .await?;
                    return Ok(());
                }
            }
        }
        _ => {
            // Text message or other type - use body as-is
            body.to_string()
        }
    };

    let _channel_args = channel.cli_args(); // Kept for potential future use

    // Write context file for MCP tools (before Claude invocation)
    if let Err(e) = write_context_file(
        &channel.directory,
        room.room_id().as_str(),
        &channel.channel_name,
        &channel.session_id,
    )
    .await
    {
        tracing::warn!(error = %e, "Failed to write MCP context file");
        // Non-fatal - continue without context file
    }

    // Start typing indicator and keep it alive
    room.typing_notice(true).await?;

    // Spawn a task to keep the typing indicator refreshed every 25 seconds
    let typing_room = room.clone();
    let typing_room_id = room.room_id().to_string();
    let (typing_tx, mut typing_rx) = tokio::sync::oneshot::channel();
    let typing_handle = tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(25));
        let max_duration = tokio::time::Instant::now() + tokio::time::Duration::from_secs(600); // 10 min max
        interval.tick().await; // Skip first immediate tick

        loop {
            if tokio::time::Instant::now() > max_duration {
                tracing::warn!(room_id = %typing_room_id, "Typing indicator timed out after 10 minutes");
                break;
            }
            tokio::select! {
                _ = interval.tick() => {
                    if let Err(e) = typing_room.typing_notice(true).await {
                        tracing::warn!(room_id = %typing_room_id, error = %e, "Failed to refresh typing indicator");
                        break;
                    }
                }
                _ = &mut typing_rx => {
                    // Stop signal received
                    break;
                }
            }
        }
    });

    // Invoke agent with streaming to show tool usage
    let claude_start = std::time::Instant::now();
    metrics::record_claude_invocation("matrix");

    // Prepare session (creates session if needed)
    // Uses prepare_session_async which minimizes lock holding for concurrent access
    tracing::info!(channel = %channel.channel_name, "[CONCURRENCY] prepare_session_async START");
    let (session_handle, session_id, is_new_session) =
        match prepare_session_async(&warm_manager, &channel).await {
            Ok((handle, sid, is_new)) => (handle, sid, is_new),
            Err(e) => {
                let _ = typing_tx.send(());
                typing_handle.abort();
                room.typing_notice(false).await?;

                metrics::record_error("warm_session");
                let error_msg = format!("⚠️ Failed to prepare session: {}", e);
                room.send(RoomMessageEventContent::text_plain(&error_msg))
                    .await?;
                return Ok(());
            }
        };

    tracing::info!(channel = %channel.channel_name, "[CONCURRENCY] prepare_session_async DONE");

    // Update session store if a new session was created
    if is_new_session {
        if let Err(e) = session_store.update_session_id(room.room_id().as_str(), &session_id) {
            tracing::warn!(error = %e, "Failed to update session ID in store");
        }
    }

    // Send prompt and get event receiver directly - no intermediate channel needed
    // The backend streams events through the returned EventReceiver
    tracing::info!(channel = %channel.channel_name, session_id = %session_id, "[CONCURRENCY] send_prompt START");

    let mut event_rx =
        match crate::warm_session::send_prompt_with_handle(&session_handle, &session_id, &prompt)
            .await
        {
            Ok(receiver) => receiver,
            Err(e) => {
                let _ = typing_tx.send(());
                typing_handle.abort();
                room.typing_notice(false).await?;

                metrics::record_error("prompt_send");
                let error_msg = format!("⚠️ Failed to send prompt: {}", e);
                room.send(RoomMessageEventContent::text_plain(&error_msg))
                    .await?;
                return Ok(());
            }
        };

    tracing::info!(channel = %channel.channel_name, "[CONCURRENCY] send_prompt DONE - got receiver");

    // Check if debug mode is enabled for this channel
    // Debug mode shows tool usage in Matrix (create .gorp/enable-debug to enable)
    let debug_enabled = is_debug_enabled(&channel.directory);
    if debug_enabled {
        tracing::debug!(channel = %channel.channel_name, "Debug mode enabled - will show tool usage");
    }

    // Process streaming events from agent
    let mut final_response = String::new();
    let mut tools_used: Vec<String> = Vec::new();
    let mut session_id_from_event: Option<String> = None;

    tracing::info!(channel = %channel.channel_name, "[CONCURRENCY] event_loop START - waiting for events");
    let mut event_count = 0;

    while let Some(event) = event_rx.recv().await {
        event_count += 1;
        tracing::trace!(channel = %channel.channel_name, event_count, event = ?event, "Received agent event");
        match event {
            AgentEvent::ToolStart { name, input, .. } => {
                tools_used.push(name.clone());
                metrics::record_tool_used(&name);

                // When tool output is hidden, add paragraph break between text blocks
                // This ensures text before and after tool usage is visually separated
                if !debug_enabled
                    && !final_response.is_empty()
                    && !final_response.ends_with('\n')
                {
                    final_response.push_str("\n\n");
                }

                // Extract input preview from JSON input
                let input_preview: String = input
                    .as_object()
                    .and_then(|o| o.get("command").or(o.get("file_path")).or(o.get("pattern")))
                    .and_then(|v| v.as_str())
                    .map(|s| s.chars().take(50).collect())
                    .unwrap_or_default();

                // Only send tool notifications if debug mode is enabled
                if debug_enabled {
                    // Build tool message with plain and HTML versions
                    let (plain, html) = if input_preview.is_empty() {
                        (format!("🔧 {}", name), format!("🔧 <code>{}</code>", name))
                    } else {
                        (
                            format!("🔧 {} · {}", name, input_preview),
                            format!("🔧 <code>{}</code> · <code>{}</code>", name, input_preview),
                        )
                    };

                    // Send tool notification to room
                    if let Err(e) = room
                        .send(RoomMessageEventContent::text_html(&plain, &html))
                        .await
                    {
                        tracing::warn!(error = %e, "Failed to send tool notification");
                    } else {
                        log_matrix_message(
                            &channel.directory,
                            room.room_id().as_str(),
                            "tool_notification",
                            &plain,
                            Some(&html),
                            None,
                            None,
                        )
                        .await;
                    }
                }
            }
            AgentEvent::ToolEnd { .. } => {
                // Tool completion - just log for now
                tracing::debug!("Tool completed");
            }
            AgentEvent::Text(text) => {
                // Accumulate text chunks
                final_response.push_str(&text);
            }
            AgentEvent::Result { text, .. } => {
                // Final result - use the accumulated text if we have it, otherwise use result text
                if !final_response.is_empty() {
                    // We already accumulated text, result is just completion marker
                    tracing::info!(
                        response_len = final_response.len(),
                        "Agent session completed"
                    );
                } else {
                    // No accumulated text, use the result text
                    final_response = text;
                    tracing::info!(
                        response_len = final_response.len(),
                        "Agent session completed with result text"
                    );
                }
                break; // Exit event loop - prompt is complete
            }
            AgentEvent::Error { code, message, .. } => {
                let _ = typing_tx.send(());
                typing_handle.abort();
                room.typing_notice(false).await?;

                // Check for session orphaned error
                if code == gorp_agent::ErrorCode::SessionOrphaned {
                    // Reset the session so next message starts fresh
                    if let Err(e) = session_store.reset_orphaned_session(room.room_id().as_str()) {
                        tracing::error!(error = %e, "Failed to reset invalid session");
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
                        "Evicted warm session after orphaned session"
                    );
                    metrics::record_error("invalid_session");
                    room.send(RoomMessageEventContent::text_plain(
                        "🔄 Session was reset (conversation data was lost). Please send your message again.",
                    ))
                    .await?;
                } else {
                    metrics::record_error("agent_streaming");
                    let error_msg = format!("⚠️ Agent error: {}", message);
                    room.send(RoomMessageEventContent::text_plain(&error_msg))
                        .await?;
                }
                return Ok(());
            }
            AgentEvent::SessionInvalid { reason } => {
                let _ = typing_tx.send(());
                typing_handle.abort();
                room.typing_notice(false).await?;

                tracing::warn!(reason = %reason, "Session invalid");
                // Reset the session so next message starts fresh
                if let Err(e) = session_store.reset_orphaned_session(room.room_id().as_str()) {
                    tracing::error!(error = %e, "Failed to reset invalid session");
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
                    "Evicted warm session after invalid session"
                );
                metrics::record_error("invalid_session");
                room.send(RoomMessageEventContent::text_plain(
                    "🔄 Session was reset (conversation data was lost). Please send your message again.",
                ))
                .await?;
                return Ok(());
            }
            AgentEvent::SessionChanged { new_session_id } => {
                tracing::info!(
                    old_session = %channel.session_id,
                    new_session = %new_session_id,
                    "Session ID changed during execution"
                );
                // Track the new session ID so we can update the database after the event loop
                session_id_from_event = Some(new_session_id);
            }
            AgentEvent::ToolProgress { .. } => {
                // Tool progress updates - just log for now
                tracing::debug!("Tool progress update");
            }
            AgentEvent::Custom { kind, payload } => {
                tracing::debug!(kind = %kind, "Received custom event");

                // Check for DISPATCH events
                if kind.starts_with("dispatch:") {
                    if let Err(e) =
                        route_to_dispatch(&session_store, room.room_id().as_str(), &kind, &payload)
                            .await
                    {
                        tracing::warn!(error = %e, "Failed to route event to DISPATCH");
                    }
                }
            }
        }
    }

    tracing::info!(channel = %channel.channel_name, event_count, "[CONCURRENCY] event_loop DONE");

    // Check if we got a response
    if final_response.is_empty() {
        let _ = typing_tx.send(());
        typing_handle.abort();
        room.typing_notice(false).await?;

        let backend_type = warm_manager.read().await.backend_type().to_string();
        metrics::record_error("agent_no_response");
        room.send(RoomMessageEventContent::text_plain(format!(
            "⚠️ {} backend finished without a response",
            backend_type
        )))
        .await?;
        return Ok(());
    }

    let claude_duration = claude_start.elapsed().as_secs_f64();
    let backend_type = warm_manager.read().await.backend_type().to_string();
    metrics::record_claude_duration(claude_duration);
    metrics::record_claude_response_length(final_response.len());
    tracing::info!(
        response_length = final_response.len(),
        tools_count = tools_used.len(),
        backend = %backend_type,
        "Agent responded"
    );

    // Filter out XML function call blocks before sending to Matrix
    // Some backends may output raw XML that shouldn't be shown to users
    let response = strip_function_calls(&final_response);

    // Update session ID if Claude CLI reported a new one via SessionChanged event
    // This is critical for session continuity - the CLI generates its own session IDs
    // which differ from the UUIDs we generate when creating new sessions
    if let Some(ref new_session_id) = session_id_from_event {
        if let Err(e) = session_store.update_session_id(room.room_id().as_str(), new_session_id) {
            tracing::error!(
                error = %e,
                room_id = %room.room_id(),
                new_session_id = %new_session_id,
                "Failed to update session ID after prompt"
            );
        } else {
            tracing::info!(
                room_id = %room.room_id(),
                old_session = %channel.session_id,
                new_session = %new_session_id,
                "Updated session ID in database"
            );
            // CRITICAL: Also update the warm session cache to match the database
            // Without this, the cached session will have a stale ID and fail on next use
            {
                let mut session = session_handle.lock().await;
                session.set_session_id(new_session_id.clone());
            }
            tracing::debug!(
                channel = %channel.channel_name,
                new_session = %new_session_id,
                "Updated session ID in warm cache"
            );
        }
    }

    // Mark session as started BEFORE sending response (to ensure consistency)
    session_store.mark_started(room.room_id().as_str())?;

    // Send response with markdown formatting, chunked if too long
    // Matrix limit is ~65KB but we chunk for better display
    let chunks = chunk_message(&response, MAX_CHUNK_SIZE);
    let chunk_count = chunks.len();
    let mut chunks_iter = chunks.into_iter().enumerate().peekable();

    // Send first chunk BEFORE stopping typing indicator
    // This ensures user sees message arriving before "stopped typing"
    if let Some((i, chunk)) = chunks_iter.next() {
        let html = markdown_to_html(&chunk);
        room.send(RoomMessageEventContent::text_html(&chunk, &html))
            .await?;
        metrics::record_message_sent();

        // Now stop typing indicator - user already sees first chunk arriving
        let _ = typing_tx.send(());
        let _ = typing_handle.await;
        room.typing_notice(false).await?;

        // Log the Matrix message
        log_matrix_message(
            &channel.directory,
            room.room_id().as_str(),
            "response",
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

        // Small delay before next chunk if there are more
        if chunks_iter.peek().is_some() {
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }
    }

    // Send remaining chunks
    for (i, chunk) in chunks_iter {
        let html = markdown_to_html(&chunk);
        room.send(RoomMessageEventContent::text_html(&chunk, &html))
            .await?;
        metrics::record_message_sent();

        // Log the Matrix message
        log_matrix_message(
            &channel.directory,
            room.room_id().as_str(),
            "response",
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

        // Small delay between chunks to maintain order (except after last)
        if i < chunk_count - 1 {
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }
    }

    // Record total message processing time
    let total_duration = start_time.elapsed().as_secs_f64();
    metrics::record_message_processing_duration(total_duration);

    tracing::info!(chunk_count, "Response sent successfully");

    Ok(())
}
