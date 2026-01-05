// ABOUTME: Direct Codex CLI backend - spawns codex exec with --json.
// ABOUTME: Parses streaming JSONL from stdout, emits AgentEvents. Supports session resume.

use crate::event::{AgentEvent, ErrorCode};
use crate::handle::{AgentHandle, Command};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command as ProcessCommand;
use tokio::sync::mpsc;

/// Configuration for the Direct Codex CLI backend
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectCodexConfig {
    /// Path to the CLI binary (e.g., "claude" for Claude Code CLI)
    /// Required - no default, must be explicitly set
    pub binary: String,
    /// Working directory for the agent
    pub working_dir: PathBuf,
    /// Sandbox mode: read-only, workspace-write, or danger-full-access
    #[serde(default = "default_sandbox")]
    pub sandbox_mode: String,
}

fn default_sandbox() -> String {
    "danger-full-access".to_string()
}

pub struct DirectCodexBackend {
    config: DirectCodexConfig,
}

impl DirectCodexBackend {
    pub fn new(config: DirectCodexConfig) -> Result<Self> {
        if config.binary.is_empty() {
            anyhow::bail!("direct backend requires 'binary' to be set (e.g., binary = \"claude\")");
        }
        Ok(Self { config })
    }

    pub fn into_handle(self) -> AgentHandle {
        let (tx, mut rx) = mpsc::channel::<Command>(32);
        let name = "direct-codex";
        let config = self.config;

        tokio::spawn(async move {
            while let Some(cmd) = rx.recv().await {
                match cmd {
                    Command::NewSession { reply } => {
                        // Codex assigns session IDs automatically
                        // We'll capture it from the output
                        let session_id = uuid::Uuid::new_v4().to_string();
                        let _ = reply.send(Ok(session_id));
                    }
                    Command::LoadSession { reply, .. } => {
                        // Codex supports resume via `codex exec resume <id>`
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
                        // Run prompt sequentially per-channel to maintain session state integrity
                        // Cross-channel concurrency is handled by each channel having its own AgentHandle
                        if let Err(e) =
                            run_prompt(&config, &session_id, &text, event_tx, is_new_session).await
                        {
                            tracing::error!(error = %e, "Direct Codex prompt failed");
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
            let cfg: DirectCodexConfig = serde_json::from_value(config.clone())?;
            let backend = DirectCodexBackend::new(cfg)?;
            Ok(backend.into_handle())
        })
    }
}

async fn run_prompt(
    config: &DirectCodexConfig,
    session_id: &str,
    text: &str,
    event_tx: mpsc::Sender<AgentEvent>,
    is_new_session: bool,
) -> Result<()> {
    let mut cmd = ProcessCommand::new(&config.binary);

    if is_new_session {
        // New session: codex exec --json -s <sandbox> "prompt"
        cmd.args([
            "exec",
            "--json",
            "-s",
            &config.sandbox_mode,
            "-C",
            config.working_dir.to_str().unwrap_or("."),
            text,
        ]);
    } else {
        // Resume session: codex exec --json -s <sandbox> resume <session_id> "prompt"
        // Note: --json and -s must come before the 'resume' subcommand
        cmd.args([
            "exec",
            "--json",
            "-s",
            &config.sandbox_mode,
            "resume",
            session_id,
            text,
        ]);
    }

    tracing::debug!(cmd = ?cmd, "Spawning Codex CLI");

    let mut child = cmd
        .current_dir(&config.working_dir)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .context("Failed to spawn Codex CLI")?;

    let stdout = child.stdout.take().context("Failed to capture stdout")?;
    let stderr = child.stderr.take().context("Failed to capture stderr")?;

    let stderr_tx = event_tx.clone();

    // Spawn task to read stderr
    let stderr_handle = tokio::spawn(async move {
        let reader = BufReader::new(stderr);
        let mut lines = reader.lines();

        while let Ok(Some(line)) = lines.next_line().await {
            if !line.is_empty() {
                tracing::warn!(stderr = %line, "Codex CLI stderr");

                // Check for session errors
                if line.contains("session") && line.contains("not found") {
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
            if let Some(events) = parse_codex_event(&json, &mut accumulated_text) {
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
                message: format!("Codex exited with status: {:?}", status.code()),
                recoverable: false,
            })
            .await;
    }

    // Send final result if we have accumulated text
    if !accumulated_text.is_empty() {
        let _ = event_tx
            .send(AgentEvent::Result {
                text: std::mem::take(&mut accumulated_text),
                usage: None,
                metadata: serde_json::json!({}),
            })
            .await;
    }

    if let Err(e) = stderr_handle.await {
        tracing::warn!(error = %e, "stderr reader task failed");
    }

    Ok(())
}

fn parse_codex_event(json: &Value, accumulated_text: &mut String) -> Option<Vec<AgentEvent>> {
    let mut events = Vec::new();

    let event_type = json.get("type").and_then(|t| t.as_str()).unwrap_or("");

    match event_type {
        // Thread started - contains thread_id which is the session ID
        "thread.started" => {
            if let Some(thread_id) = json.get("thread_id").and_then(|t| t.as_str()) {
                events.push(AgentEvent::SessionChanged {
                    new_session_id: thread_id.to_string(),
                });
            }
        }

        // Item completed - contains the actual content
        "item.completed" => {
            if let Some(item) = json.get("item") {
                let item_type = item.get("type").and_then(|t| t.as_str()).unwrap_or("");

                match item_type {
                    "agent_message" => {
                        if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
                            accumulated_text.push_str(text);
                            events.push(AgentEvent::Text(text.to_string()));
                        }
                    }
                    "reasoning" => {
                        // This is the **status** thinking text
                        if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
                            events.push(AgentEvent::Custom {
                                kind: "thinking".to_string(),
                                payload: serde_json::json!({ "status": text }),
                            });
                        }
                    }
                    "tool_call" => {
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
                        events.push(AgentEvent::ToolStart { id, name, input });
                    }
                    "tool_output" => {
                        let id = item
                            .get("tool_call_id")
                            .and_then(|i| i.as_str())
                            .unwrap_or("unknown")
                            .to_string();
                        let output = item.get("output").cloned().unwrap_or(Value::Null);
                        events.push(AgentEvent::ToolEnd {
                            id,
                            name: "".to_string(),
                            success: true,
                            output,
                            duration_ms: 0,
                        });
                    }
                    "error" => {
                        let message = item
                            .get("message")
                            .and_then(|m| m.as_str())
                            .unwrap_or("Unknown error")
                            .to_string();
                        // Don't emit as error event for warnings (like large directory)
                        if !message.contains("consider adding") {
                            events.push(AgentEvent::Error {
                                code: ErrorCode::BackendError,
                                message,
                                recoverable: false,
                            });
                        }
                    }
                    _ => {}
                }
            }
        }

        // Turn completed - contains usage stats
        "turn.completed" => {
            if let Some(usage) = json.get("usage") {
                let input_tokens = usage
                    .get("input_tokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                let output_tokens = usage
                    .get("output_tokens")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                let cache_read = usage.get("cached_input_tokens").and_then(|v| v.as_u64());

                events.push(AgentEvent::Result {
                    text: std::mem::take(accumulated_text),
                    usage: Some(crate::event::Usage {
                        input_tokens,
                        output_tokens,
                        cache_read_tokens: cache_read,
                        cache_write_tokens: None,
                        cost_usd: None,
                        extra: None,
                    }),
                    metadata: json.clone(),
                });
            }
        }

        _ => {}
    }

    if events.is_empty() {
        None
    } else {
        Some(events)
    }
}
