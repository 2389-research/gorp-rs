// ABOUTME: HTTP webhook server for injecting prompts into Claude sessions
// ABOUTME: Provides POST /webhook/session/{id} endpoint for external triggers like cron jobs

use anyhow::{Context, Result};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
#[cfg(feature = "admin")]
use axum::{middleware, response::Redirect};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tower_http::trace::TraceLayer;

#[cfg(feature = "admin")]
use crate::admin::{
    admin_router, auth_middleware, login_router, setup_guard_middleware, setup_router, ws_handler,
    AdminState, WsHub,
};
use crate::{
    bus::{BusMessage, MessageBus, MessageSource, ResponseContent, SessionTarget},
    config::Config,
    mcp::{mcp_handler, McpState},
    metrics,
    scheduler::SchedulerStore,
    session::SessionStore,
};
use metrics_exporter_prometheus::PrometheusHandle;

#[derive(Clone)]
struct WebhookState {
    session_store: SessionStore,
    bus: Arc<MessageBus>,
    config: Arc<Config>,
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
    bus: Arc<MessageBus>,
    config: Arc<Config>,
    registry: crate::platform::SharedPlatformRegistry,
) -> Result<()> {
    // Initialize Prometheus metrics
    let metrics_handle =
        metrics::init_metrics().context("Failed to initialize Prometheus metrics")?;

    let state = WebhookState {
        session_store,
        bus,
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

    #[cfg(feature = "admin")]
    let auth_config = {
        let data_dir = crate::paths::data_dir();
        let data_dir_str = data_dir.to_string_lossy();
        match crate::admin::AuthConfig::load(&data_dir_str) {
            Ok(config) => config,
            Err(e) => {
                tracing::warn!(error = %e, "Failed to load auth config, starting without auth");
                None
            }
        }
    };

    #[cfg(feature = "admin")]
    let ws_hub = WsHub::new();

    #[cfg(feature = "admin")]
    let admin_state = AdminState {
        config: Arc::clone(&state.config),
        session_store: state.session_store.clone(),
        scheduler_store: scheduler_store.clone(),
        auth_config: std::sync::Arc::new(tokio::sync::RwLock::new(auth_config)),
        ws_hub: ws_hub.clone(),
        registry: Some(registry.clone()),
        bus: None,
    };

    // Spawn platform status monitor — polls registry every 5 seconds
    // and broadcasts status changes via WebSocket
    #[cfg(feature = "admin")]
    {
        let monitor_registry = registry.clone();
        let monitor_hub = ws_hub;
        tokio::spawn(async move {
            use crate::admin::websocket::{PlatformStatusData, ServerMessage};
            use gorp_core::PlatformConnectionState;

            let mut prev_states: std::collections::HashMap<String, String> =
                std::collections::HashMap::new();

            loop {
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;

                let health = monitor_registry.read().await.health();
                for h in &health {
                    let state_str = match &h.state {
                        PlatformConnectionState::Connected => "connected",
                        PlatformConnectionState::Connecting => "connecting",
                        PlatformConnectionState::Disconnected { .. } => "disconnected",
                        PlatformConnectionState::AuthRequired => "auth_required",
                        PlatformConnectionState::RateLimited { .. } => "rate_limited",
                    };

                    let changed = prev_states
                        .get(&h.platform_id)
                        .map_or(true, |prev| prev != state_str);

                    if changed {
                        prev_states
                            .insert(h.platform_id.clone(), state_str.to_string());
                        monitor_hub.broadcast(ServerMessage::StatusPlatform {
                            data: PlatformStatusData {
                                platform: h.platform_id.clone(),
                                state: state_str.to_string(),
                            },
                        });
                    }
                }
            }
        });
    }

    #[cfg(feature = "admin")]
    let admin_routes = admin_router()
        .layer(middleware::from_fn_with_state(
            admin_state.clone(),
            auth_middleware,
        ))
        .layer(middleware::from_fn_with_state(
            admin_state.clone(),
            setup_guard_middleware,
        ))
        .with_state(admin_state.clone());

    // Create MCP state with scheduler store (Matrix client not available in webhook context)
    let mcp_state = McpState {
        session_store: state.session_store.clone(),
        scheduler_store,
        matrix_client: None,
        timezone: state.config.scheduler.timezone.clone(),
        workspace_path: state.config.workspace.path.clone(),
        room_prefix: state.config.matrix.as_ref().map(|m| m.room_prefix.clone()).unwrap_or_else(|| "Claude".to_string()),
    };

    let mcp_routes = Router::new()
        .route("/mcp", post(mcp_handler))
        .with_state(Arc::new(mcp_state));

    // Metrics endpoint - renders Prometheus text format
    let metrics_routes = Router::new()
        .route("/metrics", get(metrics_handler))
        .with_state(Arc::new(metrics_handle));

    // Setup and login routes are outside auth middleware (unauthenticated access)
    #[cfg(feature = "admin")]
    let setup_routes = setup_router().with_state(admin_state.clone());
    #[cfg(feature = "admin")]
    let login_routes = login_router().with_state(admin_state.clone());

    // WebSocket route (authenticated via same auth middleware as admin routes)
    #[cfg(feature = "admin")]
    let ws_routes = Router::new()
        .route("/admin/ws", get(ws_handler))
        .layer(middleware::from_fn_with_state(
            admin_state.clone(),
            auth_middleware,
        ))
        .layer(middleware::from_fn_with_state(
            admin_state.clone(),
            setup_guard_middleware,
        ))
        .with_state(admin_state.clone());

    // Session layer for cookie-based authentication
    #[cfg(feature = "admin")]
    let session_store = tower_sessions::MemoryStore::default();
    #[cfg(feature = "admin")]
    let session_layer = tower_sessions::SessionManagerLayer::new(session_store)
        .with_secure(false) // Allow HTTP for local dev; production should use HTTPS
        .with_same_site(tower_sessions::cookie::SameSite::Lax);

    #[cfg(feature = "admin")]
    let app = Router::new()
        .route("/", get(|| async { Redirect::permanent("/admin") }))
        .route(
            "/static/ws.js",
            get(|| async {
                (
                    [("content-type", "application/javascript")],
                    include_str!("../static/ws.js"),
                )
            }),
        )
        .route(
            "/static/feed.js",
            get(|| async {
                (
                    [("content-type", "application/javascript")],
                    include_str!("../static/feed.js"),
                )
            }),
        )
        .route(
            "/static/chat.js",
            get(|| async {
                (
                    [("content-type", "application/javascript")],
                    include_str!("../static/chat.js"),
                )
            }),
        )
        .nest("/admin", admin_routes)
        .nest("/setup", setup_routes)
        .nest("/login", login_routes)
        .merge(ws_routes)
        .merge(mcp_routes)
        .merge(webhook_routes)
        .merge(metrics_routes)
        .layer(session_layer)
        .layer(TraceLayer::new_for_http());

    #[cfg(not(feature = "admin"))]
    let app = Router::new()
        .route(
            "/",
            get(|| async {
                Json(serde_json::json!({
                    "status": "ok",
                    "message": "gorp webhook server"
                }))
            }),
        )
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

    // Publish to the message bus
    let msg = BusMessage {
        id: uuid::Uuid::new_v4().to_string(),
        source: MessageSource::Api { token_hint: "webhook".to_string() },
        session_target: SessionTarget::Session { name: channel.channel_name.clone() },
        sender: "webhook".to_string(),
        body: prompt_text,
        timestamp: Utc::now(),
    };

    metrics::record_claude_invocation("webhook");

    // Subscribe to responses BEFORE publishing to avoid races
    let mut response_rx = state.bus.subscribe_responses();
    let target_session = channel.channel_name.clone();

    state.bus.publish_inbound(msg);

    // Wait for the complete response from the orchestrator
    let timeout = std::time::Duration::from_secs(300); // 5 minute timeout
    let response_text = match tokio::time::timeout(timeout, async {
        let mut accumulated = String::new();
        loop {
            match response_rx.recv().await {
                Ok(resp) if resp.session_name == target_session => {
                    match resp.content {
                        ResponseContent::Chunk(text) => {
                            accumulated.push_str(&text);
                        }
                        ResponseContent::Complete(text) => {
                            // If we accumulated chunks, use those; otherwise use the complete text
                            if accumulated.is_empty() {
                                break Ok(text);
                            } else {
                                break Ok(accumulated);
                            }
                        }
                        ResponseContent::Error(err) => {
                            break Err(anyhow::anyhow!(err));
                        }
                        ResponseContent::SystemNotice(_) => {
                            // Ignore system notices
                        }
                    }
                }
                Ok(_) => {
                    // Response for a different session, ignore
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!(skipped = n, "Webhook response listener lagged");
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    break Err(anyhow::anyhow!("Message bus closed"));
                }
            }
        }
    }).await {
        Ok(Ok(text)) => text,
        Ok(Err(e)) => {
            // Agent/bus error
            metrics::record_webhook_request("error");
            metrics::record_error("webhook_agent");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(WebhookResponse {
                    success: false,
                    message: format!("Agent error: {}", e),
                }),
            );
        }
        Err(_) => {
            // Timeout
            metrics::record_webhook_request("error");
            metrics::record_error("webhook_timeout");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(WebhookResponse {
                    success: false,
                    message: "Request timed out after 5 minutes".to_string(),
                }),
            );
        }
    };

    let total_duration = start_time.elapsed().as_secs_f64();
    metrics::record_webhook_request("success");
    metrics::record_webhook_duration(total_duration);
    metrics::record_claude_response_length(response_text.len());

    (
        StatusCode::OK,
        Json(WebhookResponse {
            success: true,
            message: response_text,
        }),
    )
}

/// Handle GET /metrics - returns Prometheus text format
async fn metrics_handler(State(handle): State<Arc<PrometheusHandle>>) -> impl IntoResponse {
    handle.render()
}
