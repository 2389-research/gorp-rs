// ABOUTME: Mux backend - uses mux-rs for native Rust agent execution.
// ABOUTME: Provides streaming LLM responses with tool execution support.

use crate::event::{AgentEvent, ErrorCode, Usage};
use crate::handle::{AgentHandle, Command};
use anyhow::{Context, Result};
use futures::StreamExt;
use mux::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};

/// Configuration for the Mux backend
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MuxConfig {
    /// Model to use (e.g., "claude-sonnet-4-20250514")
    pub model: String,
    /// Maximum tokens for response
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
    /// Working directory for the agent
    pub working_dir: PathBuf,
    /// Path to global system prompt file (e.g., ~/.mux/system.md)
    pub global_system_prompt_path: Option<PathBuf>,
    /// Filenames to look for local system prompts
    #[serde(default = "default_local_prompt_files")]
    pub local_prompt_files: Vec<String>,
}

fn default_max_tokens() -> u32 {
    8192
}

fn default_local_prompt_files() -> Vec<String> {
    vec![
        "claude.md".to_string(),
        "CLAUDE.md".to_string(),
        "agent.md".to_string(),
    ]
}

/// In-memory session state
struct MuxSession {
    messages: Vec<Message>,
    system_prompt: Option<String>,
}

impl MuxSession {
    fn new(system_prompt: Option<String>) -> Self {
        Self {
            messages: Vec::new(),
            system_prompt,
        }
    }
}

pub struct MuxBackend {
    config: MuxConfig,
}

impl MuxBackend {
    pub fn new(config: MuxConfig) -> Result<Self> {
        Ok(Self { config })
    }

    pub fn into_handle(self) -> AgentHandle {
        let (tx, mut rx) = mpsc::channel::<Command>(32);
        let name = "mux";
        let config = self.config;

        // Create the Anthropic client
        let client = match AnthropicClient::from_env() {
            Ok(c) => Arc::new(c),
            Err(e) => {
                tracing::error!(error = %e, "Failed to create Anthropic client");
                // Return a handle that will error on all operations
                return AgentHandle::new(tx, name);
            }
        };

        // Shared session storage
        let sessions: Arc<RwLock<HashMap<String, MuxSession>>> =
            Arc::new(RwLock::new(HashMap::new()));

        tokio::spawn(async move {
            while let Some(cmd) = rx.recv().await {
                match cmd {
                    Command::NewSession { reply } => {
                        let session_id = uuid::Uuid::new_v4().to_string();

                        // Build system prompt for this session
                        let system_prompt = build_system_prompt(&config);

                        // Create new session
                        let session = MuxSession::new(system_prompt);
                        sessions.write().await.insert(session_id.clone(), session);

                        let _ = reply.send(Ok(session_id));
                    }
                    Command::LoadSession {
                        session_id, reply, ..
                    } => {
                        // For now, just check if session exists
                        // Phase 2 will load from SQLite
                        let exists = sessions.read().await.contains_key(&session_id);
                        if exists {
                            let _ = reply.send(Ok(()));
                        } else {
                            // Create empty session if not found (new behavior)
                            let system_prompt = build_system_prompt(&config);
                            let session = MuxSession::new(system_prompt);
                            sessions.write().await.insert(session_id.clone(), session);
                            let _ = reply.send(Ok(()));
                        }
                    }
                    Command::Prompt {
                        session_id,
                        text,
                        event_tx,
                        reply,
                        ..
                    } => {
                        let _ = reply.send(Ok(()));

                        let client = Arc::clone(&client);
                        let sessions = Arc::clone(&sessions);
                        let config = config.clone();

                        tokio::spawn(async move {
                            if let Err(e) = run_prompt(
                                &client,
                                &sessions,
                                &config,
                                &session_id,
                                &text,
                                event_tx,
                            )
                            .await
                            {
                                tracing::error!(error = %e, "Mux prompt failed");
                            }
                        });
                    }
                    Command::Cancel { reply, .. } => {
                        // Cancellation happens by dropping the stream
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
            let cfg: MuxConfig = serde_json::from_value(config.clone())?;
            let backend = MuxBackend::new(cfg)?;
            Ok(backend.into_handle())
        })
    }
}

/// Build system prompt from global and local files
fn build_system_prompt(config: &MuxConfig) -> Option<String> {
    let mut parts = Vec::new();

    // 1. Global system prompt (~/.mux/system.md or configured path)
    if let Some(ref global_path) = config.global_system_prompt_path {
        if let Ok(content) = std::fs::read_to_string(global_path) {
            if !content.trim().is_empty() {
                parts.push(content);
            }
        }
    } else {
        // Default to ~/.mux/system.md
        if let Some(home) = dirs::home_dir() {
            let default_path = home.join(".mux").join("system.md");
            if let Ok(content) = std::fs::read_to_string(default_path) {
                if !content.trim().is_empty() {
                    parts.push(content);
                }
            }
        }
    }

    // 2. Local system prompt (claude.md, agent.md, etc. in working_dir)
    for filename in &config.local_prompt_files {
        let local_path = config.working_dir.join(filename);
        if let Ok(content) = std::fs::read_to_string(&local_path) {
            if !content.trim().is_empty() {
                parts.push(content);
                break; // Use first found
            }
        }
    }

    if parts.is_empty() {
        None
    } else {
        Some(parts.join("\n\n---\n\n"))
    }
}

async fn run_prompt(
    client: &AnthropicClient,
    sessions: &Arc<RwLock<HashMap<String, MuxSession>>>,
    config: &MuxConfig,
    session_id: &str,
    text: &str,
    event_tx: mpsc::Sender<AgentEvent>,
) -> Result<()> {
    // Get session and add user message
    let (messages, system_prompt) = {
        let mut sessions_guard = sessions.write().await;
        let session = sessions_guard
            .get_mut(session_id)
            .context("Session not found")?;

        // Add user message
        session.messages.push(Message {
            role: Role::User,
            content: vec![ContentBlock::Text {
                text: text.to_string(),
            }],
        });

        (session.messages.clone(), session.system_prompt.clone())
    };

    // Build request
    let request = Request {
        model: config.model.clone(),
        messages,
        tools: Vec::new(), // Phase 4 will add tools
        max_tokens: Some(config.max_tokens),
        system: system_prompt,
        temperature: None,
    };

    // Use streaming API
    let mut stream = client.create_message_stream(&request);
    let mut accumulated_text = String::new();
    let mut final_usage: Option<mux::llm::Usage> = None;

    while let Some(event_result) = stream.next().await {
        match event_result {
            Ok(event) => {
                if let Some(agent_event) = translate_stream_event(&event, &mut accumulated_text) {
                    if event_tx.send(agent_event).await.is_err() {
                        tracing::debug!("Event receiver closed, stopping stream");
                        break;
                    }
                }

                // Capture usage from MessageDelta
                if let StreamEvent::MessageDelta { usage, .. } = &event {
                    final_usage = Some(usage.clone());
                }
            }
            Err(e) => {
                let error_event = translate_llm_error(&e);
                let _ = event_tx.send(error_event).await;
                return Err(e.into());
            }
        }
    }

    // Update session with assistant response
    if !accumulated_text.is_empty() {
        let mut sessions_guard = sessions.write().await;
        if let Some(session) = sessions_guard.get_mut(session_id) {
            session.messages.push(Message {
                role: Role::Assistant,
                content: vec![ContentBlock::Text {
                    text: accumulated_text.clone(),
                }],
            });
        }
    }

    // Send final result
    let usage = final_usage.map(|u| Usage {
        input_tokens: u.input_tokens as u64,
        output_tokens: u.output_tokens as u64,
        cache_read_tokens: None,  // mux doesn't track cache tokens
        cache_write_tokens: None,
        cost_usd: None,
        extra: None,
    });

    let _ = event_tx
        .send(AgentEvent::Result {
            text: accumulated_text,
            usage,
            metadata: serde_json::Value::Null,
        })
        .await;

    Ok(())
}

/// Translate mux StreamEvent to gorp AgentEvent
fn translate_stream_event(
    event: &StreamEvent,
    accumulated_text: &mut String,
) -> Option<AgentEvent> {
    match event {
        StreamEvent::ContentBlockDelta { text, .. } => {
            accumulated_text.push_str(text);
            Some(AgentEvent::Text(text.clone()))
        }
        StreamEvent::ContentBlockStart { block, .. } => {
            if let ContentBlock::ToolUse { id, name, input } = block {
                Some(AgentEvent::ToolStart {
                    id: id.clone(),
                    name: name.clone(),
                    input: input.clone(),
                })
            } else {
                None
            }
        }
        // Other events don't map directly
        StreamEvent::MessageStart { .. }
        | StreamEvent::ContentBlockStop { .. }
        | StreamEvent::MessageDelta { .. }
        | StreamEvent::MessageStop => None,
    }
}

/// Translate mux LlmError to gorp AgentEvent
fn translate_llm_error(error: &LlmError) -> AgentEvent {
    let (code, message, recoverable) = match error {
        LlmError::Http(e) => (ErrorCode::BackendError, e.to_string(), true),
        LlmError::Api { message, .. } => {
            let code = if message.contains("rate_limit") {
                ErrorCode::RateLimited
            } else if message.contains("authentication") || message.contains("invalid_api_key") {
                ErrorCode::AuthFailed
            } else {
                ErrorCode::BackendError
            };
            (code, message.clone(), code == ErrorCode::RateLimited)
        }
        LlmError::StreamClosed => (
            ErrorCode::BackendError,
            "Stream closed unexpectedly".to_string(),
            false,
        ),
        // Handle any other error variants
        _ => (ErrorCode::BackendError, error.to_string(), false),
    };

    AgentEvent::Error {
        code,
        message,
        recoverable,
    }
}

// Need dirs crate for home_dir - add to dependencies
mod dirs {
    pub fn home_dir() -> Option<std::path::PathBuf> {
        std::env::var_os("HOME").map(std::path::PathBuf::from)
    }
}
