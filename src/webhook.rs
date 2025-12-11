// ABOUTME: HTTP webhook server for injecting prompts into Claude sessions
// ABOUTME: Provides POST /webhook/session/:id endpoint for external triggers like cron jobs

use anyhow::Result;
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
    admin::{admin_router, auth_middleware, AdminState},
    claude,
    config::Config,
    mcp::{mcp_handler, McpState},
    scheduler::SchedulerStore,
    session::SessionStore,
    utils::{chunk_message, log_matrix_message, markdown_to_html, MAX_CHUNK_SIZE},
};

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
    let state = WebhookState {
        session_store,
        matrix_client,
        config,
    };

    let webhook_routes = Router::new()
        .route("/webhook/session/:session_id", post(webhook_handler))
        .with_state(Arc::new(state.clone()));

    let admin_state = AdminState {
        config: Arc::clone(&state.config),
    };

    let admin_routes = admin_router()
        .layer(middleware::from_fn_with_state(
            admin_state.clone(),
            auth_middleware,
        ))
        .with_state(admin_state);

    // Create MCP state with scheduler store and Matrix client
    let scheduler_store = SchedulerStore::new(state.session_store.db_connection());
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

    let app = Router::new()
        .route("/", get(|| async { Redirect::permanent("/admin") }))
        .nest("/admin", admin_routes)
        .merge(mcp_routes)
        .merge(webhook_routes)
        .layer(TraceLayer::new_for_http());

    let addr = format!("127.0.0.1:{}", port);
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
) -> impl IntoResponse {
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
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(WebhookResponse {
                success: false,
                message: format!("Failed to send message: {}", e),
            }),
        );
    }

    // 2. Invoke Claude directly
    let claude_response = match claude::invoke_claude(
        &state.config.claude.binary_path,
        state.config.claude.sdk_url.as_deref(),
        channel.cli_args(),
        &payload.prompt,
        Some(&channel.directory),
    )
    .await
    {
        Ok(resp) => resp,
        Err(e) => {
            tracing::error!(error = %e, "Claude invocation failed");
            let error_msg = format!("⚠️ Claude error: {}", e);
            let _ = room
                .send(RoomMessageEventContent::text_plain(&error_msg))
                .await;
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(WebhookResponse {
                    success: false,
                    message: format!("Claude error: {}", e),
                }),
            );
        }
    };

    // 3. Send Claude's response to room with markdown formatting and chunking
    let chunks = chunk_message(&claude_response, MAX_CHUNK_SIZE);
    let chunk_count = chunks.len();
    for (i, chunk) in chunks.iter().enumerate() {
        let html = markdown_to_html(chunk);
        if let Err(e) = room
            .send(RoomMessageEventContent::text_html(chunk, &html))
            .await
        {
            tracing::error!(error = %e, chunk = i, "Failed to send Claude response chunk");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(WebhookResponse {
                    success: false,
                    message: format!("Failed to send response chunk {}: {}", i, e),
                }),
            );
        }

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

    // 4. Mark session as started
    if let Err(e) = state.session_store.mark_started(&channel.room_id) {
        tracing::error!(error = %e, "Failed to mark session as started");
        // Don't fail the request - message was sent successfully
    }

    tracing::info!(
        session_id = %session_id,
        room_id = %channel.room_id,
        "Webhook processed successfully"
    );

    (
        StatusCode::OK,
        Json(WebhookResponse {
            success: true,
            message: "Message sent and Claude responded successfully".to_string(),
        }),
    )
}
