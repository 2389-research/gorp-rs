// ABOUTME: MCP (Model Context Protocol) server for gorp tools
// ABOUTME: Exposes scheduling and attachment tools to Claude via HTTP MCP endpoint

use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;

use matrix_sdk::Client;

use crate::scheduler::{
    parse_time_expression, ParsedSchedule, ScheduleStatus, ScheduledPrompt, SchedulerStore,
};
use crate::session::SessionStore;

/// MCP server state shared with handlers
#[derive(Clone)]
pub struct McpState {
    pub session_store: SessionStore,
    pub scheduler_store: SchedulerStore,
    pub matrix_client: Client,
    pub timezone: String,
}

/// JSON-RPC request structure
#[derive(Debug, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: Option<Value>,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

/// JSON-RPC response structure
#[derive(Debug, Serialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

#[derive(Debug, Serialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

/// Tool definition for MCP
#[derive(Debug, Serialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    #[serde(rename = "inputSchema")]
    pub input_schema: Value,
}

/// Context from the workspace directory (written by message_handler before Claude invocation)
#[derive(Debug, Deserialize)]
struct WorkspaceContext {
    room_id: String,
    channel_name: String,
    #[allow(dead_code)]
    session_id: String,
}

/// Read context file from a workspace directory
/// Returns None if file doesn't exist or can't be parsed
fn read_context_file(workspace_dir: &str) -> Option<WorkspaceContext> {
    let context_path = std::path::Path::new(workspace_dir)
        .join(".matrix")
        .join("context.json");

    let content = std::fs::read_to_string(&context_path).ok()?;
    serde_json::from_str(&content).ok()
}

/// Try to find the workspace directory from environment or common locations
fn find_workspace_dir() -> Option<String> {
    // Check PWD first (Claude runs from workspace directory)
    if let Ok(pwd) = std::env::var("PWD") {
        let context_path = std::path::Path::new(&pwd)
            .join(".matrix")
            .join("context.json");
        if context_path.exists() {
            return Some(pwd);
        }
    }

    // Check current working directory
    if let Ok(cwd) = std::env::current_dir() {
        let context_path = cwd.join(".matrix").join("context.json");
        if context_path.exists() {
            return Some(cwd.to_string_lossy().to_string());
        }
    }

    None
}

/// Get list of available tools
fn get_tools() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "schedule_prompt".to_string(),
            description: "Schedule a prompt to be executed at a future time. The prompt will be sent to the current channel and processed by Claude.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "prompt": {
                        "type": "string",
                        "description": "The prompt text to execute at the scheduled time"
                    },
                    "execute_at": {
                        "type": "string",
                        "description": "When to execute. Supports: 'in 5 minutes', 'tomorrow 9am', 'every monday 8am', 'every day at 3pm'"
                    },
                    "channel_name": {
                        "type": "string",
                        "description": "Channel to schedule for (optional, defaults to current channel from context)"
                    }
                },
                "required": ["prompt", "execute_at"]
            }),
        },
        ToolDefinition {
            name: "send_attachment".to_string(),
            description: "Send a file or image to the Matrix chat room. The file must exist in the workspace directory.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "file_path": {
                        "type": "string",
                        "description": "Path to the file relative to the workspace directory"
                    },
                    "caption": {
                        "type": "string",
                        "description": "Optional caption/message to send with the file"
                    },
                    "room_id": {
                        "type": "string",
                        "description": "Matrix room ID (optional, defaults to current room from context)"
                    }
                },
                "required": ["file_path"]
            }),
        },
    ]
}

/// Handle MCP JSON-RPC requests
pub async fn mcp_handler(
    State(state): State<Arc<McpState>>,
    Json(request): Json<JsonRpcRequest>,
) -> impl IntoResponse {
    tracing::debug!(method = %request.method, "MCP request received");

    let response = match request.method.as_str() {
        "initialize" => handle_initialize(&request),
        "notifications/initialized" => handle_initialized_notification(&request),
        "tools/list" => handle_tools_list(&request),
        "tools/call" => handle_tools_call(&state, &request).await,
        _ => JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: request.id,
            result: None,
            error: Some(JsonRpcError {
                code: -32601,
                message: format!("Method not found: {}", request.method),
                data: None,
            }),
        },
    };

    (StatusCode::OK, Json(response))
}

/// Handle MCP initialize request
fn handle_initialize(request: &JsonRpcRequest) -> JsonRpcResponse {
    tracing::info!("MCP initialize request received");
    JsonRpcResponse {
        jsonrpc: "2.0".to_string(),
        id: request.id.clone(),
        result: Some(json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {
                "tools": {}
            },
            "serverInfo": {
                "name": "gorp",
                "version": env!("CARGO_PKG_VERSION")
            }
        })),
        error: None,
    }
}

/// Handle MCP initialized notification (no response needed for notifications)
fn handle_initialized_notification(request: &JsonRpcRequest) -> JsonRpcResponse {
    tracing::info!("MCP initialized notification received");
    // Notifications don't require a response, but we return success anyway
    JsonRpcResponse {
        jsonrpc: "2.0".to_string(),
        id: request.id.clone(),
        result: Some(json!({})),
        error: None,
    }
}

/// Handle tools/list request
fn handle_tools_list(request: &JsonRpcRequest) -> JsonRpcResponse {
    JsonRpcResponse {
        jsonrpc: "2.0".to_string(),
        id: request.id.clone(),
        result: Some(json!({
            "tools": get_tools()
        })),
        error: None,
    }
}

/// Handle tools/call request
async fn handle_tools_call(state: &McpState, request: &JsonRpcRequest) -> JsonRpcResponse {
    let params = &request.params;

    let tool_name = params
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let arguments = params
        .get("arguments")
        .cloned()
        .unwrap_or(json!({}));

    tracing::info!(tool = %tool_name, "MCP tool call");

    let result = match tool_name {
        "schedule_prompt" => handle_schedule_prompt(state, &arguments).await,
        "send_attachment" => handle_send_attachment(state, &arguments).await,
        _ => Err(format!("Unknown tool: {}", tool_name)),
    };

    match result {
        Ok(content) => JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: request.id.clone(),
            result: Some(json!({
                "content": [{
                    "type": "text",
                    "text": content
                }]
            })),
            error: None,
        },
        Err(error) => JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: request.id.clone(),
            result: Some(json!({
                "content": [{
                    "type": "text",
                    "text": error
                }],
                "isError": true
            })),
            error: None,
        },
    }
}

/// Handle gorp_schedule_prompt tool call
async fn handle_schedule_prompt(state: &McpState, args: &Value) -> Result<String, String> {
    let prompt = args
        .get("prompt")
        .and_then(|v| v.as_str())
        .ok_or("Missing required parameter: prompt")?;

    let execute_at = args
        .get("execute_at")
        .and_then(|v| v.as_str())
        .ok_or("Missing required parameter: execute_at")?;

    let channel_name = args
        .get("channel_name")
        .and_then(|v| v.as_str());

    // If no channel specified, try to read from context file
    let channel_name = match channel_name {
        Some(name) => name.to_string(),
        None => {
            // Try to find and read context file
            if let Some(workspace_dir) = find_workspace_dir() {
                if let Some(ctx) = read_context_file(&workspace_dir) {
                    tracing::debug!(channel = %ctx.channel_name, "Using channel from context file");
                    ctx.channel_name
                } else {
                    return Err("channel_name is required (context file not readable)".to_string());
                }
            } else {
                return Err("channel_name is required (no context file found)".to_string());
            }
        }
    };

    // Look up the channel
    let channel = state
        .session_store
        .get_by_name(&channel_name)
        .map_err(|e| format!("Database error: {}", e))?
        .ok_or_else(|| format!("Channel not found: {}", channel_name))?;

    // Parse the time expression
    let parsed = parse_time_expression(execute_at, &state.timezone)
        .map_err(|e| format!("Invalid time expression: {}", e))?;

    let (execute_at_str, cron_expr, next_execution) = match parsed {
        ParsedSchedule::OneTime(dt) => (Some(dt.to_rfc3339()), None, dt.to_rfc3339()),
        ParsedSchedule::Recurring { cron, next } => (None, Some(cron), next.to_rfc3339()),
    };

    // Create the schedule
    let schedule_id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();

    let schedule = ScheduledPrompt {
        id: schedule_id.clone(),
        channel_name: channel.channel_name.clone(),
        room_id: channel.room_id.clone(),
        prompt: prompt.to_string(),
        created_by: "claude".to_string(), // Created by Claude via MCP
        created_at: now,
        execute_at: execute_at_str,
        cron_expression: cron_expr.clone(),
        last_executed_at: None,
        next_execution_at: next_execution.clone(),
        status: ScheduleStatus::Active,
        error_message: None,
        execution_count: 0,
    };

    state
        .scheduler_store
        .create_schedule(&schedule)
        .map_err(|e| format!("Failed to create schedule: {}", e))?;

    let schedule_type = if cron_expr.is_some() {
        "recurring"
    } else {
        "one-time"
    };

    Ok(format!(
        "Scheduled {} prompt for channel '{}'\nID: {}\nNext execution: {}",
        schedule_type, channel.channel_name, schedule_id, next_execution
    ))
}

/// Handle gorp_send_attachment tool call
async fn handle_send_attachment(state: &McpState, args: &Value) -> Result<String, String> {
    use matrix_sdk::ruma::{
        events::room::message::{
            FileMessageEventContent, ImageMessageEventContent, RoomMessageEventContent,
        },
        OwnedRoomId,
    };
    use std::path::Path;

    let file_path = args
        .get("file_path")
        .and_then(|v| v.as_str())
        .ok_or("Missing required parameter: file_path")?;

    let caption = args.get("caption").and_then(|v| v.as_str());
    let room_id_str = args.get("room_id").and_then(|v| v.as_str());

    // If no room_id specified, try to read from context file
    let room_id_str = match room_id_str {
        Some(id) => id.to_string(),
        None => {
            // Try to find and read context file
            if let Some(workspace_dir) = find_workspace_dir() {
                if let Some(ctx) = read_context_file(&workspace_dir) {
                    tracing::debug!(room_id = %ctx.room_id, "Using room_id from context file");
                    ctx.room_id
                } else {
                    return Err("room_id is required (context file not readable)".to_string());
                }
            } else {
                return Err("room_id is required (no context file found)".to_string());
            }
        }
    };

    let room_id: OwnedRoomId = room_id_str
        .parse()
        .map_err(|e| format!("Invalid room_id: {}", e))?;

    // Get the room
    let room = state
        .matrix_client
        .get_room(&room_id)
        .ok_or_else(|| format!("Room not found: {}", room_id_str))?;

    // Validate file exists
    let path = Path::new(file_path);
    if !path.exists() {
        return Err(format!("File not found: {}", file_path));
    }

    // Read file contents
    let file_data = tokio::fs::read(path)
        .await
        .map_err(|e| format!("Failed to read file: {}", e))?;

    let filename = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("attachment")
        .to_string();

    // Detect MIME type from extension
    let mime_type = mime_guess::from_path(path)
        .first()
        .unwrap_or(mime_guess::mime::APPLICATION_OCTET_STREAM);

    let is_image = mime_type.type_() == "image";

    // Upload to Matrix
    let upload_response = state
        .matrix_client
        .media()
        .upload(&mime_type, file_data)
        .await
        .map_err(|e| format!("Failed to upload to Matrix: {}", e))?;

    // Create message content based on type
    let content = if is_image {
        let image_content = ImageMessageEventContent::plain(
            caption.unwrap_or(&filename).to_string(),
            upload_response.content_uri,
        );
        RoomMessageEventContent::new(matrix_sdk::ruma::events::room::message::MessageType::Image(
            image_content,
        ))
    } else {
        let mut file_content = FileMessageEventContent::plain(
            caption.unwrap_or(&filename).to_string(),
            upload_response.content_uri,
        );
        file_content.filename = Some(filename.clone());
        RoomMessageEventContent::new(matrix_sdk::ruma::events::room::message::MessageType::File(
            file_content,
        ))
    };

    // Send to room
    room.send(content)
        .await
        .map_err(|e| format!("Failed to send message: {}", e))?;

    let type_str = if is_image { "image" } else { "file" };
    Ok(format!(
        "Successfully sent {} '{}' to room {}",
        type_str, filename, room_id_str
    ))
}
