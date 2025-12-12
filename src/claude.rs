// ABOUTME: This module handles spawning the Claude CLI process and parsing its JSON responses.
// ABOUTME: It provides streaming invoke with tool progress callbacks and batch invoke for simpler cases.

use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;

/// Events emitted during Claude streaming execution
#[derive(Debug, Clone)]
pub enum ClaudeEvent {
    /// Claude is calling a tool
    ToolUse { name: String, input_preview: String },
    /// Final result text
    Result(String),
    /// Error occurred
    Error(String),
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct PermissionDenial {
    tool_name: Option<String>,
    tool_use_id: Option<String>,
    #[serde(default)]
    reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ClaudeResponse {
    #[serde(default)]
    result: Option<String>,
    #[serde(default)]
    subtype: Option<String>,
    #[serde(default)]
    is_error: bool,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    permission_denials: Vec<PermissionDenial>,
}

pub fn parse_response(json: &str) -> Result<String> {
    let response: ClaudeResponse =
        serde_json::from_str(json).context("Failed to parse Claude JSON response")?;

    // If there's a result field, return it
    if let Some(result) = response.result {
        return Ok(result);
    }

    // Handle error_during_execution - this happens when Claude hits permission denials or other runtime errors
    if response.subtype.as_deref() == Some("error_during_execution") {
        let mut error_msg = String::from("Claude encountered an error during execution");

        // Check for permission denials (MCP tools that were blocked)
        if !response.permission_denials.is_empty() {
            error_msg.push_str(":\n\nPermission Denials:");
            for denial in &response.permission_denials {
                if let Some(tool) = &denial.tool_name {
                    error_msg.push_str(&format!("\n- Tool: {}", tool));
                    if let Some(reason) = &denial.reason {
                        error_msg.push_str(&format!(" ({})", reason));
                    }
                }
            }
            error_msg.push_str("\n\nPlease approve the tool permissions and try again.");
        }

        anyhow::bail!("{}", error_msg);
    }

    // If no result but there's an error indicator, provide helpful message
    if response.is_error {
        let error_details = response
            .error
            .or(response.message)
            .unwrap_or_else(|| "No error details provided".to_string());
        anyhow::bail!(
            "Claude reported an error (subtype: {}): {}",
            response.subtype.unwrap_or_else(|| "unknown".to_string()),
            error_details
        );
    }

    // If no result field at all, show what we got (log full JSON for debugging)
    tracing::error!(
        full_json = %json,
        "Claude response missing 'result' field - full JSON logged"
    );
    let error_details = response
        .error
        .or(response.message)
        .unwrap_or_else(|| "No additional details".to_string());
    anyhow::bail!(
        "Claude response missing 'result' field (subtype: {}): {}",
        response.subtype.unwrap_or_else(|| "unknown".to_string()),
        error_details
    );
}

pub async fn invoke_claude(
    binary_path: &str,
    sdk_url: Option<&str>,
    session_args: Vec<&str>,
    prompt: &str,
    working_dir: Option<&str>,
) -> Result<String> {
    // Validate binary path doesn't contain suspicious characters
    if binary_path.contains("..") || binary_path.contains('\0') {
        anyhow::bail!("Invalid claude binary path");
    }

    let mut args = vec![
        "--print",
        "--output-format",
        "json",
        "--dangerously-skip-permissions",
    ];
    args.extend(session_args);

    if let Some(url) = sdk_url {
        args.extend(["--sdk-url", url]);
    }

    args.push(prompt);

    tracing::debug!(?args, working_dir, "Spawning Claude CLI");

    let mut command = Command::new(binary_path);
    command.args(&args);

    // Set working directory if specified
    if let Some(dir) = working_dir {
        // Validate directory exists
        if !std::path::Path::new(dir).exists() {
            anyhow::bail!("Working directory does not exist: {}", dir);
        }
        command.current_dir(dir);
        tracing::info!(working_dir = dir, "Running Claude in specified directory");
    }

    let output = command.output().await.with_context(|| {
        if let Some(dir) = working_dir {
            format!("Failed to spawn claude CLI in directory: {}", dir)
        } else {
            "Failed to spawn claude CLI".to_string()
        }
    })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!(
            "Claude CLI failed with exit code {:?}: {}",
            output.status.code(),
            stderr
        );
    }

    let stdout = String::from_utf8(output.stdout).context("Claude output is not valid UTF-8")?;

    tracing::debug!(stdout_preview = %stdout.chars().take(500).collect::<String>(), "Claude raw output");

    // parse_response now handles its own error logging with full JSON
    parse_response(&stdout)
}

/// Invoke Claude with streaming output, emitting events for tool usage
/// Returns a channel receiver for events - the final event will be Result or Error
pub async fn invoke_claude_streaming(
    binary_path: &str,
    sdk_url: Option<&str>,
    session_args: Vec<&str>,
    prompt: &str,
    working_dir: Option<&str>,
) -> Result<mpsc::Receiver<ClaudeEvent>> {
    // Validate binary path
    if binary_path.contains("..") || binary_path.contains('\0') {
        anyhow::bail!("Invalid claude binary path");
    }

    // Use stream-json for real-time events
    let mut args = vec![
        "--print",
        "--output-format",
        "stream-json",
        "--verbose",
        "--dangerously-skip-permissions",
    ];
    args.extend(session_args);

    if let Some(url) = sdk_url {
        args.extend(["--sdk-url", url]);
    }

    args.push(prompt);

    tracing::debug!(?args, working_dir, "Spawning Claude CLI (streaming)");

    let mut command = Command::new(binary_path);
    command.args(&args);
    command.stdout(std::process::Stdio::piped());
    command.stderr(std::process::Stdio::piped());

    if let Some(dir) = working_dir {
        if !std::path::Path::new(dir).exists() {
            anyhow::bail!("Working directory does not exist: {}", dir);
        }
        command.current_dir(dir);
    }

    tracing::info!("Spawning Claude CLI process...");
    let mut child = command.spawn().context("Failed to spawn Claude CLI")?;
    let stdout = child.stdout.take().context("Failed to capture stdout")?;
    let stderr = child.stderr.take().context("Failed to capture stderr")?;
    tracing::info!("Claude CLI spawned successfully");

    let (tx, rx) = mpsc::channel(32);

    // Clone working_dir for the async task
    let log_dir = working_dir.map(|d| d.to_string());
    let stderr_log_dir = log_dir.clone();

    // Spawn task to read stderr and log it
    tokio::spawn(async move {
        let reader = BufReader::new(stderr);
        let mut lines = reader.lines();

        // Open stderr log file if working directory is set
        let mut log_file = if let Some(ref dir) = stderr_log_dir {
            let gorp_dir = format!("{}/.gorp", dir);
            // Create .gorp directory if it doesn't exist
            let _ = tokio::fs::create_dir_all(&gorp_dir).await;
            let path = format!("{}/claude-messages.jsonl", gorp_dir);
            tokio::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .await
                .ok()
        } else {
            None
        };

        while let Ok(Some(line)) = lines.next_line().await {
            if !line.is_empty() {
                tracing::warn!(stderr = %line, "Claude CLI stderr");
                // Log to file with STDERR prefix
                if let Some(ref mut file) = log_file {
                    use tokio::io::AsyncWriteExt;
                    let log_line = format!(
                        "{{\"type\":\"stderr\",\"message\":\"{}\"}}\n",
                        line.replace('\"', "\\\"")
                    );
                    let _ = file.write_all(log_line.as_bytes()).await;
                }
            }
        }
    });

    // Spawn task to read and parse streaming output
    tokio::spawn(async move {
        tracing::debug!("Starting to read Claude output stream");
        let reader = BufReader::new(stdout);
        let mut lines = reader.lines();
        let mut accumulated_text = String::new();

        // Open log file if working directory is set
        let mut log_file = if let Some(ref dir) = log_dir {
            let gorp_dir = format!("{}/.gorp", dir);
            // Create .gorp directory if it doesn't exist
            if let Err(e) = tokio::fs::create_dir_all(&gorp_dir).await {
                tracing::warn!(error = %e, path = %gorp_dir, "Failed to create .gorp directory");
            }
            let path = format!("{}/claude-messages.jsonl", gorp_dir);
            match tokio::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .await
            {
                Ok(f) => Some(f),
                Err(e) => {
                    tracing::warn!(error = %e, path = %path, "Failed to open JSONL log file");
                    None
                }
            }
        } else {
            None
        };

        while let Ok(Some(line)) = lines.next_line().await {
            if line.is_empty() {
                continue;
            }

            // Log raw JSONL to file
            if let Some(ref mut file) = log_file {
                use tokio::io::AsyncWriteExt;
                let timestamped = format!("{}\n", line);
                if let Err(e) = file.write_all(timestamped.as_bytes()).await {
                    tracing::warn!(error = %e, "Failed to write to JSONL log");
                }
            }

            // Parse JSON line
            let Ok(json): Result<Value, _> = serde_json::from_str(&line) else {
                continue;
            };

            let event_type = json.get("type").and_then(|v| v.as_str());

            match event_type {
                Some("assistant") => {
                    // Check for tool_use and text in content
                    if let Some(message) = json.get("message") {
                        if let Some(content) = message.get("content").and_then(|c| c.as_array()) {
                            for item in content {
                                let item_type = item.get("type").and_then(|t| t.as_str());

                                if item_type == Some("tool_use") {
                                    let name = item
                                        .get("name")
                                        .and_then(|n| n.as_str())
                                        .unwrap_or("unknown")
                                        .to_string();

                                    // Get brief input preview
                                    let input_preview = if let Some(input) = item.get("input") {
                                        get_input_preview(input, &name)
                                    } else {
                                        String::new()
                                    };

                                    tracing::info!(tool = %name, preview = %input_preview, "Tool use detected");
                                    let _ = tx
                                        .send(ClaudeEvent::ToolUse {
                                            name,
                                            input_preview,
                                        })
                                        .await;
                                } else if item_type == Some("text") {
                                    // Accumulate text content for the response
                                    if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
                                        if !text.is_empty() {
                                            // Add newline separator between text chunks to avoid
                                            // concatenation without whitespace (e.g., "event.Done")
                                            if !accumulated_text.is_empty()
                                                && !accumulated_text.ends_with('\n')
                                                && !accumulated_text.ends_with(' ')
                                            {
                                                accumulated_text.push('\n');
                                            }
                                            accumulated_text.push_str(text);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                Some("result") => {
                    let is_error = json
                        .get("is_error")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    if is_error {
                        let error = json
                            .get("error")
                            .and_then(|e| e.as_str())
                            .unwrap_or("Unknown error")
                            .to_string();
                        let _ = tx.send(ClaudeEvent::Error(error)).await;
                    } else {
                        // Use accumulated text from assistant messages
                        let result = if !accumulated_text.is_empty() {
                            std::mem::take(&mut accumulated_text)
                        } else {
                            // Fallback to result field if present
                            json.get("result")
                                .and_then(|r| r.as_str())
                                .unwrap_or("")
                                .to_string()
                        };

                        tracing::debug!(result_len = result.len(), "Sending result");
                        let _ = tx.send(ClaudeEvent::Result(result)).await;
                    }
                }
                _ => {}
            }
        }

        tracing::debug!("Claude output stream ended");

        // Wait for process to complete
        match child.wait().await {
            Ok(status) => tracing::info!(exit_code = ?status.code(), "Claude CLI exited"),
            Err(e) => tracing::error!(error = %e, "Failed to wait for Claude CLI"),
        }
    });

    Ok(rx)
}

/// Get a brief preview of tool input for display
fn get_input_preview(input: &Value, tool_name: &str) -> String {
    // Helper to truncate strings
    let truncate = |s: &str, max: usize| -> String {
        if s.len() <= max {
            s.to_string()
        } else {
            format!("{}…", s.chars().take(max - 1).collect::<String>())
        }
    };

    // Helper to get last N path components
    let short_path = |p: &str| -> String {
        let parts: Vec<&str> = p.split('/').collect();
        if parts.len() <= 2 {
            p.to_string()
        } else {
            parts[parts.len() - 2..].join("/")
        }
    };

    match tool_name {
        "Read" => input
            .get("file_path")
            .and_then(|p| p.as_str())
            .map(|p| short_path(p))
            .unwrap_or_default(),
        "Edit" => {
            let file = input
                .get("file_path")
                .and_then(|p| p.as_str())
                .map(|p| short_path(p))
                .unwrap_or_default();
            let old_str = input
                .get("old_string")
                .and_then(|s| s.as_str())
                .map(|s| truncate(s.lines().next().unwrap_or(""), 30))
                .unwrap_or_default();
            if old_str.is_empty() {
                file
            } else {
                format!("{} → {}", file, old_str)
            }
        }
        "Write" => input
            .get("file_path")
            .and_then(|p| p.as_str())
            .map(|p| short_path(p))
            .unwrap_or_default(),
        "Bash" => input
            .get("command")
            .and_then(|c| c.as_str())
            .map(|c| truncate(c.lines().next().unwrap_or(""), 60))
            .unwrap_or_default(),
        "Grep" => {
            let pattern = input.get("pattern").and_then(|p| p.as_str()).unwrap_or("");
            let path = input
                .get("path")
                .and_then(|p| p.as_str())
                .map(|p| short_path(p));
            match path {
                Some(p) => format!("/{}/  in {}", truncate(pattern, 25), p),
                None => format!("/{}/", truncate(pattern, 40)),
            }
        }
        "Glob" => input
            .get("pattern")
            .and_then(|p| p.as_str())
            .map(|p| truncate(p, 50))
            .unwrap_or_default(),
        "Task" => {
            let desc = input
                .get("description")
                .and_then(|d| d.as_str())
                .map(|d| truncate(d, 50))
                .unwrap_or_default();
            let agent = input
                .get("subagent_type")
                .and_then(|a| a.as_str())
                .unwrap_or("");
            if agent.is_empty() {
                desc
            } else {
                format!("[{}] {}", agent, desc)
            }
        }
        "WebFetch" | "WebSearch" => input
            .get("url")
            .or_else(|| input.get("query"))
            .and_then(|u| u.as_str())
            .map(|u| truncate(u, 60))
            .unwrap_or_default(),
        "TodoWrite" => input
            .get("todos")
            .and_then(|t| t.as_array())
            .map(|arr| format!("{} items", arr.len()))
            .unwrap_or_default(),
        // GSuite Gmail tools - show meaningful context
        _ if tool_name.starts_with("mcp__gsuite__gmail") => match tool_name {
            "mcp__gsuite__gmail_create_draft" | "mcp__gsuite__gmail_send_message" => {
                let to = input.get("to").and_then(|v| v.as_str()).unwrap_or("");
                let subject = input.get("subject").and_then(|v| v.as_str()).unwrap_or("");
                if !to.is_empty() && !subject.is_empty() {
                    format!("to:{} · {}", truncate(to, 25), truncate(subject, 35))
                } else if !to.is_empty() {
                    format!("to:{}", truncate(to, 50))
                } else if !subject.is_empty() {
                    truncate(subject, 50)
                } else {
                    String::new()
                }
            }
            "mcp__gsuite__gmail_list_messages" => input
                .get("query")
                .and_then(|v| v.as_str())
                .map(|s| truncate(s, 60))
                .unwrap_or_default(),
            "mcp__gsuite__gmail_get_message"
            | "mcp__gsuite__gmail_trash_message"
            | "mcp__gsuite__gmail_delete_message" => input
                .get("message_id")
                .and_then(|v| v.as_str())
                .map(|s| truncate(s, 20))
                .unwrap_or_default(),
            "mcp__gsuite__gmail_send_draft" => input
                .get("draft_id")
                .and_then(|v| v.as_str())
                .map(|s| truncate(s, 20))
                .unwrap_or_default(),
            _ => input
                .get("query")
                .or_else(|| input.get("message_id"))
                .and_then(|v| v.as_str())
                .map(|s| truncate(s, 50))
                .unwrap_or_default(),
        },
        // GSuite Calendar tools
        _ if tool_name.starts_with("mcp__gsuite__calendar") => {
            let summary = input.get("summary").and_then(|v| v.as_str());
            let start = input.get("start_time").and_then(|v| v.as_str());
            let event_id = input.get("event_id").and_then(|v| v.as_str());

            if let Some(s) = summary {
                if let Some(t) = start {
                    // Extract just date/time portion
                    let time_short = t
                        .split('T')
                        .nth(1)
                        .and_then(|t| t.split(':').take(2).collect::<Vec<_>>().join(":").into())
                        .unwrap_or_else(|| t.chars().take(10).collect());
                    format!("{} @ {}", truncate(s, 35), time_short)
                } else {
                    truncate(s, 50)
                }
            } else if let Some(id) = event_id {
                truncate(id, 30)
            } else {
                String::new()
            }
        }
        // GSuite People/Contacts tools
        _ if tool_name.starts_with("mcp__gsuite__people") => input
            .get("query")
            .or_else(|| input.get("name"))
            .or_else(|| input.get("email"))
            .and_then(|v| v.as_str())
            .map(|s| truncate(s, 50))
            .unwrap_or_default(),
        // Pagen CRM tools
        _ if tool_name.starts_with("mcp__pagen") => input
            .get("query")
            .or_else(|| input.get("name"))
            .or_else(|| input.get("contact_id"))
            .or_else(|| input.get("company_id"))
            .and_then(|v| v.as_str())
            .map(|s| truncate(s, 50))
            .unwrap_or_default(),
        // Chronicle tools
        _ if tool_name.starts_with("mcp__chronicle") => input
            .get("message")
            .or_else(|| input.get("activity"))
            .or_else(|| input.get("what"))
            .or_else(|| input.get("text"))
            .and_then(|v| v.as_str())
            .map(|s| truncate(s, 50))
            .unwrap_or_default(),
        // Toki task tools
        _ if tool_name.starts_with("mcp__toki") => input
            .get("description")
            .or_else(|| input.get("name"))
            .or_else(|| input.get("todo_id"))
            .and_then(|v| v.as_str())
            .map(|s| truncate(s, 50))
            .unwrap_or_default(),
        // Social media tools
        _ if tool_name.starts_with("mcp__socialmedia") => input
            .get("content")
            .or_else(|| input.get("agent_name"))
            .and_then(|v| v.as_str())
            .map(|s| truncate(s, 50))
            .unwrap_or_default(),
        // Other MCP tools - generic fallback
        _ if tool_name.starts_with("mcp__") => {
            // Try common field names in order of specificity
            input
                .get("content")
                .or_else(|| input.get("message"))
                .or_else(|| input.get("query"))
                .or_else(|| input.get("text"))
                .or_else(|| input.get("prompt"))
                .or_else(|| input.get("name"))
                .or_else(|| input.get("description"))
                .or_else(|| input.get("url"))
                .or_else(|| input.get("path"))
                .and_then(|v| v.as_str())
                .map(|s| truncate(s, 50))
                .unwrap_or_default()
        }
        _ => String::new(),
    }
}
