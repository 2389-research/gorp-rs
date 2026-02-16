// ABOUTME: MCP (Model Context Protocol) server for gorp tools
// ABOUTME: Exposes scheduling and attachment tools to Claude via HTTP MCP endpoint

use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;

use matrix_sdk::Client;

use crate::matrix_client;
use crate::scheduler::{
    parse_time_expression, ParsedSchedule, ScheduleStatus, ScheduledPrompt, SchedulerStore,
};
use crate::session::SessionStore;

/// MCP server state shared with handlers
#[derive(Clone)]
pub struct McpState {
    pub session_store: SessionStore,
    pub scheduler_store: SchedulerStore,
    pub matrix_client: Option<Client>,
    pub timezone: String,
    pub workspace_path: String,
    pub room_prefix: String,
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
        .join(".gorp")
        .join("context.json");

    let content = std::fs::read_to_string(&context_path).ok()?;
    serde_json::from_str(&content).ok()
}

/// Try to find the workspace directory from environment or common locations
fn find_workspace_dir() -> Option<String> {
    // Check PWD first (Claude runs from workspace directory)
    if let Ok(pwd) = std::env::var("PWD") {
        let context_path = std::path::Path::new(&pwd)
            .join(".gorp")
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
        // Scheduling tools
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
            name: "list_schedules".to_string(),
            description: "List all scheduled prompts for a channel.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "channel_name": {
                        "type": "string",
                        "description": "Channel to list schedules for (optional, defaults to current channel)"
                    },
                    "include_completed": {
                        "type": "boolean",
                        "description": "Include completed one-time schedules (default: false)"
                    }
                },
                "required": []
            }),
        },
        ToolDefinition {
            name: "cancel_schedule".to_string(),
            description: "Cancel a scheduled prompt by its ID.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "schedule_id": {
                        "type": "string",
                        "description": "The schedule ID to cancel"
                    }
                },
                "required": ["schedule_id"]
            }),
        },
        // Attachment tools
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
        // Channel management tools
        ToolDefinition {
            name: "get_status".to_string(),
            description: "Get status information about the current channel including room ID, session state, debug mode, and webhook URL.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "channel_name": {
                        "type": "string",
                        "description": "Channel to get status for (optional, defaults to current channel)"
                    }
                },
                "required": []
            }),
        },
        ToolDefinition {
            name: "list_channels".to_string(),
            description: "List all registered channels/rooms.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
        },
        ToolDefinition {
            name: "set_debug".to_string(),
            description: "Enable or disable debug mode for a channel. Debug mode shows tool usage in Matrix chat.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "enabled": {
                        "type": "boolean",
                        "description": "true to enable debug mode, false to disable"
                    },
                    "channel_name": {
                        "type": "string",
                        "description": "Channel to set debug for (optional, defaults to current channel)"
                    }
                },
                "required": ["enabled"]
            }),
        },
        ToolDefinition {
            name: "leave_room".to_string(),
            description: "Make the bot leave a Matrix room. The workspace is preserved.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "channel_name": {
                        "type": "string",
                        "description": "Channel to leave (optional, defaults to current channel)"
                    },
                    "confirm": {
                        "type": "boolean",
                        "description": "Must be true to confirm leaving"
                    }
                },
                "required": ["confirm"]
            }),
        },
        ToolDefinition {
            name: "create_channel".to_string(),
            description: "Create a new Matrix room and channel with its own Claude workspace.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Channel name (alphanumeric, dashes, underscores only)"
                    },
                    "invite_user": {
                        "type": "string",
                        "description": "Matrix user ID to invite (e.g., @user:matrix.org)"
                    }
                },
                "required": ["name"]
            }),
        },
        ToolDefinition {
            name: "invite_to_channel".to_string(),
            description: "Invite a user to an existing channel's Matrix room.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "channel_name": {
                        "type": "string",
                        "description": "Channel to invite to (optional, defaults to current channel)"
                    },
                    "user_id": {
                        "type": "string",
                        "description": "Matrix user ID to invite (e.g., @user:matrix.org)"
                    }
                },
                "required": ["user_id"]
            }),
        },
        ToolDefinition {
            name: "set_room_avatar".to_string(),
            description: "Set or change the room's avatar image. Upload an image file to use as the room icon.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "image_path": {
                        "type": "string",
                        "description": "Path to the image file (relative to workspace)"
                    },
                    "channel_name": {
                        "type": "string",
                        "description": "Channel to set avatar for (optional, defaults to current channel)"
                    }
                },
                "required": ["image_path"]
            }),
        },
        ToolDefinition {
            name: "set_room_topic".to_string(),
            description: "Set or change the room's topic/description.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "topic": {
                        "type": "string",
                        "description": "The new topic/description for the room"
                    },
                    "channel_name": {
                        "type": "string",
                        "description": "Channel to set topic for (optional, defaults to current channel)"
                    }
                },
                "required": ["topic"]
            }),
        },
        // Management reporting tool
        ToolDefinition {
            name: "report_to_management".to_string(),
            description: "Report an issue, concern, bug, or safety problem to human management. Use this to escalate problems, report your own errors or bad behavior, flag safety concerns, or communicate anything that needs human attention. Messages go to a dedicated management channel staffed by humans.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "message": {
                        "type": "string",
                        "description": "The report message - describe the issue, concern, or problem in detail"
                    },
                    "category": {
                        "type": "string",
                        "enum": ["bug", "safety", "concern", "behavior", "error", "feedback", "other"],
                        "description": "Category of the report (optional, defaults to 'other')"
                    },
                    "severity": {
                        "type": "string",
                        "enum": ["low", "medium", "high", "critical"],
                        "description": "Severity level (optional, defaults to 'medium')"
                    }
                },
                "required": ["message"]
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

    let tool_name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");

    let arguments = params.get("arguments").cloned().unwrap_or(json!({}));

    tracing::info!(tool = %tool_name, "MCP tool call");

    let result = match tool_name {
        "schedule_prompt" => handle_schedule_prompt(state, &arguments).await,
        "list_schedules" => handle_list_schedules(state, &arguments),
        "cancel_schedule" => handle_cancel_schedule(state, &arguments),
        "send_attachment" => handle_send_attachment(state, &arguments).await,
        "get_status" => handle_get_status(state, &arguments),
        "list_channels" => handle_list_channels(state),
        "set_debug" => handle_set_debug(state, &arguments),
        "leave_room" => handle_leave_room(state, &arguments).await,
        "create_channel" => handle_create_channel(state, &arguments).await,
        "invite_to_channel" => handle_invite_to_channel(state, &arguments).await,
        "set_room_avatar" => handle_set_room_avatar(state, &arguments).await,
        "set_room_topic" => handle_set_room_topic(state, &arguments).await,
        "report_to_management" => handle_report_to_management(state, &arguments).await,
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

    let channel_name = args.get("channel_name").and_then(|v| v.as_str());

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

    // Get the room (requires Matrix)
    let matrix_client = state.matrix_client.as_ref()
        .ok_or("Matrix not configured — cannot attach files to rooms")?;
    let room = matrix_client
        .get_room(&room_id)
        .ok_or_else(|| format!("Room not found: {}", room_id_str))?;

    // Validate path and prevent directory traversal
    let workspace_root = Path::new(&state.workspace_path);
    let requested_path = Path::new(file_path);

    // Reject paths with ".." to prevent traversal
    if file_path.contains("..") {
        tracing::warn!(
            path = file_path,
            "Path traversal attempt blocked: contains '..'"
        );
        return Err("Invalid path: contains path traversal".to_string());
    }

    // Build the full path relative to workspace
    let full_path = workspace_root.join(requested_path);

    // Canonicalize both paths to resolve symlinks and validate
    let canonical_workspace = workspace_root
        .canonicalize()
        .map_err(|e| format!("Workspace path error: {}", e))?;

    let canonical_full = full_path
        .canonicalize()
        .map_err(|e| format!("File not found: {}", e))?;

    // Verify the resolved path is within workspace
    if !canonical_full.starts_with(&canonical_workspace) {
        tracing::warn!(
            requested_path = file_path,
            resolved_path = %canonical_full.display(),
            workspace_root = %canonical_workspace.display(),
            "Path traversal attempt blocked: resolved path outside workspace"
        );
        return Err("Access denied: path outside workspace".to_string());
    }

    let path = canonical_full;

    // Read file contents
    let file_data = tokio::fs::read(&path)
        .await
        .map_err(|e| format!("Failed to read file: {}", e))?;

    let filename = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("attachment")
        .to_string();

    // Detect MIME type from extension
    let mime_type = mime_guess::from_path(&path)
        .first()
        .unwrap_or(mime_guess::mime::APPLICATION_OCTET_STREAM);

    let is_image = mime_type.type_() == "image";

    // Upload to Matrix (client already extracted above)
    let upload_response = matrix_client
        .media()
        .upload(&mime_type, file_data, None) // None = default RequestConfig
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

/// Handle list_schedules tool call
fn handle_list_schedules(state: &McpState, args: &Value) -> Result<String, String> {
    let channel_name = args.get("channel_name").and_then(|v| v.as_str());
    let include_completed = args
        .get("include_completed")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    // Get channel name from context if not provided
    let channel_name = match channel_name {
        Some(name) => name.to_string(),
        None => {
            if let Some(workspace_dir) = find_workspace_dir() {
                if let Some(ctx) = read_context_file(&workspace_dir) {
                    ctx.channel_name
                } else {
                    return Err("channel_name is required (context file not readable)".to_string());
                }
            } else {
                return Err("channel_name is required (no context file found)".to_string());
            }
        }
    };

    let schedules = state
        .scheduler_store
        .list_by_channel(&channel_name)
        .map_err(|e| format!("Database error: {}", e))?;

    if schedules.is_empty() {
        return Ok(format!("No schedules found for channel '{}'", channel_name));
    }

    let mut output = format!("Schedules for channel '{}':\n\n", channel_name);
    for schedule in schedules {
        // Skip completed one-time schedules unless requested
        if !include_completed
            && schedule.status == ScheduleStatus::Completed
            && schedule.cron_expression.is_none()
        {
            continue;
        }

        let schedule_type = if schedule.cron_expression.is_some() {
            "recurring"
        } else {
            "one-time"
        };
        let status = format!("{:?}", schedule.status).to_lowercase();
        let prompt_preview: String = schedule.prompt.chars().take(50).collect();

        output.push_str(&format!(
            "• {} ({})\n  ID: {}\n  Prompt: {}...\n  Next: {}\n  Status: {}\n\n",
            schedule_type,
            schedule.cron_expression.as_deref().unwrap_or("once"),
            schedule.id,
            prompt_preview,
            schedule.next_execution_at,
            status
        ));
    }

    Ok(output)
}

/// Handle cancel_schedule tool call
fn handle_cancel_schedule(state: &McpState, args: &Value) -> Result<String, String> {
    let schedule_id = args
        .get("schedule_id")
        .and_then(|v| v.as_str())
        .ok_or("Missing required parameter: schedule_id")?;

    // Verify the schedule exists
    let schedule = state
        .scheduler_store
        .get_schedule(schedule_id)
        .map_err(|e| format!("Database error: {}", e))?
        .ok_or_else(|| format!("Schedule not found: {}", schedule_id))?;

    // Cancel it
    state
        .scheduler_store
        .cancel_schedule(schedule_id)
        .map_err(|e| format!("Failed to cancel schedule: {}", e))?;

    Ok(format!(
        "Cancelled schedule '{}' for channel '{}'",
        schedule_id, schedule.channel_name
    ))
}

/// Handle get_status tool call
fn handle_get_status(state: &McpState, args: &Value) -> Result<String, String> {
    let channel_name = args.get("channel_name").and_then(|v| v.as_str());

    // Get channel name from context if not provided
    let channel_name = match channel_name {
        Some(name) => name.to_string(),
        None => {
            if let Some(workspace_dir) = find_workspace_dir() {
                if let Some(ctx) = read_context_file(&workspace_dir) {
                    ctx.channel_name
                } else {
                    return Err("channel_name is required (context file not readable)".to_string());
                }
            } else {
                return Err("channel_name is required (no context file found)".to_string());
            }
        }
    };

    let channel = state
        .session_store
        .get_by_name(&channel_name)
        .map_err(|e| format!("Database error: {}", e))?
        .ok_or_else(|| format!("Channel not found: {}", channel_name))?;

    // Check debug mode
    let debug_path = std::path::Path::new(&channel.directory)
        .join(".gorp")
        .join("enable-debug");
    let debug_enabled = debug_path.exists();

    // Count schedules
    let schedules = state
        .scheduler_store
        .list_by_channel(&channel_name)
        .unwrap_or_default();
    let active_schedules = schedules
        .iter()
        .filter(|s| s.status == ScheduleStatus::Active)
        .count();

    Ok(format!(
        "Channel: {}\n\
         Room ID: {}\n\
         Session ID: {}\n\
         Session Started: {}\n\
         Debug Mode: {}\n\
         Workspace: {}\n\
         Active Schedules: {}",
        channel.channel_name,
        channel.room_id,
        channel.session_id,
        if channel.started { "Yes" } else { "No" },
        if debug_enabled { "Enabled" } else { "Disabled" },
        channel.directory,
        active_schedules
    ))
}

/// Handle list_channels tool call
fn handle_list_channels(state: &McpState) -> Result<String, String> {
    let channels = state
        .session_store
        .list_all()
        .map_err(|e| format!("Database error: {}", e))?;

    if channels.is_empty() {
        return Ok("No channels registered.".to_string());
    }

    let mut output = format!("Registered channels ({}):\n\n", channels.len());
    for channel in channels {
        let debug_path = std::path::Path::new(&channel.directory)
            .join(".gorp")
            .join("enable-debug");
        let debug_status = if debug_path.exists() { "🔧" } else { "" };

        output.push_str(&format!(
            "• {} {}\n  Room: {}\n  Started: {}\n\n",
            channel.channel_name,
            debug_status,
            channel.room_id,
            if channel.started { "Yes" } else { "No" }
        ));
    }

    Ok(output)
}

/// Handle set_debug tool call
fn handle_set_debug(state: &McpState, args: &Value) -> Result<String, String> {
    let enabled = args
        .get("enabled")
        .and_then(|v| v.as_bool())
        .ok_or("Missing required parameter: enabled")?;

    let channel_name = args.get("channel_name").and_then(|v| v.as_str());

    // Get channel name from context if not provided
    let channel_name = match channel_name {
        Some(name) => name.to_string(),
        None => {
            if let Some(workspace_dir) = find_workspace_dir() {
                if let Some(ctx) = read_context_file(&workspace_dir) {
                    ctx.channel_name
                } else {
                    return Err("channel_name is required (context file not readable)".to_string());
                }
            } else {
                return Err("channel_name is required (no context file found)".to_string());
            }
        }
    };

    let channel = state
        .session_store
        .get_by_name(&channel_name)
        .map_err(|e| format!("Database error: {}", e))?
        .ok_or_else(|| format!("Channel not found: {}", channel_name))?;

    let debug_dir = std::path::Path::new(&channel.directory).join(".matrix");
    let debug_file = debug_dir.join("enable-debug");

    if enabled {
        // Create .matrix directory if needed
        std::fs::create_dir_all(&debug_dir)
            .map_err(|e| format!("Failed to create debug directory: {}", e))?;
        // Create enable-debug file
        std::fs::write(&debug_file, "").map_err(|e| format!("Failed to enable debug: {}", e))?;
        Ok(format!(
            "Debug mode ENABLED for channel '{}'. Tool usage will be shown in Matrix.",
            channel_name
        ))
    } else {
        // Remove enable-debug file if it exists
        if debug_file.exists() {
            std::fs::remove_file(&debug_file)
                .map_err(|e| format!("Failed to disable debug: {}", e))?;
        }
        Ok(format!(
            "Debug mode DISABLED for channel '{}'. Tool usage will be hidden.",
            channel_name
        ))
    }
}

/// Handle leave_room tool call
async fn handle_leave_room(state: &McpState, args: &Value) -> Result<String, String> {
    use matrix_sdk::ruma::OwnedRoomId;

    let confirm = args
        .get("confirm")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if !confirm {
        return Err("Must set confirm=true to leave a room".to_string());
    }

    let channel_name = args.get("channel_name").and_then(|v| v.as_str());

    // Get channel name from context if not provided
    let channel_name = match channel_name {
        Some(name) => name.to_string(),
        None => {
            if let Some(workspace_dir) = find_workspace_dir() {
                if let Some(ctx) = read_context_file(&workspace_dir) {
                    ctx.channel_name
                } else {
                    return Err("channel_name is required (context file not readable)".to_string());
                }
            } else {
                return Err("channel_name is required (no context file found)".to_string());
            }
        }
    };

    let channel = state
        .session_store
        .get_by_name(&channel_name)
        .map_err(|e| format!("Database error: {}", e))?
        .ok_or_else(|| format!("Channel not found: {}", channel_name))?;

    let room_id: OwnedRoomId = channel
        .room_id
        .parse()
        .map_err(|e| format!("Invalid room ID: {}", e))?;

    // Leave the Matrix room (if Matrix is available)
    if let Some(ref matrix_client) = state.matrix_client {
        if let Some(room) = matrix_client.get_room(&room_id) {
            room.leave()
                .await
                .map_err(|e| format!("Failed to leave Matrix room: {}", e))?;
        }
    }

    Ok(format!(
        "Left room '{}'. Workspace at '{}' is preserved.",
        channel_name, channel.directory
    ))
}

/// Handle create_channel tool call
async fn handle_create_channel(state: &McpState, args: &Value) -> Result<String, String> {
    let channel_name = args
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or("Missing required parameter: name")?;

    let invite_user = args.get("invite_user").and_then(|v| v.as_str());

    // Create Matrix room if Matrix is available, otherwise use a placeholder room ID
    let room_id = if let Some(ref matrix_client) = state.matrix_client {
        let room_name = format!("{}: {}", state.room_prefix, channel_name);
        matrix_client::create_room(matrix_client, &room_name)
            .await
            .map_err(|e| format!("Failed to create Matrix room: {}", e))?
    } else {
        // Generate a placeholder room ID for non-Matrix operation
        format!("!local-{}", uuid::Uuid::new_v4()).parse()
            .map_err(|e| format!("Failed to create placeholder room ID: {}", e))?
    };

    // Create channel in session store (handles directory creation, templates, validation)
    let channel = state
        .session_store
        .create_channel(channel_name, room_id.as_ref())
        .map_err(|e| format!("Failed to create channel: {}", e))?;

    // Invite user if specified (only when Matrix is available)
    if let Some(user_id) = invite_user {
        if let Some(ref matrix_client) = state.matrix_client {
            matrix_client::invite_user(matrix_client, &room_id, user_id)
                .await
                .map_err(|e| format!("Room created but failed to invite user: {}", e))?;
        } else {
            tracing::warn!(user = %user_id, "Cannot invite user — Matrix not configured");
        }

        Ok(format!(
            "Created channel '{}'\nRoom ID: {}\nWorkspace: {}\nInvited: {}",
            channel.channel_name, channel.room_id, channel.directory, user_id
        ))
    } else {
        Ok(format!(
            "Created channel '{}'\nRoom ID: {}\nWorkspace: {}",
            channel.channel_name, channel.room_id, channel.directory,
        ))
    }
}

/// Handle invite_to_channel tool call
async fn handle_invite_to_channel(state: &McpState, args: &Value) -> Result<String, String> {
    use matrix_sdk::ruma::OwnedRoomId;

    let user_id = args
        .get("user_id")
        .and_then(|v| v.as_str())
        .ok_or("Missing required parameter: user_id")?;

    let channel_name = args.get("channel_name").and_then(|v| v.as_str());

    // Get channel name from context if not provided
    let channel_name = match channel_name {
        Some(name) => name.to_string(),
        None => {
            if let Some(workspace_dir) = find_workspace_dir() {
                if let Some(ctx) = read_context_file(&workspace_dir) {
                    ctx.channel_name
                } else {
                    return Err("channel_name is required (context file not readable)".to_string());
                }
            } else {
                return Err("channel_name is required (no context file found)".to_string());
            }
        }
    };

    let channel = state
        .session_store
        .get_by_name(&channel_name)
        .map_err(|e| format!("Database error: {}", e))?
        .ok_or_else(|| format!("Channel not found: {}", channel_name))?;

    let room_id: OwnedRoomId = channel
        .room_id
        .parse()
        .map_err(|e| format!("Invalid room ID: {}", e))?;

    // Invite the user (requires Matrix)
    let matrix_client = state.matrix_client.as_ref()
        .ok_or("Matrix not configured — cannot invite users")?;
    matrix_client::invite_user(matrix_client, &room_id, user_id)
        .await
        .map_err(|e| format!("Failed to invite user: {}", e))?;

    Ok(format!("Invited {} to channel '{}'", user_id, channel_name))
}

/// Handle set_room_avatar tool call
async fn handle_set_room_avatar(state: &McpState, args: &Value) -> Result<String, String> {
    use matrix_sdk::ruma::{events::room::avatar::RoomAvatarEventContent, OwnedRoomId};
    use std::path::Path;

    let image_path = args
        .get("image_path")
        .and_then(|v| v.as_str())
        .ok_or("Missing required parameter: image_path")?;

    let channel_name = args.get("channel_name").and_then(|v| v.as_str());

    // Get channel name from context if not provided
    let channel_name = match channel_name {
        Some(name) => name.to_string(),
        None => {
            if let Some(workspace_dir) = find_workspace_dir() {
                if let Some(ctx) = read_context_file(&workspace_dir) {
                    ctx.channel_name
                } else {
                    return Err("channel_name is required (context file not readable)".to_string());
                }
            } else {
                return Err("channel_name is required (no context file found)".to_string());
            }
        }
    };

    let channel = state
        .session_store
        .get_by_name(&channel_name)
        .map_err(|e| format!("Database error: {}", e))?
        .ok_or_else(|| format!("Channel not found: {}", channel_name))?;

    let room_id: OwnedRoomId = channel
        .room_id
        .parse()
        .map_err(|e| format!("Invalid room ID: {}", e))?;

    let matrix_client = state.matrix_client.as_ref()
        .ok_or("Matrix not configured — cannot send images to rooms")?;
    let room = matrix_client
        .get_room(&room_id)
        .ok_or_else(|| format!("Room not found: {}", channel.room_id))?;

    // Validate path and prevent directory traversal
    let workspace_root = Path::new(&state.workspace_path);
    let requested_path = Path::new(image_path);

    // Reject paths with ".." to prevent traversal
    if image_path.contains("..") {
        tracing::warn!(
            path = image_path,
            "Path traversal attempt blocked: contains '..'"
        );
        return Err("Invalid path: contains path traversal".to_string());
    }

    // Build the full path relative to workspace
    let full_path = workspace_root.join(requested_path);

    // Canonicalize both paths to resolve symlinks and validate
    let canonical_workspace = workspace_root
        .canonicalize()
        .map_err(|e| format!("Workspace path error: {}", e))?;

    let canonical_full = full_path
        .canonicalize()
        .map_err(|e| format!("Image file not found: {}", e))?;

    // Verify the resolved path is within workspace
    if !canonical_full.starts_with(&canonical_workspace) {
        tracing::warn!(
            requested_path = image_path,
            resolved_path = %canonical_full.display(),
            workspace_root = %canonical_workspace.display(),
            "Path traversal attempt blocked: resolved path outside workspace"
        );
        return Err("Access denied: path outside workspace".to_string());
    }

    let path = canonical_full;

    // Read image data
    let image_data = tokio::fs::read(&path)
        .await
        .map_err(|e| format!("Failed to read image file: {}", e))?;

    // Detect MIME type
    let mime_type = mime_guess::from_path(&path)
        .first()
        .unwrap_or(mime_guess::mime::IMAGE_PNG);

    if mime_type.type_() != "image" {
        return Err(format!("File is not an image: {}", image_path));
    }

    // Upload image to Matrix (client already extracted above)
    let upload_response = matrix_client
        .media()
        .upload(&mime_type, image_data, None)
        .await
        .map_err(|e| format!("Failed to upload image to Matrix: {}", e))?;

    // Set room avatar
    let mut content = RoomAvatarEventContent::new();
    content.url = Some(upload_response.content_uri);
    room.send_state_event(content)
        .await
        .map_err(|e| format!("Failed to set room avatar: {}", e))?;

    let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("image");

    Ok(format!(
        "Set avatar for room '{}' to '{}'",
        channel.channel_name, filename
    ))
}

/// Handle set_room_topic tool call
async fn handle_set_room_topic(state: &McpState, args: &Value) -> Result<String, String> {
    use matrix_sdk::ruma::{events::room::topic::RoomTopicEventContent, OwnedRoomId};

    let topic = args
        .get("topic")
        .and_then(|v| v.as_str())
        .ok_or("Missing required parameter: topic")?;

    let channel_name = args.get("channel_name").and_then(|v| v.as_str());

    // Get channel name from context if not provided
    let channel_name = match channel_name {
        Some(name) => name.to_string(),
        None => {
            if let Some(workspace_dir) = find_workspace_dir() {
                if let Some(ctx) = read_context_file(&workspace_dir) {
                    ctx.channel_name
                } else {
                    return Err("channel_name is required (context file not readable)".to_string());
                }
            } else {
                return Err("channel_name is required (no context file found)".to_string());
            }
        }
    };

    let channel = state
        .session_store
        .get_by_name(&channel_name)
        .map_err(|e| format!("Database error: {}", e))?
        .ok_or_else(|| format!("Channel not found: {}", channel_name))?;

    let room_id: OwnedRoomId = channel
        .room_id
        .parse()
        .map_err(|e| format!("Invalid room ID: {}", e))?;

    let matrix_client = state.matrix_client.as_ref()
        .ok_or("Matrix not configured — cannot set room topic")?;
    let room = matrix_client
        .get_room(&room_id)
        .ok_or_else(|| format!("Room not found: {}", channel.room_id))?;

    // Set room topic
    let content = RoomTopicEventContent::new(topic.to_string());
    room.send_state_event(content)
        .await
        .map_err(|e| format!("Failed to set room topic: {}", e))?;

    Ok(format!(
        "Set topic for room '{}' to: {}",
        channel.channel_name, topic
    ))
}

/// Handle report_to_management tool call
/// Sends a report to a dedicated management room for human review
async fn handle_report_to_management(state: &McpState, args: &Value) -> Result<String, String> {
    use matrix_sdk::ruma::{events::room::message::RoomMessageEventContent, OwnedRoomId, RoomId};

    // Hardcoded management room - this is where all agent reports go
    const MANAGEMENT_ROOM_ID: &str = "!llllhqZbfveDbueMJZ:matrix.org";

    let message = args
        .get("message")
        .and_then(|v| v.as_str())
        .ok_or("Missing required parameter: message")?;

    let category = args
        .get("category")
        .and_then(|v| v.as_str())
        .unwrap_or("other");

    let severity = args
        .get("severity")
        .and_then(|v| v.as_str())
        .unwrap_or("medium");

    // Get source context (which channel is reporting)
    let source_info = if let Some(workspace_dir) = find_workspace_dir() {
        if let Some(ctx) = read_context_file(&workspace_dir) {
            format!("Channel: {} ({})", ctx.channel_name, ctx.room_id)
        } else {
            "Channel: unknown (no context)".to_string()
        }
    } else {
        "Channel: unknown (no workspace)".to_string()
    };

    // Parse the management room ID
    let room_id: OwnedRoomId = MANAGEMENT_ROOM_ID
        .parse()
        .map_err(|e| format!("Invalid management room ID: {}", e))?;

    // Try to get the room - if we're not in it, try to join (requires Matrix)
    let matrix_client = state.matrix_client.as_ref()
        .ok_or("Matrix not configured — cannot send management alerts")?;
    let room = match matrix_client.get_room(&room_id) {
        Some(r) => r,
        None => {
            // Try to join the room
            tracing::info!("Attempting to join management room: {}", MANAGEMENT_ROOM_ID);
            let room_id_ref: &RoomId = room_id.as_ref();
            matrix_client
                .join_room_by_id(room_id_ref)
                .await
                .map_err(|e| {
                    format!(
                        "Failed to join management room: {}. The bot may need to be invited first.",
                        e
                    )
                })?;

            // Get the room after joining
            matrix_client
                .get_room(&room_id)
                .ok_or_else(|| "Failed to access management room after joining".to_string())?
        }
    };

    // Get severity emoji
    let severity_emoji = match severity {
        "critical" => "🚨",
        "high" => "🔴",
        "medium" => "🟡",
        "low" => "🟢",
        _ => "⚪",
    };

    // Get category emoji
    let category_emoji = match category {
        "bug" => "🐛",
        "safety" => "🛡️",
        "concern" => "⚠️",
        "behavior" => "🤖",
        "error" => "❌",
        "feedback" => "💬",
        _ => "📋",
    };

    // Get current timestamp
    let timestamp = chrono::Utc::now()
        .format("%Y-%m-%d %H:%M:%S UTC")
        .to_string();

    // Format the report message
    let plain_text = format!(
        "{} AGENT REPORT {}\n\nCategory: {} {}\nSeverity: {} {}\nSource: {}\nTime: {}\n\n{}\n\n---",
        severity_emoji,
        severity_emoji,
        category_emoji,
        category.to_uppercase(),
        severity_emoji,
        severity.to_uppercase(),
        source_info,
        timestamp,
        message
    );

    let html = format!(
        r#"<h3>{} AGENT REPORT {}</h3>
<p><strong>Category:</strong> {} {}</p>
<p><strong>Severity:</strong> {} {}</p>
<p><strong>Source:</strong> {}</p>
<p><strong>Time:</strong> {}</p>
<hr>
<p>{}</p>
<hr>"#,
        severity_emoji,
        severity_emoji,
        category_emoji,
        category.to_uppercase(),
        severity_emoji,
        severity.to_uppercase(),
        source_info,
        timestamp,
        message.replace('\n', "<br>")
    );

    // Send to management room
    room.send(RoomMessageEventContent::text_html(&plain_text, &html))
        .await
        .map_err(|e| format!("Failed to send report to management: {}", e))?;

    tracing::info!(
        category = %category,
        severity = %severity,
        "Report sent to management room"
    );

    Ok(format!(
        "Report submitted to management.\nCategory: {}\nSeverity: {}\n\nHumans will review your report.",
        category, severity
    ))
}
