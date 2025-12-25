// ABOUTME: HTTP webhook server for injecting prompts into Claude sessions
// ABOUTME: Provides POST /webhook/session/{id} endpoint for external triggers like cron jobs

use anyhow::{Context, Result};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    middleware,
    response::{IntoResponse, Redirect},
    routing::{get, post},
    Json, Router,
};
use matrix_sdk::{
    ruma::{events::room::message::RoomMessageEventContent, OwnedRoomId},
    Client,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::{
    sync::{mpsc, oneshot},
    task::LocalSet,
};
use tower_http::trace::TraceLayer;

use crate::{
    admin::{admin_router, auth_middleware, AdminState},
    config::Config,
    mcp::{mcp_handler, McpState},
    metrics,
    scheduler::SchedulerStore,
    session::{Channel, SessionStore},
    utils::{chunk_message, log_matrix_message, markdown_to_html, MAX_CHUNK_SIZE},
    warm_session::{prepare_session_async, send_prompt_with_handle, SharedWarmSessionManager},
};
use gorp_agent::AgentEvent;
use metrics_exporter_prometheus::PrometheusHandle;

#[derive(Clone)]
struct WebhookState {
    session_store: SessionStore,
    matrix_client: Client,
    config: Arc<Config>,
    job_tx: WebhookJobSender,
}

#[derive(Debug, Deserialize)]
pub struct WebhookRequest {
    pub prompt: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct WebhookResponse {
    pub success: bool,
    pub message: String,
}

/// Channel sender used to queue webhook jobs onto the warm session worker.
type WebhookJobSender = mpsc::Sender<WebhookJob>;

/// Job sent from HTTP handlers to the LocalSet worker.
struct WebhookJob {
    channel: Channel,
    prompt: String,
    responder: oneshot::Sender<anyhow::Result<WebhookWorkerResponse>>,
}

/// Response returned by the worker after completing a prompt.
struct WebhookWorkerResponse {
    response_text: String,
    response_len: usize,
}

/// Start the webhook HTTP server
pub async fn start_webhook_server(
    port: u16,
    session_store: SessionStore,
    matrix_client: Client,
    config: Arc<Config>,
    warm_manager: SharedWarmSessionManager,
) -> Result<()> {
    let worker_session_store = session_store.clone();
    let job_tx = spawn_webhook_worker(worker_session_store, warm_manager);

    // Initialize Prometheus metrics
    let metrics_handle =
        metrics::init_metrics().context("Failed to initialize Prometheus metrics")?;

    let state = WebhookState {
        session_store,
        matrix_client,
        config,
        job_tx,
    };

    let webhook_routes = Router::new()
        .route("/webhook/session/{session_id}", post(webhook_handler))
        .with_state(Arc::new(state.clone()));

    // Create scheduler store here because it needs the database connection from session_store.
    // The scheduler_store is shared between admin routes (for viewing/managing schedules)
    // and MCP routes (for creating schedules via Claude). It must be created after
    // session_store is available but before admin_state and mcp_state are constructed.
    let scheduler_store = SchedulerStore::new(state.session_store.db_connection());

    // Initialize gauge metrics from current state (default to 0 on error)
    let channel_count = state
        .session_store
        .list_all()
        .map(|ch| ch.len())
        .unwrap_or(0);
    metrics::set_active_channels(channel_count as u64);

    let active_schedule_count = scheduler_store
        .list_all()
        .map(|s| {
            s.iter()
                .filter(|s| s.status == crate::scheduler::ScheduleStatus::Active)
                .count()
        })
        .unwrap_or(0);
    metrics::set_active_schedules(active_schedule_count as u64);

    let admin_state = AdminState {
        config: Arc::clone(&state.config),
        session_store: state.session_store.clone(),
        scheduler_store: scheduler_store.clone(),
    };

    let admin_routes = admin_router()
        .layer(middleware::from_fn_with_state(
            admin_state.clone(),
            auth_middleware,
        ))
        .with_state(admin_state);

    // Create MCP state with scheduler store and Matrix client
    let mcp_state = McpState {
        session_store: state.session_store.clone(),
        scheduler_store,
        matrix_client: state.matrix_client.clone(),
        timezone: state.config.scheduler.timezone.clone(),
        workspace_path: state.config.workspace.path.clone(),
        room_prefix: state.config.matrix.room_prefix.clone(),
    };

    let mcp_routes = Router::new()
        .route("/mcp", post(mcp_handler))
        .with_state(Arc::new(mcp_state));

    // Metrics endpoint - renders Prometheus text format
    let metrics_routes = Router::new()
        .route("/metrics", get(metrics_handler))
        .with_state(Arc::new(metrics_handle));

    let app = Router::new()
        .route("/", get(|| async { Redirect::permanent("/admin") }))
        .nest("/admin", admin_routes)
        .merge(mcp_routes)
        .merge(webhook_routes)
        .merge(metrics_routes)
        .layer(TraceLayer::new_for_http());

    // Default to localhost, but allow override for Docker (needs 0.0.0.0)
    let bind_addr =
        std::env::var("WEBHOOK_BIND_ADDRESS").unwrap_or_else(|_| "127.0.0.1".to_string());
    let addr = format!("{}:{}", bind_addr, port);
    tracing::info!(addr = %addr, "Starting webhook server");

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .await?;

    Ok(())
}

/// Handle webhook POST requests
async fn webhook_handler(
    State(state): State<Arc<WebhookState>>,
    Path(session_id): Path<String>,
    Json(payload): Json<WebhookRequest>,
) -> (StatusCode, Json<WebhookResponse>) {
    let start_time = std::time::Instant::now();

    let prompt_preview: String = payload.prompt.chars().take(50).collect();
    tracing::info!(
        session_id = %session_id,
        prompt_preview = %prompt_preview,
        "Webhook received"
    );

    // Validate API key if configured
    if let Some(expected_key) = &state.config.webhook.api_key {
        match &payload.api_key {
            Some(provided_key) if provided_key == expected_key => {
                // Valid key, continue
            }
            _ => {
                tracing::warn!(session_id = %session_id, "Webhook authentication failed");
                metrics::record_webhook_request("auth_failed");
                metrics::record_error("webhook_auth");
                return (
                    StatusCode::UNAUTHORIZED,
                    Json(WebhookResponse {
                        success: false,
                        message: "Invalid or missing API key".to_string(),
                    }),
                );
            }
        }
    }

    let prompt_text = payload.prompt;

    // Validate prompt is not empty
    if prompt_text.trim().is_empty() {
        tracing::warn!(session_id = %session_id, "Webhook received empty prompt");
        metrics::record_webhook_request("bad_request");
        metrics::record_error("webhook_empty_prompt");
        return (
            StatusCode::BAD_REQUEST,
            Json(WebhookResponse {
                success: false,
                message: "Prompt cannot be empty".to_string(),
            }),
        );
    }

    // Validate prompt size
    const MAX_WEBHOOK_PROMPT_LENGTH: usize = 64 * 1024; // 64KB
    if prompt_text.len() > MAX_WEBHOOK_PROMPT_LENGTH {
        tracing::warn!(
            session_id = %session_id,
            prompt_len = prompt_text.len(),
            "Webhook prompt exceeds size limit"
        );
        metrics::record_webhook_request("bad_request");
        metrics::record_error("webhook_prompt_too_large");
        return (
            StatusCode::BAD_REQUEST,
            Json(WebhookResponse {
                success: false,
                message: format!("Prompt too large (max {} bytes)", MAX_WEBHOOK_PROMPT_LENGTH),
            }),
        );
    }

    // Look up channel by session ID
    let channel = match state.session_store.get_by_session_id(&session_id) {
        Ok(Some(c)) => c,
        Ok(None) => {
            tracing::warn!(session_id = %session_id, "Session not found");
            metrics::record_webhook_request("not_found");
            metrics::record_error("webhook_session_not_found");
            return (
                StatusCode::NOT_FOUND,
                Json(WebhookResponse {
                    success: false,
                    message: format!("Session not found: {}", session_id),
                }),
            );
        }
        Err(e) => {
            tracing::error!(error = %e, "Database error");
            metrics::record_webhook_request("error");
            metrics::record_error("webhook_database");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(WebhookResponse {
                    success: false,
                    message: format!("Database error: {}", e),
                }),
            );
        }
    };

    // Get Matrix room
    let room_id: OwnedRoomId = match channel.room_id.parse() {
        Ok(id) => id,
        Err(e) => {
            tracing::error!(error = %e, room_id = %channel.room_id, "Invalid room ID");
            metrics::record_webhook_request("error");
            metrics::record_error("webhook_invalid_room_id");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(WebhookResponse {
                    success: false,
                    message: format!("Invalid room ID: {}", e),
                }),
            );
        }
    };

    let Some(room) = state.matrix_client.get_room(&room_id) else {
        tracing::warn!(room_id = %channel.room_id, "Room not found");
        metrics::record_webhook_request("not_found");
        metrics::record_error("webhook_room_not_found");
        return (
            StatusCode::NOT_FOUND,
            Json(WebhookResponse {
                success: false,
                message: format!("Room not found: {}", channel.room_id),
            }),
        );
    };

    // 1. Send webhook prompt to room for visibility
    if let Err(e) = room
        .send(RoomMessageEventContent::text_plain(format!(
            "🤖 Webhook: {}",
            prompt_text
        )))
        .await
    {
        tracing::error!(error = %e, "Failed to send webhook prompt to room");
        metrics::record_webhook_request("error");
        metrics::record_error("webhook_send_failed");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(WebhookResponse {
                success: false,
                message: format!("Failed to send message: {}", e),
            }),
        );
    }
    metrics::record_message_sent();

    let claude_start = std::time::Instant::now();
    metrics::record_claude_invocation("webhook");

    let room_id_for_log = channel.room_id.clone();
    let channel_name_for_log = channel.channel_name.clone();

    let (responder_tx, responder_rx) = oneshot::channel();
    if let Err(e) = state
        .job_tx
        .send(WebhookJob {
            channel: channel.clone(),
            prompt: prompt_text,
            responder: responder_tx,
        })
        .await
    {
        tracing::error!(error = %e, "Warm session worker channel closed");
        metrics::record_webhook_request("error");
        metrics::record_error("webhook_worker_unavailable");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(WebhookResponse {
                success: false,
                message: "Warm session worker unavailable".to_string(),
            }),
        );
    }

    let worker_result = match responder_rx.await {
        Ok(result) => result,
        Err(_) => {
            tracing::error!(
                session_id = %session_id,
                room_id = %channel.room_id,
                channel = %channel.channel_name,
                "Warm session worker dropped response channel"
            );
            metrics::record_webhook_request("error");
            metrics::record_error("webhook_worker_dropped");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(WebhookResponse {
                    success: false,
                    message: "ACP worker failed to respond".to_string(),
                }),
            );
        }
    };

    let worker_response = match worker_result {
        Ok(response) => response,
        Err(e) => {
            tracing::error!(
                room_id = %room_id_for_log,
                channel = %channel_name_for_log,
                error = %e,
                "Warm session worker returned error"
            );
            let error_msg = format!("⚠️ ACP error: {}", e);
            let _ = room
                .send(RoomMessageEventContent::text_plain(&error_msg))
                .await;
            metrics::record_webhook_request("error");
            metrics::record_error("webhook_warm_session");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(WebhookResponse {
                    success: false,
                    message: format!("ACP error: {}", e),
                }),
            );
        }
    };

    let claude_duration = claude_start.elapsed().as_secs_f64();
    metrics::record_claude_duration(claude_duration);
    metrics::record_claude_response_length(worker_response.response_len);

    let chunks = chunk_message(&worker_response.response_text, MAX_CHUNK_SIZE);
    let chunk_count = chunks.len();
    for (i, chunk) in chunks.iter().enumerate() {
        let html = markdown_to_html(chunk);
        if let Err(e) = room
            .send(RoomMessageEventContent::text_html(chunk, &html))
            .await
        {
            tracing::error!(error = %e, chunk = i, "Failed to send Claude response chunk");
            metrics::record_webhook_request("error");
            metrics::record_error("webhook_response_send_failed");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(WebhookResponse {
                    success: false,
                    message: format!("Failed to send response chunk {}: {}", i, e),
                }),
            );
        }
        metrics::record_message_sent();
        log_matrix_message(
            &channel.directory,
            &channel.room_id,
            "webhook_response",
            chunk,
            Some(&html),
            if chunk_count > 1 { Some(i) } else { None },
            if chunk_count > 1 {
                Some(chunk_count)
            } else {
                None
            },
        )
        .await;

        if i < chunks.len() - 1 {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    }

    tracing::info!(
        room_id = %room_id_for_log,
        channel = %channel_name_for_log,
        duration = claude_duration,
        "Webhook processed successfully"
    );

    let total_duration = start_time.elapsed().as_secs_f64();
    metrics::record_webhook_request("success");
    metrics::record_webhook_duration(total_duration);

    (
        StatusCode::OK,
        Json(WebhookResponse {
            success: true,
            message: "Message sent and Claude responded successfully".to_string(),
        }),
    )
}

fn spawn_webhook_worker(
    session_store: SessionStore,
    warm_manager: SharedWarmSessionManager,
) -> WebhookJobSender {
    let (tx, mut rx) = mpsc::channel::<WebhookJob>(32);

    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("Failed to create webhook worker runtime");
        let local = LocalSet::new();

        local.block_on(&rt, async move {
            while let Some(job) = rx.recv().await {
                let session_store = session_store.clone();
                let warm_manager = warm_manager.clone();
                let WebhookJob {
                    channel,
                    prompt,
                    responder,
                } = job;

                let result =
                    process_webhook_job(channel, prompt, session_store, warm_manager).await;
                if responder.send(result).is_err() {
                    tracing::warn!(
                        "Webhook handler dropped before worker response could be delivered"
                    );
                }
            }
        });
    });

    tx
}

async fn process_webhook_job(
    channel: Channel,
    prompt: String,
    session_store: SessionStore,
    warm_manager: SharedWarmSessionManager,
) -> Result<WebhookWorkerResponse> {
    let (session_handle, session_id, is_new_session) =
        prepare_session_async(&warm_manager, &channel).await?;

    if is_new_session {
        if let Err(e) = session_store.update_session_id(&channel.room_id, &session_id) {
            tracing::warn!(
                error = %e,
                room_id = %channel.room_id,
                "Failed to persist new session ID during webhook prep"
            );
        }
    }

    tracing::info!(
        channel = %channel.channel_name,
        session_id = %session_id,
        "Webhook worker sending prompt"
    );

    // Send prompt and get event receiver directly
    let mut event_rx = send_prompt_with_handle(&session_handle, &session_id, &prompt).await?;

    tracing::info!(
        channel = %channel.channel_name,
        session_id = %session_id,
        "Webhook worker prompt started, processing events"
    );

    let mut response = String::new();
    let mut session_id_from_event: Option<String> = None;

    // Add timeout to prevent indefinite waiting
    let timeout_duration = std::time::Duration::from_secs(300); // 5 minutes
    let start = std::time::Instant::now();

    while let Some(event) = event_rx.recv().await {
        // Check for timeout
        if start.elapsed() > timeout_duration {
            tracing::warn!(
                channel = %channel.channel_name,
                session_id = %session_id,
                "Webhook event processing timed out after 5 minutes"
            );
            metrics::record_error("webhook_timeout");
            return Err(anyhow::anyhow!("Request timed out after 5 minutes"));
        }

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
                    channel = %channel.channel_name,
                    tool = %name,
                    preview = %input_preview,
                    "Webhook tool invocation"
                );
            }
            AgentEvent::ToolEnd { .. } => {
                // Tool completion - just log for now
                tracing::debug!("Tool completed");
            }
            AgentEvent::Text(text) => {
                response.push_str(&text);
            }
            AgentEvent::Result { text, .. } => {
                if response.is_empty() {
                    response = text;
                }
                break;
            }
            AgentEvent::Error { code, message, .. } => {
                // Check for session orphaned error
                if code == gorp_agent::ErrorCode::SessionOrphaned {
                    if let Err(e) = session_store.reset_orphaned_session(&channel.room_id) {
                        tracing::error!(
                            error = %e,
                            room_id = %channel.room_id,
                            "Failed to reset invalid session from webhook"
                        );
                    }
                    // Mark session as invalidated FIRST so concurrent users see it
                    {
                        let mut session = session_handle.lock().await;
                        session.set_invalidated(true);
                    }
                    // Evict from warm cache so next request creates fresh session
                    let evicted = {
                        let mut mgr = warm_manager.write().await;
                        mgr.evict(&channel.channel_name)
                    };
                    tracing::info!(
                        channel = %channel.channel_name,
                        evicted = evicted,
                        "Evicted warm session after orphaned session in webhook"
                    );
                    metrics::record_error("invalid_session");
                    return Err(anyhow::anyhow!(
                        "Session was reset (conversation data was lost). Please trigger the webhook again."
                    ));
                }
                metrics::record_error("agent_streaming");
                return Err(anyhow::anyhow!(message));
            }
            AgentEvent::SessionInvalid { reason } => {
                tracing::warn!(reason = %reason, "Session invalid");
                if let Err(e) = session_store.reset_orphaned_session(&channel.room_id) {
                    tracing::error!(
                        error = %e,
                        room_id = %channel.room_id,
                        "Failed to reset invalid session from webhook"
                    );
                }
                // Mark session as invalidated FIRST so concurrent users see it
                {
                    let mut session = session_handle.lock().await;
                    session.set_invalidated(true);
                }
                // Evict from warm cache so next request creates fresh session
                let evicted = {
                    let mut mgr = warm_manager.write().await;
                    mgr.evict(&channel.channel_name)
                };
                tracing::info!(
                    channel = %channel.channel_name,
                    evicted = evicted,
                    "Evicted warm session after invalid session in webhook"
                );
                metrics::record_error("invalid_session");
                return Err(anyhow::anyhow!(
                    "Session was reset (conversation data was lost). Please trigger the webhook again."
                ));
            }
            AgentEvent::SessionChanged { new_session_id } => {
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

    if response.trim().is_empty() {
        metrics::record_error("acp_no_response");
        return Err(anyhow::anyhow!("ACP agent finished without a response"));
    }

    // Update session ID if Claude CLI reported a new one
    if let Some(ref new_sess_id) = session_id_from_event {
        if let Err(e) = session_store.update_session_id(&channel.room_id, new_sess_id) {
            tracing::error!(
                error = %e,
                room_id = %channel.room_id,
                "Failed to update session ID after webhook prompt"
            );
        } else {
            // CRITICAL: Also update the warm session cache to match the database
            {
                let mut session = session_handle.lock().await;
                session.set_session_id(new_sess_id.clone());
            }
            tracing::debug!(
                channel = %channel.channel_name,
                new_session = %new_sess_id,
                "Updated session ID in warm cache (webhook)"
            );
        }
    }
    if let Err(e) = session_store.mark_started(&channel.room_id) {
        tracing::error!(
            error = %e,
            room_id = %channel.room_id,
            "Failed to mark session as started after webhook"
        );
    }

    let response_len = response.len();
    tracing::info!(
        channel = %channel.channel_name,
        response_len,
        "Webhook worker completed prompt"
    );

    Ok(WebhookWorkerResponse {
        response_text: response,
        response_len,
    })
}

/// Handle GET /metrics - returns Prometheus text format
async fn metrics_handler(State(handle): State<Arc<PrometheusHandle>>) -> impl IntoResponse {
    handle.render()
}
