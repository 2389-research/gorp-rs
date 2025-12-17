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
use tower_http::trace::TraceLayer;

use crate::{
    acp_client::{AcpClient, AcpEvent},
    admin::{admin_router, auth_middleware, AdminState},
    config::Config,
    mcp::{mcp_handler, McpState},
    metrics,
    scheduler::SchedulerStore,
    session::SessionStore,
    utils::{chunk_message, log_matrix_message, markdown_to_html, MAX_CHUNK_SIZE},
};
use metrics_exporter_prometheus::PrometheusHandle;

#[derive(Clone)]
pub struct WebhookState {
    pub session_store: SessionStore,
    pub matrix_client: Client,
    pub config: Arc<Config>,
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

/// Start the webhook HTTP server
pub async fn start_webhook_server(
    port: u16,
    session_store: SessionStore,
    matrix_client: Client,
    config: Arc<Config>,
) -> Result<()> {
    // Initialize Prometheus metrics
    let metrics_handle =
        metrics::init_metrics().context("Failed to initialize Prometheus metrics")?;

    let state = WebhookState {
        session_store,
        matrix_client,
        config,
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

    tracing::info!(
        session_id = %session_id,
        prompt_preview = %payload.prompt.chars().take(50).collect::<String>(),
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

    // Validate prompt is not empty
    if payload.prompt.trim().is_empty() {
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
        .send(RoomMessageEventContent::text_plain(&format!(
            "🤖 Webhook: {}",
            payload.prompt
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

    // 2. Invoke ACP agent directly
    let claude_start = std::time::Instant::now();
    metrics::record_claude_invocation("webhook");

    // Check if agent binary is configured
    let agent_binary = match state.config.acp.agent_binary.as_ref() {
        Some(b) => b.clone(),
        None => {
            tracing::error!("ACP agent binary not configured");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(WebhookResponse {
                    success: false,
                    message: "ACP agent binary not configured".to_string(),
                }),
            );
        }
    };

    let acp_response = {
        let working_dir = std::path::Path::new(&channel.directory).to_path_buf();
        let session_id = channel.session_id.clone();
        let prompt_text = payload.prompt.clone();
        let started = channel.started;
        let agent_binary_clone = agent_binary.clone();

        // ACP requires spawn_local which isn't Send, so use spawn_blocking with a new runtime
        tokio::task::spawn_blocking(move || {
            let rt = match tokio::runtime::Runtime::new() {
                Ok(rt) => rt,
                Err(e) => {
                    tracing::error!(error = %e, "Failed to create tokio runtime for webhook");
                    return Err(anyhow::anyhow!("Failed to create runtime: {}", e));
                }
            };
            rt.block_on(async {
                let local = tokio::task::LocalSet::new();
                local.run_until(async move {
            // Spawn ACP client
            let client = match AcpClient::spawn(&working_dir, &agent_binary_clone).await {
                Ok(c) => c,
                Err(e) => {
                    tracing::error!(error = %e, "Failed to spawn ACP client for webhook");
                    return Err(anyhow::anyhow!("Failed to spawn ACP client: {}", e));
                }
            };

            // Initialize ACP connection
            if let Err(e) = client.initialize().await {
                tracing::error!(error = %e, "Failed to initialize ACP for webhook");
                return Err(anyhow::anyhow!("Failed to initialize ACP: {}", e));
            }

            // Create or load session
            let (active_session_id, session_changed) = if !started {
                match client.new_session().await {
                    Ok(new_id) => {
                        tracing::info!(session_id = %new_id, "Created new ACP session for webhook");
                        (new_id, true)
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "Failed to create new ACP session for webhook");
                        return Err(anyhow::anyhow!("Failed to create session: {}", e));
                    }
                }
            } else {
                if let Err(e) = client.load_session(&session_id).await {
                    tracing::warn!(error = %e, session_id = %session_id, "Failed to load session for webhook, creating new one");
                    match client.new_session().await {
                        Ok(new_id) => {
                            tracing::info!(session_id = %new_id, "Created new ACP session for webhook after load failure");
                            (new_id, true)
                        }
                        Err(e2) => {
                            tracing::error!(error = %e2, "Failed to create fallback session for webhook");
                            return Err(anyhow::anyhow!("Failed to create session: {}", e2));
                        }
                    }
                } else {
                    (session_id.clone(), false)
                }
            };

            // Send prompt and collect response
            let mut rx = match client.prompt(&active_session_id, &prompt_text).await {
                Ok(rx) => rx,
                Err(e) => {
                    tracing::error!(error = %e, "Failed to send prompt for webhook");
                    return Err(anyhow::anyhow!("Failed to send prompt: {}", e));
                }
            };

            let mut response = String::new();
            while let Some(event) = rx.recv().await {
                match event {
                    AcpEvent::Text(text) => {
                        response.push_str(&text);
                    }
                    AcpEvent::Result { text } => {
                        if response.is_empty() {
                            response = text;
                        }
                    }
                    AcpEvent::Error(e) => {
                        return Err(anyhow::anyhow!("ACP error: {}", e));
                    }
                    AcpEvent::InvalidSession => {
                        return Err(anyhow::anyhow!("Invalid session"));
                    }
                    AcpEvent::ToolUse { .. } | AcpEvent::SessionChanged { .. } => {
                        // Ignore tool use and session change events in webhook context
                        // (session changes are tracked via local variable)
                    }
                }
            }

            Ok((response, active_session_id, session_changed))
                }).await
            })
        }).await
    };

    let acp_response = match acp_response {
        Ok(result) => result,
        Err(e) => {
            tracing::error!(error = %e, "Failed to spawn ACP blocking task");
            metrics::record_webhook_request("error");
            metrics::record_error("webhook_spawn_blocking");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(WebhookResponse {
                    success: false,
                    message: format!("Internal error spawning ACP task: {}", e),
                }),
            );
        }
    };

    let (acp_response, new_session_id, session_changed) = match acp_response {
        Ok((resp, sess_id, changed)) => {
            let claude_duration = claude_start.elapsed().as_secs_f64();
            metrics::record_claude_duration(claude_duration);
            metrics::record_claude_response_length(resp.len());
            (resp, sess_id, changed)
        }
        Err(e) => {
            tracing::error!(error = %e, "ACP invocation failed");
            metrics::record_webhook_request("error");
            metrics::record_error("webhook_acp_failed");
            let error_msg = format!("⚠️ ACP error: {}", e);
            let _ = room
                .send(RoomMessageEventContent::text_plain(&error_msg))
                .await;
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(WebhookResponse {
                    success: false,
                    message: format!("ACP error: {}", e),
                }),
            );
        }
    };

    // 3. Send ACP's response to room with markdown formatting and chunking
    let chunks = chunk_message(&acp_response, MAX_CHUNK_SIZE);
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

        // Log the Matrix message
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

        // Small delay between chunks for ordering
        if i < chunks.len() - 1 {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    }

    // 4. Update session ID if a new one was created, then mark session as started
    if session_changed {
        if let Err(e) = state
            .session_store
            .update_session_id(&channel.room_id, &new_session_id)
        {
            tracing::error!(error = %e, "Failed to update session ID");
            // Don't fail the request - message was sent successfully
        }
    }
    if let Err(e) = state.session_store.mark_started(&channel.room_id) {
        tracing::error!(error = %e, "Failed to mark session as started");
        // Don't fail the request - message was sent successfully
    }

    tracing::info!(
        session_id = %session_id,
        room_id = %channel.room_id,
        "Webhook processed successfully"
    );

    // Record success metrics
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

/// Handle GET /metrics - returns Prometheus text format
async fn metrics_handler(State(handle): State<Arc<PrometheusHandle>>) -> impl IntoResponse {
    handle.render()
}
