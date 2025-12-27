// ABOUTME: Mux backend - uses mux-rs for native Rust agent execution.
// ABOUTME: Provides streaming LLM responses with SQLite session persistence.

use crate::event::{AgentEvent, ErrorCode, Usage};
use crate::handle::{AgentHandle, Command};
use anyhow::{Context, Result};
use futures::StreamExt;
use mux::prelude::*;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
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

/// Session state with message history
#[derive(Clone)]
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

/// Serializable message format for SQLite storage
#[derive(Serialize, Deserialize)]
struct StoredMessage {
    role: String,
    content: Vec<StoredContentBlock>,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type")]
enum StoredContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        content: String,
        is_error: bool,
    },
}

/// SQLite-backed session store for mux backend
struct SessionDb {
    conn: Mutex<Connection>,
}

impl SessionDb {
    fn new(db_path: &std::path::Path) -> Result<Self> {
        let conn = Connection::open(db_path).context("Failed to open mux sessions database")?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS mux_sessions (
                session_id TEXT PRIMARY KEY,
                messages_json TEXT NOT NULL,
                system_prompt TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )",
            [],
        )?;

        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn save_session(&self, session_id: &str, session: &MuxSession) -> Result<()> {
        let messages: Vec<StoredMessage> = session
            .messages
            .iter()
            .map(|m| StoredMessage {
                role: match m.role {
                    Role::User => "user".to_string(),
                    Role::Assistant => "assistant".to_string(),
                },
                content: m
                    .content
                    .iter()
                    .filter_map(|c| match c {
                        ContentBlock::Text { text } => {
                            Some(StoredContentBlock::Text { text: text.clone() })
                        }
                        ContentBlock::ToolUse { id, name, input } => {
                            Some(StoredContentBlock::ToolUse {
                                id: id.clone(),
                                name: name.clone(),
                                input: input.clone(),
                            })
                        }
                        ContentBlock::ToolResult {
                            tool_use_id,
                            content,
                            is_error,
                        } => Some(StoredContentBlock::ToolResult {
                            tool_use_id: tool_use_id.clone(),
                            content: content.clone(),
                            is_error: *is_error,
                        }),
                    })
                    .collect(),
            })
            .collect();

        let messages_json = serde_json::to_string(&messages)?;
        let now = chrono::Utc::now().to_rfc3339();

        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("Database mutex poisoned: {}", e))?;

        conn.execute(
            "INSERT INTO mux_sessions (session_id, messages_json, system_prompt, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?4)
             ON CONFLICT(session_id) DO UPDATE SET
                messages_json = ?2,
                system_prompt = ?3,
                updated_at = ?4",
            params![session_id, messages_json, session.system_prompt, now],
        )?;

        tracing::debug!(session_id = %session_id, messages = messages.len(), "Session saved to database");
        Ok(())
    }

    fn load_session(&self, session_id: &str) -> Result<Option<MuxSession>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("Database mutex poisoned: {}", e))?;

        let mut stmt = conn.prepare(
            "SELECT messages_json, system_prompt FROM mux_sessions WHERE session_id = ?1",
        )?;

        let result = stmt.query_row(params![session_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
            ))
        });

        match result {
            Ok((messages_json, system_prompt)) => {
                let stored: Vec<StoredMessage> = serde_json::from_str(&messages_json)?;
                let messages: Vec<Message> = stored
                    .into_iter()
                    .map(|m| Message {
                        role: if m.role == "user" {
                            Role::User
                        } else {
                            Role::Assistant
                        },
                        content: m
                            .content
                            .into_iter()
                            .map(|c| match c {
                                StoredContentBlock::Text { text } => ContentBlock::Text { text },
                                StoredContentBlock::ToolUse { id, name, input } => {
                                    ContentBlock::ToolUse { id, name, input }
                                }
                                StoredContentBlock::ToolResult {
                                    tool_use_id,
                                    content,
                                    is_error,
                                } => ContentBlock::ToolResult {
                                    tool_use_id,
                                    content,
                                    is_error,
                                },
                            })
                            .collect(),
                    })
                    .collect();

                tracing::debug!(session_id = %session_id, messages = messages.len(), "Session loaded from database");
                Ok(Some(MuxSession {
                    messages,
                    system_prompt,
                }))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
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
                return AgentHandle::new(tx, name);
            }
        };

        // Create session database in working directory
        let db_path = config.working_dir.join(".mux_sessions.db");
        let session_db = match SessionDb::new(&db_path) {
            Ok(db) => Arc::new(db),
            Err(e) => {
                tracing::error!(error = %e, "Failed to create session database");
                return AgentHandle::new(tx, name);
            }
        };

        // In-memory session cache (backed by SQLite)
        let sessions: Arc<RwLock<HashMap<String, MuxSession>>> =
            Arc::new(RwLock::new(HashMap::new()));

        tokio::spawn(async move {
            while let Some(cmd) = rx.recv().await {
                match cmd {
                    Command::NewSession { reply } => {
                        let session_id = uuid::Uuid::new_v4().to_string();
                        let system_prompt = build_system_prompt(&config);
                        let session = MuxSession::new(system_prompt);

                        // Save to database immediately
                        if let Err(e) = session_db.save_session(&session_id, &session) {
                            tracing::error!(error = %e, "Failed to save new session");
                        }

                        sessions.write().await.insert(session_id.clone(), session);
                        let _ = reply.send(Ok(session_id));
                    }
                    Command::LoadSession {
                        session_id, reply, ..
                    } => {
                        // Check in-memory cache first
                        let exists = sessions.read().await.contains_key(&session_id);
                        if exists {
                            let _ = reply.send(Ok(()));
                            continue;
                        }

                        // Try to load from database
                        match session_db.load_session(&session_id) {
                            Ok(Some(session)) => {
                                sessions.write().await.insert(session_id.clone(), session);
                                let _ = reply.send(Ok(()));
                            }
                            Ok(None) => {
                                // Session doesn't exist - create new one
                                let system_prompt = build_system_prompt(&config);
                                let session = MuxSession::new(system_prompt);
                                if let Err(e) = session_db.save_session(&session_id, &session) {
                                    tracing::error!(error = %e, "Failed to save new session");
                                }
                                sessions.write().await.insert(session_id.clone(), session);
                                let _ = reply.send(Ok(()));
                            }
                            Err(e) => {
                                tracing::error!(error = %e, "Failed to load session");
                                let _ = reply.send(Err(e));
                            }
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
                        let session_db = Arc::clone(&session_db);
                        let config = config.clone();

                        tokio::spawn(async move {
                            if let Err(e) = run_prompt(
                                &client,
                                &sessions,
                                &session_db,
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
                break;
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
    session_db: &Arc<SessionDb>,
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

    // Update session with assistant response and persist
    if !accumulated_text.is_empty() {
        let mut sessions_guard = sessions.write().await;
        if let Some(session) = sessions_guard.get_mut(session_id) {
            session.messages.push(Message {
                role: Role::Assistant,
                content: vec![ContentBlock::Text {
                    text: accumulated_text.clone(),
                }],
            });

            // Persist to database
            if let Err(e) = session_db.save_session(session_id, session) {
                tracing::error!(error = %e, "Failed to persist session after prompt");
            }
        }
    }

    // Send final result
    let usage = final_usage.map(|u| Usage {
        input_tokens: u.input_tokens as u64,
        output_tokens: u.output_tokens as u64,
        cache_read_tokens: None,
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
        _ => (ErrorCode::BackendError, error.to_string(), false),
    };

    AgentEvent::Error {
        code,
        message,
        recoverable,
    }
}

mod dirs {
    pub fn home_dir() -> Option<std::path::PathBuf> {
        std::env::var_os("HOME").map(std::path::PathBuf::from)
    }
}

mod chrono {
    pub struct Utc;
    impl Utc {
        pub fn now() -> DateTime {
            DateTime
        }
    }
    pub struct DateTime;
    impl DateTime {
        pub fn to_rfc3339(&self) -> String {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap();
            format!("{}", now.as_secs())
        }
    }
}
