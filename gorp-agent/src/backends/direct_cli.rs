// ABOUTME: Direct CLI backend - spawns claude with --print --output-format stream-json.
// ABOUTME: Parses streaming JSONL from stdout, emits AgentEvents.

use crate::event::{AgentEvent, ErrorCode, Usage};
use crate::handle::{AgentHandle, Command};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command as ProcessCommand;
use tokio::sync::mpsc;

/// Configuration for the Direct CLI backend
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectCliConfig {
    /// Path to the claude binary
    pub binary: String,
    /// Optional SDK URL for the claude CLI
    pub sdk_url: Option<String>,
    /// Working directory for the agent
    pub working_dir: PathBuf,
}

pub struct DirectCliBackend {
    config: DirectCliConfig,
}

impl DirectCliBackend {
    pub fn new(config: DirectCliConfig) -> Result<Self> {
        Ok(Self { config })
    }

    pub fn into_handle(self) -> AgentHandle {
        let (tx, mut rx) = mpsc::channel::<Command>(32);
        let name = "direct";
        let config = self.config;

        tokio::spawn(async move {
            while let Some(cmd) = rx.recv().await {
                match cmd {
                    Command::NewSession { reply } => {
                        // Direct CLI doesn't have persistent sessions by default
                        // Generate a UUID for tracking
                        let session_id = uuid::Uuid::new_v4().to_string();
                        let _ = reply.send(Ok(session_id));
                    }
                    Command::LoadSession { reply, .. } => {
                        // Direct CLI can use --resume with session ID
                        let _ = reply.send(Ok(()));
                    }
                    Command::Prompt {
                        session_id,
                        text,
                        event_tx,
                        reply,
                        is_new_session,
                    } => {
                        let _ = reply.send(Ok(()));
                        if let Err(e) = run_prompt(&config, &session_id, &text, event_tx, is_new_session).await {
                            tracing::error!(error = %e, "Direct CLI prompt failed");
                        }
                    }
                    Command::Cancel { reply, .. } => {
                        // TODO: Kill the running process
                        let _ = reply.send(Ok(()));
                    }
                }
            }
        });

        AgentHandle::new(tx, name)
    }

    /// Factory function for the registry
    pub fn factory() -> crate::registry::BackendFactory {
        Box::new(|config| {
            let cfg: DirectCliConfig = serde_json::from_value(config.clone())?;
            let backend = DirectCliBackend::new(cfg)?;
            Ok(backend.into_handle())
        })
    }
}

async fn run_prompt(
    config: &DirectCliConfig,
    session_id: &str,
    text: &str,
    event_tx: mpsc::Sender<AgentEvent>,
    is_new_session: bool,
) -> Result<()> {
    let mut args = vec![
        "--print".to_string(),
        "--output-format".to_string(),
        "stream-json".to_string(),
        "--verbose".to_string(),
        "--dangerously-skip-permissions".to_string(),
    ];

    // Only use --resume for existing sessions, not new ones
    if !is_new_session {
        args.push("--resume".to_string());
        args.push(session_id.to_string());
    }

    if let Some(ref url) = config.sdk_url {
        args.push("--sdk-url".to_string());
        args.push(url.clone());
    }

    args.push(text.to_string());

    tracing::debug!(?args, "Spawning Claude CLI");

    let mut child = ProcessCommand::new(&config.binary)
        .args(&args)
        .current_dir(&config.working_dir)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .context("Failed to spawn Claude CLI")?;

    let stdout = child.stdout.take().context("Failed to capture stdout")?;
    let stderr = child.stderr.take().context("Failed to capture stderr")?;

    let stderr_tx = event_tx.clone();

    // Spawn task to read stderr and detect errors - we'll join this later
    let stderr_handle = tokio::spawn(async move {
        let reader = BufReader::new(stderr);
        let mut lines = reader.lines();

        while let Ok(Some(line)) = lines.next_line().await {
            if !line.is_empty() {
                tracing::warn!(stderr = %line, "Claude CLI stderr");

                // Check for orphaned session error
                if line.contains("No conversation found with session ID") {
                    tracing::warn!("Detected orphaned session - sending SessionInvalid event");
                    let _ = stderr_tx
                        .send(AgentEvent::SessionInvalid {
                            reason: "Session not found".to_string(),
                        })
                        .await;
                }
            }
        }
    });

    let reader = BufReader::new(stdout);
    let mut lines = reader.lines();
    let mut accumulated_text = String::new();

    while let Ok(Some(line)) = lines.next_line().await {
        if line.is_empty() {
            continue;
        }

        if let Ok(json) = serde_json::from_str::<Value>(&line) {
            if let Some(events) = parse_cli_event(&json, &mut accumulated_text) {
                for event in events {
                    if event_tx.send(event).await.is_err() {
                        tracing::debug!("Event receiver closed, stopping stream");
                        break;
                    }
                }
            }
        }
    }

    let status = child.wait().await?;
    if !status.success() {
        let _ = event_tx
            .send(AgentEvent::Error {
                code: ErrorCode::BackendError,
                message: format!("CLI exited with status: {:?}", status.code()),
                recoverable: false,
            })
            .await;
    }

    // Wait for stderr reader to complete - ensures we don't leak the task
    if let Err(e) = stderr_handle.await {
        tracing::warn!(error = %e, "stderr reader task failed to complete");
    }

    Ok(())
}

fn parse_cli_event(json: &Value, accumulated_text: &mut String) -> Option<Vec<AgentEvent>> {
    let event_type = json.get("type")?.as_str()?;

    match event_type {
        "system" => {
            // Capture session_id from init event
            let subtype = json.get("subtype").and_then(|s| s.as_str());
            if subtype == Some("init") {
                if let Some(session_id) = json.get("session_id").and_then(|s| s.as_str()) {
                    return Some(vec![AgentEvent::SessionChanged {
                        new_session_id: session_id.to_string(),
                    }]);
                }
            }
            None
        }
        "assistant" => {
            let mut events = Vec::new();

            if let Some(content) = json
                .get("message")
                .and_then(|m| m.get("content"))
                .and_then(|c| c.as_array())
            {
                for item in content {
                    let item_type = item.get("type").and_then(|t| t.as_str());

                    if item_type == Some("tool_use") {
                        let name = item
                            .get("name")
                            .and_then(|n| n.as_str())
                            .unwrap_or("unknown")
                            .to_string();
                        let id = item
                            .get("id")
                            .and_then(|i| i.as_str())
                            .unwrap_or("unknown")
                            .to_string();
                        let input = item.get("input").cloned().unwrap_or(Value::Null);

                        tracing::info!(tool = %name, id = %id, "Tool use detected");
                        events.push(AgentEvent::ToolStart { id, name, input });
                    } else if item_type == Some("text") {
                        if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
                            if !text.is_empty() {
                                // Only add a space between chunks if:
                                // 1. There's accumulated text
                                // 2. The accumulated text doesn't end with whitespace
                                // 3. The new text doesn't start with whitespace or punctuation
                                if !accumulated_text.is_empty() {
                                    let ends_with_ws = accumulated_text.ends_with(|c: char| c.is_whitespace());
                                    let starts_with_ws_or_punct = text.starts_with(|c: char| {
                                        c.is_whitespace() || c.is_ascii_punctuation()
                                    });
                                    if !ends_with_ws && !starts_with_ws_or_punct {
                                        accumulated_text.push(' ');
                                    }
                                }
                                accumulated_text.push_str(text);

                                // Emit text event for streaming display
                                events.push(AgentEvent::Text(text.to_string()));
                            }
                        }
                    }
                }
            }

            if events.is_empty() {
                None
            } else {
                Some(events)
            }
        }
        "result" => {
            let is_error = json
                .get("is_error")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            if is_error {
                let message = json
                    .get("error")
                    .and_then(|e| e.as_str())
                    .unwrap_or("Unknown error")
                    .to_string();

                let code = if message.contains("timeout") {
                    ErrorCode::Timeout
                } else if message.contains("rate limit") {
                    ErrorCode::RateLimited
                } else if message.contains("permission") {
                    ErrorCode::PermissionDenied
                } else {
                    ErrorCode::BackendError
                };

                Some(vec![AgentEvent::Error {
                    code,
                    message,
                    recoverable: false,
                }])
            } else {
                // Use accumulated text from assistant messages
                let result_text = if !accumulated_text.is_empty() {
                    std::mem::take(accumulated_text)
                } else {
                    // Fallback to result field if present
                    json.get("result")
                        .and_then(|r| r.as_str())
                        .unwrap_or("")
                        .to_string()
                };

                let usage = extract_usage(json);

                tracing::debug!(
                    result_len = result_text.len(),
                    input_tokens = usage.as_ref().map(|u| u.input_tokens).unwrap_or(0),
                    output_tokens = usage.as_ref().map(|u| u.output_tokens).unwrap_or(0),
                    "Sending result with usage"
                );

                Some(vec![AgentEvent::Result {
                    text: result_text,
                    usage,
                    metadata: json.clone(),
                }])
            }
        }
        _ => None,
    }
}

fn extract_usage(json: &Value) -> Option<Usage> {
    let mut usage = Usage::default();
    let mut found_usage = false;

    // Get total cost
    if let Some(cost) = json.get("total_cost_usd").and_then(|v| v.as_f64()) {
        usage.cost_usd = Some(cost);
        found_usage = true;
    }

    // Get usage object for token counts
    if let Some(usage_obj) = json.get("usage") {
        usage.input_tokens = usage_obj
            .get("input_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        usage.output_tokens = usage_obj
            .get("output_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        usage.cache_read_tokens = usage_obj.get("cache_read_input_tokens").and_then(|v| v.as_u64());
        usage.cache_write_tokens = usage_obj
            .get("cache_creation_input_tokens")
            .and_then(|v| v.as_u64());
        found_usage = true;
    }

    // Also check modelUsage for aggregated token counts if usage is empty
    if usage.input_tokens == 0 && usage.output_tokens == 0 {
        if let Some(model_usage) = json.get("modelUsage").and_then(|v| v.as_object()) {
            for (_model, stats) in model_usage {
                usage.input_tokens += stats
                    .get("inputTokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                usage.output_tokens += stats
                    .get("outputTokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                usage.cache_read_tokens = usage
                    .cache_read_tokens
                    .or_else(|| stats.get("cacheReadInputTokens").and_then(|v| v.as_u64()));
                usage.cache_write_tokens = usage
                    .cache_write_tokens
                    .or_else(|| stats.get("cacheCreationInputTokens").and_then(|v| v.as_u64()));
                found_usage = true;
            }
        }
    }

    if found_usage {
        Some(usage)
    } else {
        None
    }
}
